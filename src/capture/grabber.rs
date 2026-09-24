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

fn contains_region(
    screen_x: i32,
    screen_y: i32,
    screen_w: i32,
    screen_h: i32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> bool {
    let (sx, sy, sw, sh) = (
        screen_x as i64,
        screen_y as i64,
        screen_w as i64,
        screen_h as i64,
    );
    let (x, y, w, h) = (x as i64, y as i64, w as i64, h as i64);
    x >= sx && y >= sy && x + w <= sx + sw && y + h <= sy + sh
}

/// Captures a region of the screen with persistent display handle caching.
/// Avoids calling expensive display enumeration APIs on every frame.
pub fn capture_region(x: i32, y: i32, w: u32, h: u32) -> Result<DynamicImage> {
    if w == 0 || h == 0 {
        return Err(anyhow!("Cannot capture region with 0 width or height"));
    }

    // Fast path: try cached screen if it still covers the entire selected board.
    {
        let cache = CACHED_SCREEN.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref c) = *cache {
            if c.cached_at.elapsed() < Duration::from_secs(5)
                && contains_region(c.screen_x, c.screen_y, c.screen_w, c.screen_h, x, y, w, h)
            {
                let local_x = x - c.screen_x;
                let local_y = y - c.screen_y;

                if let Ok(image) = c.screen.capture_area(local_x, local_y, w, h) {
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
            contains_region(
                s.display_info.x,
                s.display_info.y,
                s.display_info.width as i32,
                s.display_info.height as i32,
                x,
                y,
                w,
                h,
            )
        })
        .ok_or_else(|| anyhow!("Selected board is outside one display; reselect the full board"))?;

    let local_x = x - screen.display_info.x;
    let local_y = y - screen.display_info.y;

    let image = screen.capture_area(local_x, local_y, w, h)?;

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

#[cfg(test)]
mod tests {
    use super::contains_region;

    #[test]
    fn capture_requires_the_entire_board_on_one_display() {
        assert!(contains_region(-1920, 0, 1920, 1080, -1800, 100, 800, 800));
        assert!(!contains_region(-1920, 0, 1920, 1080, -100, 100, 800, 800));
        assert!(!contains_region(0, 0, 1920, 1080, 1800, 100, 800, 800));
        assert!(!contains_region(0, 0, 1920, 1080, 0, 0, u32::MAX, 800));
    }
}
