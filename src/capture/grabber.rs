use anyhow::Result;
use image::DynamicImage;
use screenshots::Screen;

pub fn capture_region(x: u32, y: u32, w: u32, h: u32) -> Result<DynamicImage> {
    let screens = Screen::all()?;
    let screen = screens
        .first()
        .ok_or_else(|| anyhow::anyhow!("No screen found"))?;
    let image = screen.capture_area(x as i32, y as i32, w, h)?;
    Ok(DynamicImage::ImageRgba8(image))
}
