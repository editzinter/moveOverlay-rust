use anyhow::{anyhow, Result};
use image::DynamicImage;
use screenshots::Screen;

/// Captures a region of the screen.
/// Uses the `screenshots` crate which works on all Windows setups including
/// remote desktop and headless sessions.
pub fn capture_region(x: i32, y: i32, w: u32, h: u32) -> Result<DynamicImage> {
    if w == 0 || h == 0 {
        return Err(anyhow!("Cannot capture region with 0 width or height"));
    }

    let screens = Screen::all()?;
    let center_x = x + (w as i32) / 2;
    let center_y = y + (h as i32) / 2;

    let screen = screens
        .iter()
        .find(|s| {
            let sx = s.display_info.x;
            let sy = s.display_info.y;
            let sw = s.display_info.width as i32;
            let sh = s.display_info.height as i32;
            center_x >= sx && center_x < sx + sw && center_y >= sy && center_y < sy + sh
        })
        .or_else(|| screens.first())
        .ok_or_else(|| anyhow!("No screen found"))?;

    let local_x = (x - screen.display_info.x).max(0);
    let local_y = (y - screen.display_info.y).max(0);

    let max_avail_w = (screen.display_info.width as i32 - local_x).max(1) as u32;
    let max_avail_h = (screen.display_info.height as i32 - local_y).max(1) as u32;

    let capture_w = w.min(max_avail_w);
    let capture_h = h.min(max_avail_h);

    let image = screen.capture_area(local_x, local_y, capture_w, capture_h)?;
    Ok(DynamicImage::ImageRgba8(image))
}
