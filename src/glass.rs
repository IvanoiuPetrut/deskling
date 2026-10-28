//! The "glass" look: a blurred copy of the desktop behind the widget, drawn as its background.
//!
//! Windows' own blur-behind effects don't work on GPUI's windows, so the widget takes a picture
//! of the desktop (wallpaper + icons), cuts out the part under itself and blurs it here.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// A picture of the desktop in screen pixels: 4 bytes per pixel (blue, green, red, alpha), row by row.
pub struct Snapshot {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

/// A screen rectangle in pixels.
#[derive(Clone, Copy)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The part of `snapshot` under `rect`, blurred by about `radius` pixels.
pub fn blurred_region(snapshot: &Snapshot, rect: ScreenRect, radius: u32) -> Option<Arc<RenderImage>> {
    if rect.width <= 0 || rect.height <= 0 || snapshot.width <= 0 || snapshot.height <= 0 {
        return None;
    }

    // Three box blurs in a row look almost like a real (gaussian) blur, and are much cheaper.
    let box_radius = (radius / 2).max(1) as usize;
    // Cut out a bit more than needed, so the edges blur with their real neighbours.
    let margin = (box_radius * 3) as i32;
    let width = (rect.width + 2 * margin) as usize;
    let height = (rect.height + 2 * margin) as usize;

    let mut pixels = vec![0u8; width * height * 4];
    for row in 0..height {
        // Outside the snapshot (widget partly off screen), repeat the nearest edge pixel.
        let sy = (rect.y - margin + row as i32 - snapshot.top).clamp(0, snapshot.height - 1);
        for col in 0..width {
            let sx = (rect.x - margin + col as i32 - snapshot.left).clamp(0, snapshot.width - 1);
            let src = ((sy * snapshot.width + sx) * 4) as usize;
            let dst = (row * width + col) * 4;
            pixels[dst..dst + 3].copy_from_slice(&snapshot.bgra[src..src + 3]);
            pixels[dst + 3] = 255;
        }
    }

    let mut temp = pixels.clone();
    for _ in 0..3 {
        // Horizontal pass (along each row), then vertical pass (along each column).
        blur_lines(&pixels, &mut temp, height, width, 4, width * 4, box_radius);
        blur_lines(&temp, &mut pixels, width, height, width * 4, 4, box_radius);
    }

    // Keep only the part under the widget.
    let (out_width, out_height) = (rect.width as usize, rect.height as usize);
    let mut out = Vec::with_capacity(out_width * out_height * 4);
    for row in 0..out_height {
        let start = ((row + margin as usize) * width + margin as usize) * 4;
        out.extend_from_slice(&pixels[start..start + out_width * 4]);
    }

    // GPUI wants BGRA bytes; `RgbaImage` is only the container type here.
    let image = RgbaImage::from_raw(out_width as u32, out_height as u32, out)?;
    Some(Arc::new(RenderImage::new([Frame::new(image)])))
}

/// One 1-D box blur over every line: each pixel becomes the average of the `2 * radius + 1`
/// pixels around it. `step` is the byte distance between neighbours on a line, `line_step`
/// the distance between the starts of two lines. Alpha is left at 255.
fn blur_lines(
    src: &[u8],
    dst: &mut [u8],
    lines: usize,
    len: usize,
    step: usize,
    line_step: usize,
    radius: usize,
) {
    let window = (2 * radius + 1) as u32;
    let last = len as isize - 1;
    for line in 0..lines {
        let base = line * line_step;
        let at = |i: isize| base + i.clamp(0, last) as usize * step;
        for channel in 0..3 {
            // Running sum: add the pixel entering the window, remove the one leaving it.
            let mut sum: u32 = (-(radius as isize)..=radius as isize)
                .map(|i| src[at(i) + channel] as u32)
                .sum();
            for i in 0..len as isize {
                dst[at(i) + channel] = (sum / window) as u8;
                sum += src[at(i + radius as isize + 1) + channel] as u32;
                sum -= src[at(i - radius as isize) + channel] as u32;
            }
        }
        for i in 0..len as isize {
            dst[at(i) + 3] = 255;
        }
    }
}
