use std::{
    io::{self, Error, ErrorKind, Write},
    time::Duration,
};

use crate::{
    event::{filter::CursorPositionFilter, poll_internal, read_internal, InternalEvent},
    terminal::{disable_raw_mode, enable_raw_mode, sys::is_raw_mode_enabled},
};

/// Returns the cursor position (column, row).
///
/// The top left cell is represented as `(0, 0)`.
///
/// On unix systems, this function will block and possibly time out while
/// [`crossterm::event::read`](crate::event::read) or [`crossterm::event::poll`](crate::event::poll) are being called.
pub fn position() -> io::Result<(u16, u16)> {
    if std::env::var("CROSSTERM_SKIP_CURSOR_QUERY").ok().as_deref() == Some("1") {
        return Ok((0, 0));
    }
    if is_raw_mode_enabled() {
        read_position_raw()
    } else {
        read_position()
    }
}

fn read_position() -> io::Result<(u16, u16)> {
    enable_raw_mode()?;
    let pos = read_position_raw();
    disable_raw_mode()?;
    pos
}

fn read_position_raw() -> io::Result<(u16, u16)> {
    let timeout_ms: u64 = std::env::var("CROSSTERM_CURSOR_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);

    let mut stdout = io::stdout();
    stdout.write_all(b"\x1B[6n")?;
    stdout.flush()?;

    loop {
        match poll_internal(Some(Duration::from_millis(timeout_ms)), &CursorPositionFilter) {
            Ok(true) => {
                if let Ok(InternalEvent::CursorPosition(x, y)) =
                    read_internal(&CursorPositionFilter)
                {
                    return Ok((x, y));
                }
            }
            Ok(false) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "The cursor position could not be read within a normal duration",
                ));
            }
            Err(_) => {}
        }
    }
}
