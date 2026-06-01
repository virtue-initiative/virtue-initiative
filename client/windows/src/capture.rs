use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use virtue_core::{CoreError, CoreResult, PlatformHooks, Screenshot};

#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC, SRCCOPY,
    SelectObject,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::GetTickCount64;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[cfg(target_os = "windows")]
pub fn capture_screen_png() -> Result<Vec<u8>> {
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return Err(anyhow!("invalid screen size {}x{}", width, height));
        }

        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            return Err(anyhow!("GetDC failed"));
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            return Err(anyhow!("CreateCompatibleDC failed"));
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            return Err(anyhow!("CreateCompatibleBitmap failed"));
        }

        let old_obj = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        if old_obj.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            return Err(anyhow!("SelectObject failed"));
        }

        if BitBlt(mem_dc, 0, 0, width, height, Some(screen_dc), 0, 0, SRCCOPY).is_err() {
            let _ = SelectObject(mem_dc, old_obj);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            return Err(anyhow!("BitBlt failed"));
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let rows = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(mem_dc, old_obj);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);

        if rows == 0 {
            return Err(anyhow!("GetDIBits failed"));
        }

        let mut rgba = Vec::with_capacity(bgra.len());
        for px in bgra.chunks_exact(4) {
            rgba.push(px[2]);
            rgba.push(px[1]);
            rgba.push(px[0]);
            rgba.push(px[3]);
        }

        let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
            .context("failed to create image from framebuffer")?;

        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .context("failed to encode screenshot as png")?;

        Ok(encoded.into_inner())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_screen_png() -> Result<Vec<u8>> {
    Err(anyhow!("windows capture is only supported on Windows"))
}

#[cfg(target_os = "windows")]
pub fn read_last_startup_time_utc_ms() -> CoreResult<Option<i64>> {
    let uptime_ms = unsafe { GetTickCount64() };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| CoreError::CommandFailed(e.to_string()))?
        .as_millis();
    let startup_ms = now_ms.saturating_sub(uptime_ms as u128);
    Ok(Some(
        i64::try_from(startup_ms).map_err(|_| CoreError::InvalidState("clock overflow"))?,
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn read_last_startup_time_utc_ms() -> CoreResult<Option<i64>> {
    Ok(None)
}

// Reads HKLM\SYSTEM\CurrentControlSet\Control\Windows\ShutdownTime (REG_BINARY FILETIME).
#[cfg(target_os = "windows")]
pub fn read_last_shutdown_time_utc_ms() -> CoreResult<Option<i64>> {
    use windows::core::PCWSTR;

    const FILETIME_TO_UNIX_OFFSET: u64 = 116_444_736_000_000_000;

    let key_path: Vec<u16> = "SYSTEM\\CurrentControlSet\\Control\\Windows\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "ShutdownTime\0".encode_utf16().collect();

    unsafe {
        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        let open_result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(key_path.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        );
        if open_result.is_err() {
            return Ok(None);
        }

        let mut data = [0u8; 8];
        let mut data_size = 8u32;
        let query_result = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(value_name.as_ptr()),
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut data_size),
        );
        let _ = RegCloseKey(hkey);

        if query_result.is_err() || data_size != 8 {
            return Ok(None);
        }

        let filetime = u64::from_le_bytes(data);
        if filetime < FILETIME_TO_UNIX_OFFSET {
            return Ok(None);
        }
        let unix_ms = (filetime - FILETIME_TO_UNIX_OFFSET) / 10_000;
        Ok(Some(i64::try_from(unix_ms).map_err(|_| {
            CoreError::InvalidState("shutdown time overflow")
        })?))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_last_shutdown_time_utc_ms() -> CoreResult<Option<i64>> {
    Ok(None)
}

#[derive(Clone)]
pub struct WindowsPlatformHooks;

impl WindowsPlatformHooks {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsPlatformHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformHooks for WindowsPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes =
            capture_screen_png().map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes,
            content_type: "image/png".to_string(),
        })
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        read_last_shutdown_time_utc_ms()
    }

    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        read_last_startup_time_utc_ms()
    }
}
