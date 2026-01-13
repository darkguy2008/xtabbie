//! Window switcher creation and event handling.

use std::collections::HashSet;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

use crate::icons::{create_generic_icon, get_window_icon};
use crate::log;
use crate::ui::{draw_switcher, Layout, WindowInfo};
use crate::window::{activate_window, collect_windows_by_zorder, log_window_debug_info, should_show_in_switcher};

// X11 keycodes
const TAB_KEYCODE: u8 = 23;
const ALT_L_KEYCODE: u8 = 64;
const ALT_R_KEYCODE: u8 = 108;
const ESCAPE_KEYCODE: u8 = 9;
const RETURN_KEYCODE: u8 = 36;

// Layout constants
const ICON_SIZE: u16 = 48;
const PADDING: u16 = 8;
const TITLE_HEIGHT: u16 = 24;
const MAX_COLS: u16 = 20;

/// Resources for a switcher window.
struct SwitcherWindow {
    windows: Vec<WindowInfo>,
    win_id: Window,
    gc_id: Gcontext,
    gc_inv_id: Gcontext,
    layout: Layout,
}

/// Run the switcher in test mode (keyboard navigation, Enter to select).
pub fn run_test_mode(
    conn: &impl Connection,
    screen: &Screen,
    root: Window,
) -> Result<(), Box<dyn std::error::Error>> {
    log::clear();
    log_fmt!("=== Test mode started ===");

    let switcher = create_switcher_window(conn, screen, root)?;

    if switcher.windows.is_empty() {
        return Ok(());
    }

    let mut selected: usize = 0;

    loop {
        let event = conn.wait_for_event()?;
        match event {
            x11rb::protocol::Event::Expose(_) => {
                draw_switcher(
                    conn,
                    switcher.win_id,
                    switcher.gc_id,
                    switcher.gc_inv_id,
                    &switcher.windows,
                    selected,
                    &switcher.layout,
                )?;
            }
            x11rb::protocol::Event::KeyPress(ev) => match ev.detail {
                TAB_KEYCODE => {
                    selected = navigate_selection(selected, switcher.windows.len(), &ev);
                    draw_switcher(
                        conn,
                        switcher.win_id,
                        switcher.gc_id,
                        switcher.gc_inv_id,
                        &switcher.windows,
                        selected,
                        &switcher.layout,
                    )?;
                }
                RETURN_KEYCODE => {
                    activate_window(conn, switcher.windows[selected].wid, root)?;
                    break;
                }
                ESCAPE_KEYCODE => break,
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}

/// Run the switcher in daemon mode (global Alt+Tab hotkey).
pub fn run_daemon_mode(
    conn: &impl Connection,
    screen: &Screen,
    root: Window,
) -> Result<(), Box<dyn std::error::Error>> {
    // Grab Alt+Tab and Alt+Shift+Tab on root window
    let mod_mask = ModMask::M1; // Alt

    conn.grab_key(true, root, mod_mask, TAB_KEYCODE, GrabMode::ASYNC, GrabMode::ASYNC)?;
    conn.grab_key(
        true,
        root,
        mod_mask | ModMask::SHIFT,
        TAB_KEYCODE,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
    )?;
    conn.flush()?;

    // Main daemon loop
    loop {
        let event = conn.wait_for_event()?;

        if let x11rb::protocol::Event::KeyPress(ev) = event {
            if ev.detail == TAB_KEYCODE {
                let shift_held = (ev.state & KeyButMask::SHIFT).bits() != 0;
                show_switcher(conn, screen, root, shift_held)?;
            }
        }
    }
}

/// Show the switcher window and handle its event loop.
fn show_switcher(
    conn: &impl Connection,
    screen: &Screen,
    root: Window,
    shift_held: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    log::clear();
    log_fmt!("=== Switcher activated (shift={}) ===", shift_held);

    let switcher = create_switcher_window(conn, screen, root)?;

    if switcher.windows.is_empty() {
        conn.destroy_window(switcher.win_id)?;
        conn.flush()?;
        return Ok(());
    }

    // Start with second window selected (like traditional alt-tab), or last if shift
    let mut selected = initial_selection(switcher.windows.len(), shift_held);

    // Grab keyboard to get all key events while switcher is open
    conn.grab_keyboard(
        true,
        switcher.win_id,
        x11rb::CURRENT_TIME,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
    )?;
    conn.flush()?;

    let result = run_switcher_loop(conn, &switcher, root, &mut selected);

    // Cleanup
    conn.ungrab_keyboard(x11rb::CURRENT_TIME)?;
    conn.destroy_window(switcher.win_id)?;
    conn.flush()?;

    result
}

fn run_switcher_loop(
    conn: &impl Connection,
    switcher: &SwitcherWindow,
    root: Window,
    selected: &mut usize,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let event = conn.wait_for_event()?;
        match event {
            x11rb::protocol::Event::Expose(_) => {
                draw_switcher(
                    conn,
                    switcher.win_id,
                    switcher.gc_id,
                    switcher.gc_inv_id,
                    &switcher.windows,
                    *selected,
                    &switcher.layout,
                )?;
            }
            x11rb::protocol::Event::KeyPress(ev) => match ev.detail {
                TAB_KEYCODE => {
                    *selected = navigate_selection(*selected, switcher.windows.len(), &ev);
                    draw_switcher(
                        conn,
                        switcher.win_id,
                        switcher.gc_id,
                        switcher.gc_inv_id,
                        &switcher.windows,
                        *selected,
                        &switcher.layout,
                    )?;
                }
                ESCAPE_KEYCODE => return Ok(()),
                _ => {}
            },
            x11rb::protocol::Event::KeyRelease(ev) => {
                // Alt released - activate and close
                if ev.detail == ALT_L_KEYCODE || ev.detail == ALT_R_KEYCODE {
                    activate_window(conn, switcher.windows[*selected].wid, root)?;
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn initial_selection(window_count: usize, shift_held: bool) -> usize {
    if window_count > 1 {
        if shift_held {
            window_count - 1
        } else {
            1
        }
    } else {
        0
    }
}

fn navigate_selection(current: usize, count: usize, ev: &KeyPressEvent) -> usize {
    let shift_held = (ev.state & KeyButMask::SHIFT).bits() != 0;
    if shift_held {
        if current == 0 {
            count - 1
        } else {
            current - 1
        }
    } else {
        (current + 1) % count
    }
}

/// Create the switcher window with all discovered windows.
fn create_switcher_window(
    conn: &impl Connection,
    screen: &Screen,
    root: Window,
) -> Result<SwitcherWindow, Box<dyn std::error::Error>> {
    log_fmt!("Collecting windows...");

    // Gather windows in Z-order (MRU - most recently used first)
    let window_list = collect_windows_by_zorder(conn, root);
    let windows = deduplicate_windows(conn, window_list, root);

    // Calculate layout
    let layout = calculate_layout(screen, windows.len());

    // Create the window
    let (win_id, gc_id, gc_inv_id) = create_x11_window(conn, screen, root, &layout)?;

    Ok(SwitcherWindow {
        windows,
        win_id,
        gc_id,
        gc_inv_id,
        layout,
    })
}

fn deduplicate_windows(conn: &impl Connection, window_list: Vec<(Window, String)>, root: Window) -> Vec<WindowInfo> {
    let generic_icon = create_generic_icon(ICON_SIZE);
    let mut seen_titles = HashSet::new();
    let mut windows = Vec::new();

    log_fmt!("Found {} windows before filtering", window_list.len());

    for (wid, title) in window_list {
        log_window_debug_info(conn, wid, root);

        // Check EWMH filtering first
        let (should_show, reason) = should_show_in_switcher(conn, wid);
        if !should_show {
            log_fmt!("  -> FILTERED OUT ({})", reason);
            continue;
        }

        // Then check for duplicate titles
        if seen_titles.insert(title.clone()) {
            log_fmt!("  -> INCLUDED (unique title)");
            let icon = get_window_icon(conn, wid, ICON_SIZE)
                .unwrap_or_else(|| generic_icon.scale(ICON_SIZE));

            windows.push(WindowInfo { wid, title, icon });
        } else {
            log_fmt!("  -> SKIPPED (duplicate title)");
        }
    }

    log_fmt!("Final window count: {}", windows.len());
    windows
}

fn calculate_layout(screen: &Screen, window_count: usize) -> Layout {
    let max_width = (screen.width_in_pixels as f32 * 0.8) as u16;
    let max_cols_by_width = ((max_width - PADDING) / (ICON_SIZE + PADDING)).max(1);
    let cols = (window_count as u16).min(max_cols_by_width).min(MAX_COLS).max(1);
    let win_width = cols * (ICON_SIZE + PADDING) + PADDING;

    Layout {
        cols,
        icon_size: ICON_SIZE,
        padding: PADDING,
        win_width,
    }
}

fn create_x11_window(
    conn: &impl Connection,
    screen: &Screen,
    root: Window,
    layout: &Layout,
) -> Result<(Window, Gcontext, Gcontext), Box<dyn std::error::Error>> {
    let Layout { cols, icon_size, padding, win_width } = *layout;

    let rows = 1u16.max(cols); // Ensure at least one row
    let _ = rows; // Layout calculation happens in draw_switcher
    let win_height = icon_size + padding * 2 + TITLE_HEIGHT;

    let x = (screen.width_in_pixels.saturating_sub(win_width)) / 2;
    let y = (screen.height_in_pixels.saturating_sub(win_height)) / 2;

    let win_id = conn.generate_id()?;
    let gc_id = conn.generate_id()?;
    let gc_inv_id = conn.generate_id()?;

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win_id,
        root,
        x as i16,
        y as i16,
        win_width,
        win_height,
        2,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(screen.white_pixel)
            .border_pixel(screen.black_pixel)
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::KEY_RELEASE),
    )?;

    conn.create_gc(
        gc_id,
        win_id,
        &CreateGCAux::new()
            .foreground(screen.black_pixel)
            .background(screen.white_pixel),
    )?;

    conn.create_gc(
        gc_inv_id,
        win_id,
        &CreateGCAux::new()
            .foreground(screen.white_pixel)
            .background(screen.black_pixel),
    )?;

    conn.change_property8(
        PropMode::REPLACE,
        win_id,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"Switch",
    )?;

    conn.map_window(win_id)?;
    conn.flush()?;

    Ok((win_id, gc_id, gc_inv_id))
}
