//! xtabbie - A simple X11 alt-tab window switcher.

use x11rb::connection::Connection;

mod icons;
mod switcher;
mod ui;
mod window;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let test_mode = std::env::args().any(|arg| arg == "--test");

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    if test_mode {
        switcher::run_test_mode(&conn, screen, root)
    } else {
        switcher::run_daemon_mode(&conn, screen, root)
    }
}
