// /preview/<port>(/<path>) - dynamic reverse proxy to a user-started dev
// server (issue #1, web-type user buttons).
//
// Dev servers live in the terminals spawned by THIS app (pty.rs), so they bind
// on the shared netns loopback - the proxy target is always `127.0.0.1:<port>`
// (a Caddy-side route could not reach a loopback-bound server, which is why
// the proxy lives here rather than in the gateway; the gateway's catch-all
// already hands /preview/* over to axum, so Caddyfile/compose are untouched).
//
// Liveness for the sidebar button is handled elsewhere: buttons.toml web
// entries TCP-probe `127.0.0.1:<port>` from config.rs, identical to the
// built-in code-server/vnc buttons. This module only does the forwarding:
//   - plain HTTP: method/headers/body forwarded, response streamed unbuffered
//     (SSE / chunked survive); no auto-decompression (reqwest was built
//     without gzip/brotli features) so bytes pass through untouched;
//   - WebSocket upgrades (vite HMR & co): upgraded on both sides and pumped
//     message-by-message until either side closes.
//
// Known boundary (documented in the README): apps that emit ROOT-absolute
// asset URLs (vite's default) break under ANY subpath proxy - same reason
// pi-web needed a dedicated origin. The proxy does no HTML rewriting; such
// apps configure `base` + `server.hmr.path` to `/preview/<port>/` upstream.

use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{ws::WebSocketUpgrade, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::state::AppState;

/// Ports the proxy refuses. 0 is meaningless; 8088 is axum itself (proxying
/// it would recurse: /preview/8088/preview/...). Mirrors the registration-time
/// rejection in routes/buttons.rs.
fn port_allowed(port: u16) -> bool {
    port != 0 && port != 8088
}

/// Strip the `/preview/<port>` prefix off the request path. The bare path
/// (`/preview/5173`) and the trailing-slash form (`/preview/5173/`) both map
/// to `/` - the dev-server root. Query strings are carried by the caller from
/// the original Uri.
fn upstream_path(full_path: &str, port: u16) -> String {
    let prefix = format!("/preview/{port}");
    let rest = full_path.strip_prefix(&prefix).unwrap_or("");
    if rest.is_empty() || rest == "/" {
        "/".to_string()
    } else {
        rest.to_string()
    }
}

/// Hop-by-hop headers (RFC 7230 §6.1): they describe the single connection and
/// must not survive a proxy hop. `upgrade` is in the list, but WS requests
/// never reach this filter (they are diverted before the HTTP path).
fn is_hop_by_hop(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection" | "keep-alive" | "proxy-authenticate" | "proxy-authorization" | "te"
            | "trailer" | "transfer-encoding" | "upgrade"
    )
}

/// Budget for the upstream WS handshake: a dev server that is not up must
/// fail the pane fast, not hang the browser.
///
/// (The HTTP layer needs no timeout - the shared reqwest client is built
/// without one so SSE can stream indefinitely.)
const WS_CONNECT_TIMEOUT: Duration = Duration::from_millis(2000);

