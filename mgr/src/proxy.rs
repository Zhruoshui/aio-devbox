// /api/sbx/:name/* - per-sandbox reverse proxy to the sandbox app's axum
// port (sandbox-mgr unified Phase 1, design §1; prd D6/D7).
//
// mgr-web's workbench panes reach a sandbox's app API through mgr-api
// instead of the browser cross-origin dialing sbx-<name>-piweb directly:
// adopted stacks' app images send no CORS headers (prd D6), and funnelling
// every sandbox surface through the mgr.localhost origin leaves a single
// place to attach auth later. Forwarded surfaces (design §0):
// /api/manifest, /api/buttons* + probe, /api/term/ws (pty), /preview/:port/*
// (user web buttons) - HTTP and WebSocket alike.
//
// Host derivation is CLOSED: the upstream is always
// `http://sbx-<name>-piweb:8088/<path>` with <name> taken from the route,
// which must pass routes::validate_name AND exist in the sandboxes table
// (native and adopted rows alike - adopt network-connects the same alias,
// 契约 8). No request parameter ever reaches the upstream HOST, so there is
// no SSRF surface - only a name-keyed map over registered sandboxes. The
// alias is the app service's aio-mgr-net alias (composegen / adopt), NOT
// the gateway's `sbx-<name>` - the same choice as the /api/usage fan-out
// (usage.rs module comment: mgr talks to the app DIRECTLY, not through the
// sandbox gateway).
//
// No liveness filtering: a stopped sandbox fails to connect and the error
// surfaces as 502 (mgr-web greys stopped sandboxes out at the tree level;
// the proxy does not double-guess - design §1).
//
// Implementation follows the app's /preview proxy (app/src/routes/
// preview.rs) with two mgr-specific deviations, both deliberate:
//   - errors render in the mgr ApiError JSON shape ({"error": ...}) instead
//     of preview's plain text (mgr-web's apiError decodes JSON only);
//   - an unknown :name is a real 404 (the proxied upstream does not exist),
//     not the lifecycle handlers' 400-shaped "not found" (routes.rs
//     require_row) - proxy semantics, same body shape.
//
// Route note: only `/api/sbx/:name/*path` is registered (design §1) -
// matchit 0.7.3 catch-alls need a non-empty tail. The bare `/api/sbx/<name>`
// backtracks to the `/api/*rest` seam 404; the trailing-slash form
// `/api/sbx/<name>/` matches NOTHING (a partially-walked subtree does not
// backtrack to a sibling catch-all - same gap as `/api/sandboxes/`) and is
// answered by the router's default fallback (routes.rs unmatched_fallback),
// which is also what keeps it off the production SPA. Nothing upstream
// lives at the app root worth proxying either way.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{ws::WebSocketUpgrade, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::db;
use crate::routes::ApiError;
use crate::state::AppState;
use crate::usage::APP_PORT;

