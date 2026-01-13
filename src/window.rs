//! Window discovery, activation, and Z-order functions for X11.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

/// Get the title of an X11 window, trying _NET_WM_NAME first, then WM_NAME.
pub fn get_window_title(conn: &impl Connection, window: Window) -> Option<String> {
    // Try _NET_WM_NAME (UTF-8) first
    if let Some(title) = get_net_wm_name(conn, window) {
        return Some(title);
    }

    // Fall back to WM_NAME
    get_wm_name(conn, window)
}

fn get_net_wm_name(conn: &impl Connection, window: Window) -> Option<String> {
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME").ok()?.reply().ok()?.atom;
    let utf8 = conn.intern_atom(false, b"UTF8_STRING").ok()?.reply().ok()?.atom;

    let prop = conn
        .get_property(false, window, net_wm_name, utf8, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    if prop.value.is_empty() {
        return None;
    }

    let s = String::from_utf8(prop.value).ok()?;
    if s.trim().is_empty() {
        return None;
    }

    Some(s)
}

fn get_wm_name(conn: &impl Connection, window: Window) -> Option<String> {
    let prop = conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::ANY, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    if prop.value.is_empty() {
        return None;
    }

    // Try UTF-8 first
    if let Ok(s) = String::from_utf8(prop.value.clone()) {
        if !s.trim().is_empty() {
            return Some(s);
        }
    }

    // Fall back to Latin-1 interpretation
    let s: String = prop.value.iter().map(|&b| b as char).collect();
    if s.trim().is_empty() {
        return None;
    }

    Some(s)
}

/// Check if a window is in viewable (mapped) state.
pub fn is_viewable(conn: &impl Connection, window: Window) -> bool {
    conn.get_window_attributes(window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|attrs| attrs.map_state == MapState::VIEWABLE)
        .unwrap_or(false)
}

/// Collect windows in Z-order (most recently used first).
/// X11 query_tree returns children in bottom-to-top stacking order,
/// so we reverse to get top-to-bottom (MRU order).
pub fn collect_windows_by_zorder(conn: &impl Connection, root: Window) -> Vec<(Window, String)> {
    let tree = match conn.query_tree(root).ok().and_then(|c| c.reply().ok()) {
        Some(t) => t,
        None => return Vec::new(),
    };

    // Children are in bottom-to-top order, reverse for MRU
    tree.children
        .iter()
        .rev()
        .filter_map(|&child| find_window_with_title(conn, child, 0))
        .collect()
}

/// Find a window with a title, searching down the tree.
/// Returns the window ID that has the title (might be a child).
fn find_window_with_title(
    conn: &impl Connection,
    window: Window,
    depth: u32,
) -> Option<(Window, String)> {
    const MAX_DEPTH: u32 = 10;

    if depth > MAX_DEPTH {
        return None;
    }

    // Check if this window is viewable and has a title
    if is_viewable(conn, window) {
        if let Some(title) = get_window_title(conn, window) {
            return Some((window, title));
        }
    }

    // Search children
    let tree = conn.query_tree(window).ok()?.reply().ok()?;
    for child in tree.children {
        if let Some(result) = find_window_with_title(conn, child, depth + 1) {
            return Some(result);
        }
    }

    None
}

/// Find the top-level parent of a window (direct child of root).
pub fn find_toplevel_parent(conn: &impl Connection, window: Window, root: Window) -> Window {
    const MAX_DEPTH: u32 = 20;
    let mut current = window;

    for _ in 0..MAX_DEPTH {
        let tree = match conn.query_tree(current).ok().and_then(|c| c.reply().ok()) {
            Some(t) => t,
            None => return window,
        };

        if tree.parent == root || tree.parent == 0 {
            return current;
        }

        current = tree.parent;
    }

    window
}

/// Activate a window by raising it and setting input focus.
pub fn activate_window(
    conn: &impl Connection,
    window: Window,
    root: Window,
) -> Result<(), Box<dyn std::error::Error>> {
    let toplevel = find_toplevel_parent(conn, window, root);

    // Raise and map both the toplevel and the actual window
    let _ = conn.configure_window(toplevel, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = conn.map_window(toplevel);
    let _ = conn.map_window(window);
    conn.flush()?;

    // Send WM_TAKE_FOCUS if supported
    send_take_focus(conn, window);

    // Set input focus
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, window, x11rb::CURRENT_TIME);
    conn.flush()?;

    Ok(())
}

fn send_take_focus(conn: &impl Connection, window: Window) {
    let wm_protocols = match conn.intern_atom(false, b"WM_PROTOCOLS").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return,
    };

    let wm_take_focus = match conn.intern_atom(false, b"WM_TAKE_FOCUS").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return,
    };

    let event = ClientMessageEvent::new(
        32,
        window,
        wm_protocols,
        [wm_take_focus, x11rb::CURRENT_TIME, 0, 0, 0],
    );

    let _ = conn.send_event(false, window, EventMask::NO_EVENT, event);
}
