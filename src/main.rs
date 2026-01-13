//! xtabbie - A simple X11 alt-tab window switcher.

use x11rb::connection::Connection;

mod icons;
#[macro_use]
mod log;
mod switcher;
mod ui;
mod window;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let test_mode = args.iter().any(|arg| arg == "--test");
    let log_mode = args.iter().any(|arg| arg == "--log");

    if log_mode {
        log::enable();
    }

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    log_fmt!("xtabbie started, test_mode={}, screen={}", test_mode, screen_num);

    if test_mode {
        switcher::run_test_mode(&conn, screen, root)
    } else {
        switcher::run_daemon_mode(&conn, screen, root)
    }
}