/// Budget for the upstream WS handshake (preview.rs pattern): a sandbox
/// that is down must fail the pane fast, not hang the browser. The HTTP
/// leg needs no timeout - the shared reqwest client is built without one
/// (state.rs) so proxied SSE / long-poll responses stream indefinitely.
const WS_CONNECT_TIMEOUT: Duration = Duration::from_millis(2000);

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/sbx/:name/*path", any(proxy))
}

/// Split `/api/sbx/<name>/<rest>` into (name, rest). `rest` keeps its
/// leading slash; the trailing-slash form normalizes to `/` (preview.rs
/// upstream_path semantics). None when the path does not start with the
/// proxy prefix (defensive - the route only feeds matching paths in).
///
/// The name is taken from the RAW (still percent-encoded) path on purpose:
/// the slug alphabet [a-z0-9-] never needs percent-decoding, so any escape
/// sequence in a name is by definition not a registered sandbox and is
/// rejected by validate_name without ever building an upstream host from it.
fn split_proxy_path(full_path: &str) -> Option<(String, String)> {
    let tail = full_path.strip_prefix("/api/sbx/")?;
    if tail.is_empty() {
        return None; // "/api/sbx/" - no name segment (defensive; unrouted)
    }
    let (name, rest) = match tail.split_once('/') {
        Some((n, r)) if !r.is_empty() => (n, format!("/{r}")),
        Some((n, _)) => (n, "/".to_string()),
        None => (tail, "/".to_string()),
    };
    Some((name.to_string(), rest))
}

/// The upstream URL, ALWAYS derived from the validated sandbox name:
/// `sbx-<name>-piweb` on aio-mgr-net, the app's axum port (usage.rs
/// APP_PORT - single owner of the alias:port pairing). `scheme` is "http"
/// or "ws".
fn upstream_url(scheme: &str, name: &str, path_and_query: &str) -> String {
    format!("{scheme}://sbx-{name}-piweb:{APP_PORT}{path_and_query}")
}

/// Hop-by-hop headers (RFC 7230 §6.1): they describe the single connection
/// and must not survive a proxy hop. `upgrade` is in the list, but WS
/// requests never reach this filter (they are diverted before the HTTP
/// path). Byte-identical to preview.rs's filter.
fn is_hop_by_hop(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

async fn proxy(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    // Extractor order matters: WebSocketUpgrade is FromRequestParts (and
    // yields None on non-WS requests); Bytes consumes the body, so it must
    // be the LAST extractor.
    ws: Option<WebSocketUpgrade>,
    body: Bytes,
) -> Response {
    let Some((name, path)) = split_proxy_path(uri.path()) else {
        return ApiError::with_status(StatusCode::NOT_FOUND, "no such API route".to_string())
            .into_response();
    };
    if let Err(msg) = crate::routes::validate_name(&name) {
        return ApiError::with_status(StatusCode::BAD_REQUEST, msg).into_response();
    }
    // Registered-sandbox gate (the require_row pattern, routes.rs): only
    // names the DB knows may be dialed. Not-found is a real 404 here (see
    // module comment); the row itself is unused - the alias derives from
    // the name, and liveness is deliberately NOT checked (design §1).
    {
        let conn = state.db.lock().unwrap();
        match db::get_sandbox(&conn, &name) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return ApiError::with_status(
                    StatusCode::NOT_FOUND,
                    format!("sandbox {name:?} not found"),
                )
                .into_response();
            }
            Err(e) => return ApiError::from(e).into_response(),
        }
    }

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
            return proxy_ws(ws, &name, &path_and_query, &headers).await;
        }
    }

    proxy_http(&state, method, &headers, body, &name, &path_and_query).await
}

/// Plain HTTP forwarding: build the upstream request from the incoming
/// parts, stream the response back unbuffered (preview.rs pattern).
async fn proxy_http(
    state: &AppState,
    method: Method,
    headers: &HeaderMap,
    body: Bytes,
    name: &str,
    path_and_query: &str,
) -> Response {
    let url = upstream_url("http", name, path_and_query);
    let mut req = state.http.request(method, &url);
    for (hname, hvalue) in headers.iter() {
        if is_hop_by_hop(hname) || hname == header::HOST {
            // hyper sets Host from the URL (sbx-<name>-piweb:8088). The app
            // does no host filtering (that is pi-web's middleware, and
            // pi-web is reached via its own subdomain, 契约 3 - never here).
            continue;
        }
        req = req.header(hname, hvalue);
    }

    let resp = match req.body(body).send().await {
        Ok(r) => r,
        Err(e) => {
            // Connect refused / DNS miss / timeout: the sandbox is stopped
            // or gone. 502 in the mgr JSON shape (mgr-web decodes {"error"}),
            // never a hang and never a masked 404.
            tracing::debug!("sbx proxy upstream {url} failed: {e}");
            return ApiError::with_status(
                StatusCode::BAD_GATEWAY,
                format!("upstream unreachable: {e}"),
            )
            .into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (hname, hvalue) in resp.headers().iter() {
        if is_hop_by_hop(hname) {
            continue;
        }
        builder = builder.header(hname, hvalue);
    }
    // Stream the body through untouched (SSE / chunked / large files).
    builder
        .body(Body::from_stream(resp.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// WebSocket tunneling (preview.rs pattern): accept the client upgrade,
/// connect upstream with tokio-tungstenite (plaintext - aio-mgr-net
/// internal), then pump messages both ways until either side closes. The
/// negotiated subprotocol is echoed back so protocol-aware clients keep
/// working.
async fn proxy_ws(
    mut ws: WebSocketUpgrade,
    name: &str,
    path_and_query: &str,
    headers: &HeaderMap,
) -> Response {
    let url = upstream_url("ws", name, path_and_query);
    let mut request = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("sbx proxy ws bad request url {url}: {e}");
            return ApiError::with_status(
                StatusCode::BAD_GATEWAY,
                format!("upstream unreachable: {e}"),
            )
            .into_response();
        }
    };
    if let Some(proto) = headers.get(header::SEC_WEBSOCKET_PROTOCOL) {
        request
            .headers_mut()
            .insert(header::SEC_WEBSOCKET_PROTOCOL, proto.clone());
    }

    let upstream = match tokio::time::timeout(
        WS_CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async(request),
    )
    .await
    {
        Ok(Ok((stream, resp))) => (stream, resp),
        Ok(Err(e)) => {
            tracing::debug!("sbx proxy ws upstream {url} failed: {e}");
            return ApiError::with_status(
                StatusCode::BAD_GATEWAY,
                format!("upstream unreachable: {e}"),
            )
            .into_response();
        }
        Err(_) => {
            tracing::debug!("sbx proxy ws upstream {url} timed out");
            return ApiError::with_status(
                StatusCode::BAD_GATEWAY,
                format!("upstream {url} timed out"),
            )
            .into_response();
        }
    };

    let (upstream, up_resp) = upstream;

    // The upstream may have negotiated a subprotocol from the ones we
    // forwarded; the 101 back to the browser must echo the SAME choice or
    // protocol-aware clients abort.
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

        // Client -> upstream (terminal keystrokes as Text frames, the pty
        // resize control frame as Binary). Ended by a close frame, an error,
        // or the other side's task dropping on its own termination.
        let to_upstream = async move {
            while let Some(Ok(msg)) = cl_stream.next().await {
                let msg = match msg {
                    axum::extract::ws::Message::Text(t) => {
                        tokio_tungstenite::tungstenite::Message::Text(t)
                    }
                    axum::extract::ws::Message::Binary(b) => {
                        tokio_tungstenite::tungstenite::Message::Binary(b.to_vec())
                    }
                    axum::extract::ws::Message::Ping(p) => {
                        tokio_tungstenite::tungstenite::Message::Ping(p.to_vec())
                    }
                    axum::extract::ws::Message::Pong(p) => {
                        tokio_tungstenite::tungstenite::Message::Pong(p.to_vec())
                    }
                    axum::extract::ws::Message::Close(c) => {
                        tokio_tungstenite::tungstenite::Message::Close(c.map(|f| {
                            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: f.code.into(),
                                reason: f.reason,
                            }
                        }))
                    }
                };
                if up_sink.send(msg).await.is_err() {
                    break;
                }
            }
        };

        // Upstream -> client (pty output).
        let to_client = async move {
            while let Some(Ok(msg)) = up_stream.next().await {
                let msg = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => {
                        axum::extract::ws::Message::Text(t)
                    }
                    tokio_tungstenite::tungstenite::Message::Binary(b) => {
                        axum::extract::ws::Message::Binary(b)
                    }
                    tokio_tungstenite::tungstenite::Message::Ping(p) => {
                        axum::extract::ws::Message::Ping(p)
                    }
                    tokio_tungstenite::tungstenite::Message::Pong(p) => {
                        axum::extract::ws::Message::Pong(p)
                    }
                    tokio_tungstenite::tungstenite::Message::Close(c) => {
                        axum::extract::ws::Message::Close(c.map(|f| {
                            axum::extract::ws::CloseFrame {
                                code: f.code.into(),
                                reason: f.reason,
                            }
                        }))
                    }
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

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn insert_sandbox(state: &AppState, name: &str, adopted: bool) {
        let conn = state.db.lock().unwrap();
        db::insert_sandbox(
            &conn,
            &db::SandboxRow {
                name: name.into(),
                created_at: 0,
                env_json: "{}".into(),
                env_hash: String::new(),
                cpus: None,
                mem_mb: None,
                status: "running".into(),
                adopted,
                external_compose: None,
                services_json: None,
            },
        )
        .expect("insert test sandbox");
    }

    /// Serve the REAL top-level router (routes::router already merges the
    /// proxy) on an ephemeral port and return its base URL. mgr has no tower
    /// dev-dependency for Router::oneshot, so integration assertions go over
    /// a real socket with the same shared reqwest client the handlers use.
    async fn serve(state: &Arc<AppState>) -> String {
        let app = crate::routes::router().with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    #[test]
    fn split_proxy_path_forms() {
        // Deep paths keep their leading slash and query stays with the caller.
        let (name, rest) = split_proxy_path("/api/sbx/alpha/api/manifest").unwrap();
        assert_eq!((name.as_str(), rest.as_str()), ("alpha", "/api/manifest"));
        let (name, rest) = split_proxy_path("/api/sbx/alpha/preview/3000/x").unwrap();
        assert_eq!((name.as_str(), rest.as_str()), ("alpha", "/preview/3000/x"));
        // Trailing-slash -> app root.
        let (name, rest) = split_proxy_path("/api/sbx/alpha/").unwrap();
        assert_eq!((name.as_str(), rest.as_str()), ("alpha", "/"));
        // Non-matching prefixes never produce a name (defensive branch).
        assert!(split_proxy_path("/api/sandboxes").is_none());
        assert!(split_proxy_path("/api/sbx/").is_none());
    }

    #[test]
    fn upstream_url_is_name_derived_only() {
        // The alias is the APP service's (sbx-<name>-piweb, NOT the gateway's
        // sbx-<name>) on the app's axum port - the usage.rs fan-out pairing.
        assert_eq!(
            upstream_url("http", "alpha", "/api/manifest"),
            "http://sbx-alpha-piweb:8088/api/manifest"
        );
        assert_eq!(
            upstream_url("ws", "a-b1", "/api/term/ws?cmd=pi"),
            "ws://sbx-a-b1-piweb:8088/api/term/ws?cmd=pi"
        );
    }

    #[tokio::test]
    async fn proxy_route_wins_over_api_seam() {
        // A /api/sbx/... request must be answered by the PROXY (unknown
        // sandbox -> "sandbox ... not found"), not swallowed by the /api/*rest
        // seam ("no such API route") - and the seam itself must keep 404ing
        // unrelated paths as before.
        let state = Arc::new(AppState::new_for_test());
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/sbx/ghost/api/manifest"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(
            v["error"], "sandbox \"ghost\" not found",
            "proxy answered, not the seam"
        );

        let r = state
            .http
            .get(format!("{base}/api/nonexistent"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["error"], "no such API route", "seam behavior unchanged");
    }

    #[tokio::test]
    async fn unknown_sandbox_is_404_error_json() {
        // require_row gate: a well-formed slug that is not in the sandboxes
        // table is a 404 in the mgr error shape - never a dial attempt.
        let state = Arc::new(AppState::new_for_test());
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/sbx/ghost/api/manifest"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["error"], "sandbox \"ghost\" not found");
    }

    #[tokio::test]
    async fn invalid_name_slug_is_rejected_400() {
        // Anything validate_name rejects (uppercase, %-escapes, punctuation)
        // is a 400 BEFORE any upstream URL is built - the raw, still-encoded
        // segment is checked on purpose, so a path-injection attempt can
        // never become part of the upstream host.
        let state = Arc::new(AppState::new_for_test());
        insert_sandbox(&state, "alpha", false); // a valid row exists: proves rejection is by NAME
        let base = serve(&state).await;

        // (".." / dot-segments are omitted: the url crate normalizes them
        // client-side before the wire, so they never reach the router.)
        for bad in ["Alpha", "evil%20name", "evil%2Fname", "under_score"] {
            let r = state
                .http
                .get(format!("{base}/api/sbx/{bad}/api/manifest"))
                .send()
                .await
                .unwrap_or_else(|e| panic!("{bad}: {e}"));
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "name {bad:?}");
            let v: serde_json::Value = r.json().await.unwrap();
            let msg = v["error"].as_str().unwrap_or_default();
            assert!(msg.contains("invalid name"), "{bad:?}: {msg}");
        }
    }

    #[tokio::test]
    async fn bare_and_trailing_slash_forms_do_not_reach_proxy() {
        // Module-comment contract: ONLY the `*path` form routes to the proxy.
        // The bare form backtracks to the /api seam; the trailing-slash form
        // matches no route at all (partially-walked subtrees do not backtrack
        // to sibling catch-alls in matchit 0.7.3) and lands on the router's
        // default fallback - which in production is the SPA-hostile seam
        // guard (routes.rs unmatched_fallback), never the ServeDir SPA.
        // Locked because Phase 2's paneUrl builds proxy URLs and must know a
        // stray trailing slash is a 404 JSON, not an HTML page or a dial.
        let state = Arc::new(AppState::new_for_test());
        insert_sandbox(&state, "alpha", false);
        let base = serve(&state).await;

        // Bare form: the seam answers.
        let r = state
            .http
            .get(format!("{base}/api/sbx/alpha"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["error"], "no such API route");

        // Trailing-slash form: matches nothing; the default fallback answers
        // in the same JSON shape (routes.rs, verified in its own tests).
        let r = state
            .http
            .get(format!("{base}/api/sbx/alpha/"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["error"], "no such API route");

        // Deep trailing-slash DOES route (catch-all tail "x/"): the proxy
        // answers (registered + unreachable -> 502), proving the fallback
        // does not over-capture real proxy paths.
        let r = state
            .http
            .get(format!("{base}/api/sbx/alpha/x/"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
        let v: serde_json::Value = r.json().await.unwrap();
        assert!(v["error"].as_str().unwrap().contains("unreachable"));
    }

    #[tokio::test]
    async fn registered_but_unreachable_sandbox_is_502() {
        // No liveness filtering (design §1): a REGISTERED sandbox (native or
        // adopted - both carry the alias) passes the gate and the connect
        // failure surfaces as 502 in the mgr error shape. The test AppState
        // builds a no-proxy reqwest client (state.rs new_for_test), so the
        // alias cannot resolve anywhere a test runs - a dev sandbox's
        // proxy env would otherwise "answer" the bare alias on the proxy's
        // own terms and the passthrough would hide this branch.
        let state = Arc::new(AppState::new_for_test());
        insert_sandbox(&state, "alpha", false);
        insert_sandbox(&state, "legacy", true);
        let base = serve(&state).await;

        for name in ["alpha", "legacy"] {
            let r = state
                .http
                .get(format!("{base}/api/sbx/{name}/api/manifest"))
                .send()
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let status = r.status();
            let body_text = r.text().await.unwrap_or_default();
            assert_eq!(
                status,
                StatusCode::BAD_GATEWAY,
                "{name}: status {status} body {body_text}"
            );
            let v: serde_json::Value = serde_json::from_str(&body_text).unwrap();
            let msg = v["error"].as_str().unwrap_or_default();
            assert!(msg.contains("unreachable"), "{name}: {msg}");
            // The message names the derived upstream - the host came from the
            // sandbox name, not from anything the request carried.
            assert!(msg.contains(&format!("sbx-{name}-piweb")), "{name}: {msg}");
        }
    }
}
