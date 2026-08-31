// GET /api/term/ws - terminal pty WebSocket bridge (Phase E).
//
// Spawns a pty (login shell, or a command under a login shell when `?cmd=` is
// given) and bridges it bidirectionally to the WebSocket:
//   client keystrokes (Text frames) -> pty stdin (writer)
//   pty stdout/stderr (reader)      -> client (Text frames)
//
// The pty runs as root in /root because the app process is
// root and the child inherits it (design §4, implement.md risky
// point). When the pty child exits (reader EOF) or the client closes, the
// session is torn down: the child is killed+reaped and the WS is closed.
//
// WS client contract (XtermPane.tsx):
//   - Text frames  -> pty stdin (keystrokes as UTF-8 strings).
//   - Binary frames -> control channel. The only control today is resize: a
//     5-byte payload [0x01, cols_le_u16, rows_le_u16] calls TIOCSWINSZ on the
//     pty so the shell/TUI (opencode) redraws at the xterm.js pane's size.
//     Splitting keystrokes (Text) from control (Binary) removes any ambiguity,
//     so a user typing JSON can never be misread as a control message.
//   Server -> client: pty stdout/stderr as Text frames (`String::from_utf8_lossy`
//   so XtermPane's `typeof ev.data === "string"` check always passes). On pty
//   exit the WS is closed cleanly.
//
// Routing: registered as an explicit `GET /api/term/ws` route in main.rs,
// ranked higher than the `/api/*rest` 502 seam catch-all (static segments win
// over the catch-all in matchit 0.7.3) - the same mechanism that lets
// `/api/manifest` win. Sibling paths like `/api/term/notaws` still fall through
// to the seam.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Query;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::io::Read;
use std::io::Write;
use tokio::sync::mpsc;

use portable_pty::PtySize;

use crate::pty::{spawn_pty, PtySession};

/// Read-buffer size for the pty->WS pump. 8 KiB is generous for terminal
/// output chunks without excessive per-alloc overhead.
const READ_BUF_SIZE: usize = 8192;
/// Capacity of the pty->WS output channel. A bounded channel provides
/// backpressure: if the client is slow, the reader thread blocks on send
/// instead of buffering unboundedly (MVP OOM guard). When the WS closes, the
/// receiver is dropped on return, unblocking the reader thread.
const PTY_CHANNEL_CAPACITY: usize = 128;

/// Query params for `/api/term/ws`. `cmd` is optional: absent/empty = default
/// login shell; non-empty = run that command under a login shell (e.g.
/// `opencode`).
#[derive(Deserialize)]
pub struct TermQuery {
    pub cmd: Option<String>,
}

/// WebSocket upgrade handler for `GET /api/term/ws`.
pub async fn terminal_ws(ws: WebSocketUpgrade, Query(query): Query<TermQuery>) -> Response {
    ws.on_upgrade(move |socket| run_pty_session(socket, query.cmd))
}

/// Run a pty session bridged to a WebSocket until either side closes.
async fn run_pty_session(socket: WebSocket, cmd: Option<String>) {
    let session = match spawn_pty(cmd) {
        Ok(s) => s,
        Err(e) => {
            // Tell the client why the pty didn't start, then close. Best-effort:
            // the socket is dropped on return regardless.
            tracing::error!("pty spawn failed: {e}");
            let (mut sink, _stream) = socket.split();
            let _ = sink
                .send(Message::Text(format!(
                    "\r\n\x1b[31mpty spawn failed: {e}\x1b[0m\r\n"
                )))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    let PtySession {
        reader,
        mut writer,
        mut child,
        master,
        // `master` is retained for the resize control path (Binary frames below).
    } = session;

    let (ws_sink, mut ws_stream) = socket.split();
    let (pty_tx, mut pty_rx) = mpsc::channel::<String>(PTY_CHANNEL_CAPACITY);

    // pty -> WS pump: a dedicated OS thread owns the blocking reader and pushes
    // each chunk (lossily decoded to UTF-8) through the bounded channel. The
    // thread exits on EOF (child exited) or when the channel receiver is dropped
    // (WS closed -> run_pty_session returns -> pty_rx drops). A dedicated thread
    // (not spawn_blocking) is correct here because the read blocks for the
    // entire session lifetime, not a short burst.
    let mut reader = reader;
    std::thread::spawn(move || {
        let mut buf = [0u8; READ_BUF_SIZE];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF: child exited.
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if pty_tx.blocking_send(chunk).is_err() {
                        break; // WS closed (receiver dropped) -> stop reading.
                    }
                }
                Err(e) => {
                    tracing::debug!("pty read error: {e}");
                    break;
                }
            }
        }
    });

    // Bidirectional pump. `pty_rx.recv()` and `ws_stream.next()` are both
    // cancel-safe, so `tokio::select!` is sound here. The loop ends when the
    // pty child exits (recv returns None) or the client closes/errored.
    let mut ws_sink = ws_sink;
    loop {
        tokio::select! {
            chunk = pty_rx.recv() => {
                match chunk {
                    Some(s) => {
                        if ws_sink.send(Message::Text(s)).await.is_err() {
                            break; // client gone
                        }
                    }
                    None => break, // pty reader EOF -> child exited
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(s))) => {
                        if let Err(e) = writer.write_all(s.as_bytes()) {
                            tracing::warn!("pty write failed: {e}");
                            break;
                        }
                        let _ = writer.flush();
                    }
                    Some(Ok(Message::Binary(b))) => {
                        // Control channel. Resize = 5 bytes [0x01, cols_le,
                        // rows_le]; apply via TIOCSWINSZ so the shell/TUI
                        // redraws at the xterm.js pane's size. Anything else
                        // (unknown control / non-browser client) is written
                        // through to the pty for robustness.
                        if b.len() == 5 && b[0] == 0x01 {
                            let cols = u16::from_le_bytes([b[1], b[2]]);
                            let rows = u16::from_le_bytes([b[3], b[4]]);
                            if let Err(e) = master.resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            }) {
                                tracing::warn!("pty resize failed: {e}");
                            }
                        } else {
                            let _ = writer.write_all(&b);
                            let _ = writer.flush();
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if ws_sink.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => { /* ignore */ }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(e)) => {
                        tracing::debug!("ws recv error: {e}");
                        break;
                    }
                    None => break, // stream ended
                }
            }
        }
    }

    // Teardown: kill the child (if still alive) and reap it so we don't leak a
    // process. `kill()` on an already-exited child is a no-op error (ignored).
    // `wait()` is a brief blocking call - after SIGKILL the child exits
    // immediately. Dropping `ws_sink`/`writer`/`pty_rx` on return closes the
    // remaining fds; dropping `pty_rx` unblocks the reader thread if it was
    // parked on `blocking_send`.
    let _ = child.kill();
    let _ = child.wait();
    let _ = ws_sink.close().await;
}
