pub const DEFAULT_TRAY_ICON_SIZE: u32 = 16;

/// Builds the default tray icon used across desktop clients.
/// Returns RGBA pixels in row-major order.
pub fn build_default_tray_icon_rgba() -> (u32, u32, Vec<u8>) {
    let width = DEFAULT_TRAY_ICON_SIZE;
    let height = DEFAULT_TRAY_ICON_SIZE;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    // Keep this aligned with the historical Linux tray icon: a green filled circle.
    let center = (DEFAULT_TRAY_ICON_SIZE / 2) as i32;
    let radius = 6_i32;
    let radius_sq = radius * radius;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let idx = ((y as u32 * width + x as u32) * 4) as usize;
            let dx = x - center;
            let dy = y - center;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq <= radius_sq {
                rgba[idx] = 34;
                rgba[idx + 1] = 197;
                rgba[idx + 2] = 94;
                rgba[idx + 3] = 255;
            }
        }
    }

    (width, height, rgba)
}