pub async fn preview_proxy(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    // Extractor order matters: WebSocketUpgrade is FromRequestParts (and
    // yields None on non-WS requests); Bytes consumes the body, so it must
    // be the LAST extractor.
    ws: Option<WebSocketUpgrade>,
    body: Bytes,
) -> Response {
    // Parse + validate the port. Non-numeric or forbidden ports fail fast with
    // 404 (an upstream that is not there), never a proxy attempt.
    let Some(seg) = uri.path().strip_prefix("/preview/").and_then(|r| r.split('/').next()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(port) = seg.parse::<u16>() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !port_allowed(port) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = upstream_path(uri.path(), port);
    let path_and_query = match uri.query() {
        Some(q) => format!("{path}?{q}"),
        None => path,
    };

    // WS upgrade requests divert to the tunnel path before any HTTP
    // forwarding (the Upgrade header is hop-by-hop and must not be re-sent).
    if let Some(ws) = ws {
        if headers
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        {
            return proxy_ws(ws, port, &path_and_query, &headers).await;
        }
    }

    proxy_http(&state, method, &headers, body, port, &path_and_query).await
}

/// Plain HTTP forwarding: build the upstream request from the incoming parts,
/// stream the response back unbuffered.
async fn proxy_http(
    state: &AppState,
    method: Method,
    headers: &HeaderMap,
    body: Bytes,
    port: u16,
    path_and_query: &str,
) -> Response {
    let url = format!("http://127.0.0.1:{port}{path_and_query}");
    let mut req = state.http.request(method, &url);
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name) || name == header::HOST {
            // hyper sets Host from the URL (127.0.0.1:<port>) - the standard
            // loopback-proxy semantic, and what pi-web's IP-literal trust
            // already expects.
            continue;
        }
        req = req.header(name, value);
    }

    let resp = match req.body(body).send().await {
        Ok(r) => r,
        Err(e) => {
            // Connect refused / timeout: the dev server is not (yet) up.
            tracing::debug!("preview upstream {url} failed: {e}");
            return (StatusCode::BAD_GATEWAY, format!("upstream {url} unreachable")).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (name, value) in resp.headers().iter() {
        if is_hop_by_hop(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    // Stream the body through untouched (SSE / chunked / large files).
    builder
        .body(Body::from_stream(resp.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// WebSocket tunneling: accept the client upgrade, connect upstream with
/// tokio-tungstenite (plaintext - always 127.0.0.1), then pump messages both
/// ways until either side closes. The subprotocol header is forwarded so
/// protocol-negotiated clients (vite HMR uses `vite-hmr`) keep working.
async fn proxy_ws(
    mut ws: WebSocketUpgrade,
    port: u16,
    path_and_query: &str,
    headers: &HeaderMap,
) -> Response {
    let url = format!("ws://127.0.0.1:{port}{path_and_query}");
    let mut request = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("preview ws bad request url {url}: {e}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    if let Some(proto) = headers.get(header::SEC_WEBSOCKET_PROTOCOL) {
        request.headers_mut().insert(header::SEC_WEBSOCKET_PROTOCOL, proto.clone());
    }

    let upstream = match tokio::time::timeout(
        WS_CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async(request),
    )
    .await
    {
        Ok(Ok((stream, resp))) => (stream, resp),
        Ok(Err(e)) => {
            tracing::debug!("preview ws upstream {url} failed: {e}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
        Err(_) => {
            tracing::debug!("preview ws upstream {url} timed out");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    let (upstream, up_resp) = upstream;

    // The upstream may have negotiated a subprotocol from the ones we
    // forwarded; the 101 back to the browser must echo the SAME choice or
    // protocol-aware clients (vite HMR negotiates `vite-hmr`) abort.
    let protos: Vec<String> = up_resp
        .headers()
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !protos.is_empty() {
        ws = ws.protocols(protos);
    }

    ws.on_upgrade(move |client| async move {
        let (mut up_sink, mut up_stream) = upstream.split();
        let (mut cl_sink, mut cl_stream) = client.split();

        // Client -> upstream. Ended by a close frame, an error, or the
        // upstream side's task dropping on its own termination.
        let to_upstream = async move {
            while let Some(Ok(msg)) = cl_stream.next().await {
                let msg = match msg {
                    axum::extract::ws::Message::Text(t) => tokio_tungstenite::tungstenite::Message::Text(t),
                    axum::extract::ws::Message::Binary(b) => tokio_tungstenite::tungstenite::Message::Binary(b.to_vec()),
                    axum::extract::ws::Message::Ping(p) => tokio_tungstenite::tungstenite::Message::Ping(p.to_vec()),
                    axum::extract::ws::Message::Pong(p) => tokio_tungstenite::tungstenite::Message::Pong(p.to_vec()),
                    axum::extract::ws::Message::Close(c) => tokio_tungstenite::tungstenite::Message::Close(c.map(|f| {
                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason.into(),
                        }
                    })),
                };
                if up_sink.send(msg).await.is_err() {
                    break;
                }
            }
        };

        // Upstream -> client.
        let to_client = async move {
            while let Some(Ok(msg)) = up_stream.next().await {
                let msg = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => axum::extract::ws::Message::Text(t),
                    tokio_tungstenite::tungstenite::Message::Binary(b) => axum::extract::ws::Message::Binary(b),
                    tokio_tungstenite::tungstenite::Message::Ping(p) => axum::extract::ws::Message::Ping(p),
                    tokio_tungstenite::tungstenite::Message::Pong(p) => axum::extract::ws::Message::Pong(p),
                    tokio_tungstenite::tungstenite::Message::Close(c) => axum::extract::ws::Message::Close(c.map(|f| {
                        axum::extract::ws::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason,
                        }
                    })),
                    // Raw frames carry extensions this proxy does not
                    // negotiate - drop them silently.
                    tokio_tungstenite::tungstenite::Message::Frame(_) => continue,
                };
                if cl_sink.send(msg).await.is_err() {
                    break;
                }
            }
        };

        tokio::join!(to_upstream, to_client);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_allowed_rejects_zero_and_self() {
        assert!(!port_allowed(0));
        assert!(!port_allowed(8088)); // axum itself - recursion guard
        assert!(port_allowed(5173));
        assert!(port_allowed(1));
        assert!(port_allowed(65535));
    }

    #[test]
    fn upstream_path_strips_prefix() {
        assert_eq!(upstream_path("/preview/5173", 5173), "/");
        assert_eq!(upstream_path("/preview/5173/", 5173), "/");
        assert_eq!(upstream_path("/preview/5173/src/main.tsx", 5173), "/src/main.tsx");
        assert_eq!(upstream_path("/preview/5173/@vite/client", 5173), "/@vite/client");
        // A different port in the path must not be stripped (defensive - the
        // router only routes matching :port segments here).
        assert_eq!(upstream_path("/preview/5173/preview/8088", 5173), "/preview/8088");
    }

    #[test]
    fn hop_by_hop_filter_matches_rfc_7230() {
        let mk = |s: &str| header::HeaderName::from_bytes(s.as_bytes()).unwrap();
        for h in ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"] {
            assert!(is_hop_by_hop(&mk(h)), "{h} must be stripped");
        }
        assert!(!is_hop_by_hop(&mk("content-type")));
        assert!(!is_hop_by_hop(&mk("sec-websocket-key")));
        assert!(!is_hop_by_hop(&mk("set-cookie")));
    }
}
