use anyhow::{anyhow, Result};
use image::DynamicImage;
use screenshots::Screen;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct CachedScreen {
    screen: Screen,
    screen_x: i32,
    screen_y: i32,
    screen_w: i32,
    screen_h: i32,
    cached_at: Instant,
}

static CACHED_SCREEN: Mutex<Option<CachedScreen>> = Mutex::new(None);

/// Captures a region of the screen with persistent display handle caching.
/// Avoids calling expensive display enumeration APIs on every frame.
pub fn capture_region(x: i32, y: i32, w: u32, h: u32) -> Result<DynamicImage> {
    if w == 0 || h == 0 {
        return Err(anyhow!("Cannot capture region with 0 width or height"));
    }

    let center_x = x + (w as i32) / 2;
    let center_y = y + (h as i32) / 2;

    // Fast path: try cached screen if still valid (< 5 seconds) and covers the center
    {
        let cache = CACHED_SCREEN.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref c) = *cache {
            if c.cached_at.elapsed() < Duration::from_secs(5)
                && center_x >= c.screen_x
                && center_x < c.screen_x + c.screen_w
                && center_y >= c.screen_y
                && center_y < c.screen_y + c.screen_h
            {
                let local_x = (x - c.screen_x).max(0);
                let local_y = (y - c.screen_y).max(0);
                let max_avail_w = (c.screen_w - local_x).max(1) as u32;
                let max_avail_h = (c.screen_h - local_y).max(1) as u32;
                let capture_w = w.min(max_avail_w);
                let capture_h = h.min(max_avail_h);

                if let Ok(image) = c.screen.capture_area(local_x, local_y, capture_w, capture_h) {
                    return Ok(DynamicImage::ImageRgba8(image));
                }
            }
        }
    }

    // Slow path: enumerate screens and update cache
    let screens = Screen::all()?;
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

    // Cache the resolved screen
    {
        let mut cache = CACHED_SCREEN.lock().unwrap_or_else(|p| p.into_inner());
        *cache = Some(CachedScreen {
            screen: *screen,
            screen_x: screen.display_info.x,
            screen_y: screen.display_info.y,
            screen_w: screen.display_info.width as i32,
            screen_h: screen.display_info.height as i32,
            cached_at: Instant::now(),
        });
    }

    Ok(DynamicImage::ImageRgba8(image))
}
