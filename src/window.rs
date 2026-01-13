//! Window discovery, activation, and Z-order functions for X11.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::log;

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

/// Check if a window has WM_STATE property (indicates it's managed by the WM).
pub fn has_wm_state(conn: &impl Connection, window: Window) -> bool {
    let wm_state = match conn.intern_atom(false, b"WM_STATE").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return false,
    };

    conn.get_property(false, window, wm_state, wm_state, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|prop| !prop.value.is_empty())
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

    log_fmt!("Activating window 0x{:x}, toplevel=0x{:x}", window, toplevel);

    // Raise and map both the toplevel and the actual window
    let _ = conn.configure_window(toplevel, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = conn.map_window(toplevel);
    let _ = conn.map_window(window);
    conn.flush()?;

    log_fmt!("  Raised and mapped, sending WM_TAKE_FOCUS");

    // Send WM_TAKE_FOCUS if supported
    send_take_focus(conn, window);

    // Set input focus
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, window, x11rb::CURRENT_TIME);
    conn.flush()?;

    log_fmt!("  Focus set");

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

/// Get WM_CLASS property (instance name and class name).
pub fn get_wm_class(conn: &impl Connection, window: Window) -> Option<(String, String)> {
    let prop = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    if prop.value.is_empty() {
        return None;
    }

    // WM_CLASS is two null-terminated strings: instance\0class\0
    let parts: Vec<&[u8]> = prop.value.split(|&b| b == 0).collect();
    let instance = parts.first().and_then(|s| String::from_utf8(s.to_vec()).ok()).unwrap_or_default();
    let class = parts.get(1).and_then(|s| String::from_utf8(s.to_vec()).ok()).unwrap_or_default();

    Some((instance, class))
}

/// Get _NET_WM_WINDOW_TYPE property.
pub fn get_window_type(conn: &impl Connection, window: Window) -> Vec<String> {
    let atom = match conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return vec![],
    };

    let prop = match conn.get_property(false, window, atom, AtomEnum::ATOM, 0, 32).ok().and_then(|c| c.reply().ok()) {
        Some(p) => p,
        None => return vec![],
    };

    if prop.value.is_empty() {
        return vec![];
    }

    // Parse as array of atoms
    let atoms: Vec<Atom> = prop.value32().map(|iter| iter.collect()).unwrap_or_default();
    atoms.iter().filter_map(|&a| atom_name(conn, a)).collect()
}

/// Get _NET_WM_STATE property.
pub fn get_window_state(conn: &impl Connection, window: Window) -> Vec<String> {
    let atom = match conn.intern_atom(false, b"_NET_WM_STATE").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return vec![],
    };

    let prop = match conn.get_property(false, window, atom, AtomEnum::ATOM, 0, 32).ok().and_then(|c| c.reply().ok()) {
        Some(p) => p,
        None => return vec![],
    };

    if prop.value.is_empty() {
        return vec![];
    }

    let atoms: Vec<Atom> = prop.value32().map(|iter| iter.collect()).unwrap_or_default();
    atoms.iter().filter_map(|&a| atom_name(conn, a)).collect()
}

/// Get the name of an atom.
fn atom_name(conn: &impl Connection, atom: Atom) -> Option<String> {
    let reply = conn.get_atom_name(atom).ok()?.reply().ok()?;
    String::from_utf8(reply.name).ok()
}

/// Check if a window should be shown in the switcher.
/// Only shows windows that have WM_STATE (managed by the window manager).
/// Returns (should_show, reason) tuple for logging purposes.
pub fn should_show_in_switcher(conn: &impl Connection, window: Window) -> (bool, &'static str) {
    if has_wm_state(conn, window) {
        (true, "has WM_STATE")
    } else {
        (false, "no WM_STATE (not managed by WM)")
    }
}

/// Log detailed debug info about a window.
pub fn log_window_debug_info(conn: &impl Connection, window: Window, root: Window) {
    if !log::is_enabled() {
        return;
    }

    let title = get_window_title(conn, window).unwrap_or_else(|| "(no title)".into());
    let class = get_wm_class(conn, window)
        .map(|(i, c)| format!("{} / {}", i, c))
        .unwrap_or_else(|| "(no class)".into());
    let types = get_window_type(conn, window);
    let states = get_window_state(conn, window);
    let viewable = is_viewable(conn, window);
    let wm_state = has_wm_state(conn, window);
    let toplevel = find_toplevel_parent(conn, window, root);
    let (should_show, reason) = should_show_in_switcher(conn, window);

    log_fmt!("Window 0x{:x}:", window);
    log_fmt!("  Title: {}", title);
    log_fmt!("  Class: {}", class);
    log_fmt!("  Type: {:?}", types);
    log_fmt!("  State: {:?}", states);
    log_fmt!("  Viewable: {}, WM_STATE: {}", viewable, wm_state);
    log_fmt!("  TopLevel: 0x{:x}", toplevel);
    log_fmt!("  ShouldShow: {} ({})", should_show, reason);
}
