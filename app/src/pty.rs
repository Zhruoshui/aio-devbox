// Pty bridge: spawn a login shell (or a command under a login shell) in a
// portable-pty, returning the reader/writer/child handle.
//
// The app process already runs as uid 1000 (`gem`); the spawned shell inherits
// that uid (design §4, implement.md Phase E risky point). We do NOT drop/gain
// privileges - we just spawn. cwd = /home/gem, HOME=/home/gem, TERM=xterm-256color;
// the rest of the environment (PATH, etc.) is inherited so /usr/local/bin
// (where opencode lives) is on PATH for `?cmd=opencode`.
//
// Empty / absent cmd  -> `/bin/bash -l`  (interactive login shell; interactive
//                                       because the pty is a tty).
// Non-empty cmd       -> `/bin/bash -l -c <cmd>` (login shell so profile/PATH
//                                       are sourced; runs the command, exits
//                                       when it exits).

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io;

/// Initial pty size (cols x rows). The frontend (XtermPane.tsx) does not yet
/// send resize messages over the WS - it only forwards keystrokes as text - so
/// the pty stays at this size for the session. A future enhancement could add a
/// Initial pty size (cols x rows). The frontend (XtermPane.tsx) sends the real
/// terminal size as a resize message on WebSocket open (and on every fit), so
/// the pty is resized to match the xterm.js pane shortly after spawn. This is
/// just the size until that first resize arrives (and a fallback if the client
/// never sends one).
const PTY_COLS: u16 = 80;
const PTY_ROWS: u16 = 24;

/// A live pty session: the master reader/writer plus the child handle.
///
/// The original master is kept alive (`master`) so the pty fd remains valid for
/// the duration of the session; `try_clone_reader()` / `take_writer()` return
/// dup'd fds that are independently usable. `master` is also the handle for
/// resizing the pty (TIOCSWINSZ) when the frontend reports a new terminal size.
/// Dropping the `PtySession` tears everything down.
pub struct PtySession {
    pub reader: Box<dyn io::Read + Send>,
    pub writer: Box<dyn io::Write + Send>,
    pub child: Box<dyn Child + Send>,
    /// Kept alive so the master pty fd is not closed prematurely, AND used to
    /// resize the pty when the frontend reports a new cols/rows. The reader and
    /// writer hold dup'd fds, but we retain the master for both purposes.
    pub master: Box<dyn MasterPty + Send>,
}

/// Spawn a pty running either a login shell (no cmd / empty cmd) or a command
/// under a login shell (non-empty cmd).
///
/// Returns `io::Result` so callers can treat spawn failures uniformly.
pub fn spawn_pty(cmd: Option<String>) -> io::Result<PtySession> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(pty_err)?;

    // A login shell (`-l`) sources /etc/profile and ~/.profile so PATH and
    // other env are set up as for an interactive login. The pty is itself a
    // tty, so the shell is interactive even without `-i`.
    let mut builder = CommandBuilder::new("/bin/bash");
    match cmd.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            builder.arg("-l");
        }
        Some(c) => {
            builder.arg("-l");
            builder.arg("-c");
            builder.arg(c);
        }
    }
    builder.cwd("/home/gem");
    builder.env("HOME", "/home/gem");
    builder.env("TERM", "xterm-256color");
    // The rest of the environment (PATH, etc.) is inherited from the app
    // process by CommandBuilder's default behavior.

    let child = pair.slave.spawn_command(builder).map_err(pty_err)?;

    // Take the reader/writer from the master (dup'd fds). Dropping the slave
    // after spawn is critical: it ensures that when the child exits (closing
    // its slave fds), read() on the master returns EOF instead of hanging -
    // we'd otherwise hold a slave fd open ourselves.
    let reader = pair.master.try_clone_reader().map_err(pty_err)?;
    let writer = pair.master.take_writer().map_err(pty_err)?;
    let master = pair.master;
    drop(pair.slave);

    Ok(PtySession {
        reader,
        writer,
        child,
        master,
    })
}

/// Convert a portable-pty error into an `io::Error`. portable-pty 0.8 returns
/// `anyhow::Error` (which is private when re-exported as `portable_pty::Error`),
/// so we take any `Display` error and string-wrap it - no direct dependency on
/// the concrete error type.
fn pty_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}
