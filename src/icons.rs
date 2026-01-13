//! Icon handling for window switcher - fetching, converting, and rendering.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

/// Represents a 1-bit black and white icon.
pub struct BwIcon {
    pub width: u16,
    pub height: u16,
    /// Pixel data: true = black, false = white.
    pub data: Vec<bool>,
}

impl BwIcon {
    /// Scale the icon to target size using nearest neighbor.
    pub fn scale(&self, target_size: u16) -> BwIcon {
        let mut scaled = Vec::with_capacity((target_size * target_size) as usize);

        for y in 0..target_size {
            for x in 0..target_size {
                let src_x = (x as u32 * self.width as u32 / target_size as u32) as usize;
                let src_y = (y as u32 * self.height as u32 / target_size as u32) as usize;
                let idx = src_y * self.width as usize + src_x;
                scaled.push(self.data.get(idx).copied().unwrap_or(false));
            }
        }

        BwIcon {
            width: target_size,
            height: target_size,
            data: scaled,
        }
    }
}

/// Fetch _NET_WM_ICON and convert to B&W with hard threshold.
pub fn get_window_icon(conn: &impl Connection, window: Window, target_size: u16) -> Option<BwIcon> {
    let net_wm_icon = conn.intern_atom(false, b"_NET_WM_ICON").ok()?.reply().ok()?.atom;

    let reply = conn
        .get_property(false, window, net_wm_icon, AtomEnum::CARDINAL, 0, 65536)
        .ok()?
        .reply()
        .ok()?;

    if reply.value.is_empty() {
        return None;
    }

    // Parse icon data - format is: width, height, ARGB pixels...
    let data = cast_bytes_to_u32(&reply.value);
    if data.len() < 2 {
        return None;
    }

    let (width, height, pixels) = find_best_icon(data, target_size)?;
    let bw = argb_to_bw(pixels);

    let icon = BwIcon {
        width: width as u16,
        height: height as u16,
        data: bw,
    };

    Some(icon.scale(target_size))
}

/// Find the icon closest to target size from the icon data.
fn find_best_icon(data: &[u32], target_size: u16) -> Option<(u32, u32, &[u32])> {
    let mut best: Option<(u32, u32, &[u32])> = None;
    let mut idx = 0;

    while idx + 2 < data.len() {
        let width = data[idx];
        let height = data[idx + 1];
        let pixel_count = (width * height) as usize;

        if width == 0 || height == 0 || idx + 2 + pixel_count > data.len() {
            break;
        }

        let pixels = &data[idx + 2..idx + 2 + pixel_count];

        if should_replace_best(best, width, height, target_size) {
            best = Some((width, height, pixels));
        }

        idx += 2 + pixel_count;
    }

    best
}

fn should_replace_best(
    best: Option<(u32, u32, &[u32])>,
    width: u32,
    height: u32,
    target_size: u16,
) -> bool {
    let Some((bw, bh, _)) = best else {
        return true;
    };

    let target = target_size as i32;
    let best_diff = (bw as i32 - target).abs() + (bh as i32 - target).abs();
    let this_diff = (width as i32 - target).abs() + (height as i32 - target).abs();

    this_diff < best_diff || (this_diff == best_diff && width >= target_size as u32)
}

fn cast_bytes_to_u32(bytes: &[u8]) -> &[u32] {
    if bytes.is_empty() {
        return &[];
    }
    // Safety: This is safe because:
    // 1. We're reading from a valid byte slice
    // 2. X11 icon data is always u32-aligned
    // 3. We divide length by 4 to get correct count
    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u32, bytes.len() / 4) }
}

/// Convert ARGB pixels to B&W using luminance threshold.
fn argb_to_bw(pixels: &[u32]) -> Vec<bool> {
    pixels
        .iter()
        .map(|&argb| {
            let a = ((argb >> 24) & 0xFF) as f32 / 255.0;
            let r = ((argb >> 16) & 0xFF) as f32;
            let g = ((argb >> 8) & 0xFF) as f32;
            let b = (argb & 0xFF) as f32;

            // Luminance formula (ITU-R BT.601)
            let lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;
            // Blend with white background based on alpha
            let value = lum * a + (1.0 - a);

            // Hard threshold - true = black
            value < 0.5
        })
        .collect()
}

/// Create a generic window icon (fallback when no icon available).
pub fn create_generic_icon(size: u16) -> BwIcon {
    let s = size as usize;
    let titlebar_h = s / 5;
    let border = 2;

    let mut data = vec![false; s * s];

    for y in 0..s {
        for x in 0..s {
            let idx = y * s + x;
            let is_border = x < border || x >= s - border || y < border || y >= s - border;
            let is_titlebar = y < titlebar_h + border;
            let is_inner_border = x == border || x == s - border - 1 || y == s - border - 1;

            data[idx] = is_border || is_titlebar || is_inner_border;
        }
    }

    BwIcon {
        width: size,
        height: size,
        data,
    }
}
