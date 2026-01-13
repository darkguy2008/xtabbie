//! UI drawing functions for the window switcher.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::icons::BwIcon;

/// Information about a window displayed in the switcher.
pub struct WindowInfo {
    pub wid: Window,
    pub title: String,
    pub icon: BwIcon,
}

/// Layout constants for the switcher UI.
pub struct Layout {
    pub cols: u16,
    pub icon_size: u16,
    pub padding: u16,
    pub win_width: u16,
}

/// Draw a single icon cell, optionally with selection highlight.
pub fn draw_icon(
    conn: &impl Connection,
    win_id: Window,
    gc: Gcontext,
    gc_inv: Gcontext,
    x: i16,
    y: i16,
    cell_size: u16,
    icon: &BwIcon,
    selected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    const ICON_PADDING: i16 = 2;
    let icon_size = icon.width as i16;

    let icon_x = x + (cell_size as i16 - icon_size) / 2;
    let icon_y = y + (cell_size as i16 - icon_size) / 2;

    // Draw selection box if selected
    if selected {
        let box_rect = Rectangle {
            x: icon_x - ICON_PADDING,
            y: icon_y - ICON_PADDING,
            width: (icon_size + ICON_PADDING * 2) as u16,
            height: (icon_size + ICON_PADDING * 2) as u16,
        };
        conn.poly_fill_rectangle(win_id, gc, &[box_rect])?;
    }

    // Collect pixels by color for batch drawing
    let mut black_pixels = Vec::new();
    let mut white_pixels = Vec::new();

    for iy in 0..icon.height {
        for ix in 0..icon.width {
            let idx = (iy * icon.width + ix) as usize;
            let is_black = icon.data.get(idx).copied().unwrap_or(false);

            let rect = Rectangle {
                x: icon_x + ix as i16,
                y: icon_y + iy as i16,
                width: 1,
                height: 1,
            };

            if selected {
                // Invert colors when selected
                if is_black {
                    white_pixels.push(rect);
                }
            } else if is_black {
                black_pixels.push(rect);
            }
        }
    }

    if !black_pixels.is_empty() {
        conn.poly_fill_rectangle(win_id, gc, &black_pixels)?;
    }
    if !white_pixels.is_empty() {
        conn.poly_fill_rectangle(win_id, gc_inv, &white_pixels)?;
    }

    Ok(())
}

/// Draw the complete switcher UI with all windows and selection.
pub fn draw_switcher(
    conn: &impl Connection,
    win_id: Window,
    gc_id: Gcontext,
    gc_inv_id: Gcontext,
    windows: &[WindowInfo],
    selected: usize,
    layout: &Layout,
) -> Result<(), Box<dyn std::error::Error>> {
    let Layout { cols, icon_size, padding, .. } = *layout;

    // Draw each window icon
    for (i, winfo) in windows.iter().enumerate() {
        let col = (i as u16) % cols;
        let row = (i as u16) / cols;

        let cx = padding + col * (icon_size + padding);
        let cy = padding + row * (icon_size + padding);

        // Clear cell background
        let cell = Rectangle {
            x: cx as i16,
            y: cy as i16,
            width: icon_size,
            height: icon_size,
        };
        conn.poly_fill_rectangle(win_id, gc_inv_id, &[cell])?;

        // Draw icon
        draw_icon(
            conn,
            win_id,
            gc_id,
            gc_inv_id,
            cx as i16,
            cy as i16,
            icon_size,
            &winfo.icon,
            i == selected,
        )?;
    }

    // Draw title bar
    draw_title_bar(conn, win_id, gc_id, gc_inv_id, windows, selected, layout)?;

    conn.flush()?;
    Ok(())
}

fn draw_title_bar(
    conn: &impl Connection,
    win_id: Window,
    gc_id: Gcontext,
    gc_inv_id: Gcontext,
    windows: &[WindowInfo],
    selected: usize,
    layout: &Layout,
) -> Result<(), Box<dyn std::error::Error>> {
    const TITLE_HEIGHT: u16 = 24;
    let Layout { cols, icon_size, padding, win_width } = *layout;

    let rows = ((windows.len() as u16 + cols - 1) / cols).max(1);
    let icons_height = rows * (icon_size + padding) + padding;
    let title_y = icons_height as i16;

    // Clear title background
    let title_bg = Rectangle {
        x: 0,
        y: title_y,
        width: win_width,
        height: TITLE_HEIGHT,
    };
    conn.poly_fill_rectangle(win_id, gc_inv_id, &[title_bg])?;

    // Draw separator line
    conn.poly_line(
        CoordMode::ORIGIN,
        win_id,
        gc_id,
        &[
            Point { x: 0, y: title_y },
            Point { x: win_width as i16, y: title_y },
        ],
    )?;

    // Draw title text
    if selected < windows.len() {
        let title = &windows[selected].title;
        let display_title = truncate_title(title, win_width);

        let text_width = display_title.len() as i16 * 6;
        let text_x = ((win_width as i16) - text_width) / 2;
        let text_y = title_y + 16;

        conn.image_text8(win_id, gc_id, text_x.max(4), text_y, display_title.as_bytes())?;
    }

    Ok(())
}

fn truncate_title(title: &str, win_width: u16) -> String {
    let max_chars = (win_width / 7) as usize;
    if title.len() > max_chars {
        format!("{}...", &title[..max_chars.saturating_sub(3)])
    } else {
        title.to_string()
    }
}
