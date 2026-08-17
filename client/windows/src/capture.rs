use std::io::Cursor;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use virtue_core::{CoreError, CoreResult, LifecycleHooks, Screenshot, ScreenshotHooks};

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

// Reads the logon time of the current user session via the process token and LSA.
// Uses logon time rather than system uptime because Virtue starts at user login, not system boot.
#[cfg(target_os = "windows")]
pub fn read_last_login_utc_ms() -> CoreResult<Option<i64>> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::Authentication::Identity::{
        LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const FILETIME_TO_UNIX_OFFSET_100NS: i64 = 116_444_736_000_000_000;

    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;

        let mut stats = TOKEN_STATISTICS::default();
        let mut returned = 0u32;
        let token_result = GetTokenInformation(
            token,
            TokenStatistics,
            Some(&mut stats as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_STATISTICS>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        token_result.map_err(|e| CoreError::CommandFailed(e.to_string()))?;

        let mut session_data: *mut SECURITY_LOGON_SESSION_DATA = std::ptr::null_mut();
        LsaGetLogonSessionData(&stats.AuthenticationId, &mut session_data)
            .ok()
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;

        if session_data.is_null() {
            return Ok(None);
        }

        let logon_time = (*session_data).LogonTime;
        let _ = LsaFreeReturnBuffer(session_data as *mut core::ffi::c_void);

        if logon_time <= 0 || logon_time < FILETIME_TO_UNIX_OFFSET_100NS {
            return Ok(None);
        }
        let unix_ms = (logon_time - FILETIME_TO_UNIX_OFFSET_100NS) / 10_000;
        Ok(Some(unix_ms))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_last_login_utc_ms() -> CoreResult<Option<i64>> {
    Ok(None)
}

// Reads HKLM\SYSTEM\CurrentControlSet\Control\Windows\ShutdownTime (REG_BINARY FILETIME).
// The OS writes this key only during a clean shutdown, so it never advances
// across a crash/power-loss — see `read_eventlog_last_before_boot_ms` for the
// fallback that covers that case.
#[cfg(target_os = "windows")]
fn read_registry_shutdown_time_ms() -> Option<i64> {
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
            return None;
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
            return None;
        }

        let filetime = u64::from_le_bytes(data);
        if filetime < FILETIME_TO_UNIX_OFFSET {
            return None;
        }
        let unix_ms = (filetime - FILETIME_TO_UNIX_OFFSET) / 10_000;
        i64::try_from(unix_ms).ok()
    }
}

// Extracts the `SystemTime='...'` attribute of the first (i.e. newest, given
// `/rd:true`) `<TimeCreated>` element in `wevtutil qe ... /f:xml` output.
// `wevtutil` single-quotes XML attributes, not double-quotes.
fn parse_eventlog_timecreated_ms(xml: &str) -> Option<i64> {
    let key = "SystemTime='";
    let start = xml.find(key)? + key.len();
    let end = start + xml[start..].find('\'')?;
    let ts = &xml[start..end];
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// Falls back to the last System-log event timestamp recorded before the
// current boot, analogous to `journalctl --list-boots` on Linux: unlike the
// `ShutdownTime` registry key, Windows keeps writing to the event log right up
// until a crash/power-loss, so its last pre-boot entry is a floor for the
// true (unclean) shutdown moment. Best-effort: `None` on any failure.
#[cfg(target_os = "windows")]
fn read_eventlog_last_before_boot_ms(boot_start_utc_ms: i64) -> Option<i64> {
    let boot_time_iso = chrono::DateTime::from_timestamp_millis(boot_start_utc_ms)?
        .format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let query = format!("*[System[TimeCreated[@SystemTime<='{boot_time_iso}']]]");
    let output = Command::new("wevtutil")
        .args(["qe", "System", "/c:1", "/rd:true", "/f:xml"])
        .arg(format!("/q:{query}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_eventlog_timecreated_ms(&String::from_utf8_lossy(&output.stdout))
}

// This is a floor/approximation of the true logout time, not exact — see
// `LifecycleHooks::get_last_logout_utc_ms`. Combines the clean-shutdown
// registry timestamp with the event-log floor (which alone covers unclean
// shutdowns the registry key misses) and returns whichever is newer.
#[cfg(target_os = "windows")]
pub fn read_last_logout_utc_ms() -> CoreResult<Option<i64>> {
    let registry_ms = read_registry_shutdown_time_ms();

    let boot_start_utc_ms = read_boot_clock_ms()
        .ok()
        .and_then(|boot_ms| Some(read_utc_now_ms()? - boot_ms));
    let eventlog_ms = boot_start_utc_ms.and_then(read_eventlog_last_before_boot_ms);

    Ok(registry_ms.into_iter().chain(eventlog_ms).max())
}

#[cfg(target_os = "windows")]
fn read_utc_now_ms() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}

#[cfg(not(target_os = "windows"))]
pub fn read_last_logout_utc_ms() -> CoreResult<Option<i64>> {
    Ok(None)
}

// `QueryInterruptTime` returns 100ns units since boot, INCLUDING time spent
// suspended.
#[cfg(target_os = "windows")]
pub fn read_boot_clock_ms() -> CoreResult<i64> {
    use windows::Win32::System::WindowsProgramming::QueryInterruptTime;
    let time_100ns = unsafe { QueryInterruptTime() };
    Ok((time_100ns / 10_000) as i64)
}

#[cfg(not(target_os = "windows"))]
pub fn read_boot_clock_ms() -> CoreResult<i64> {
    Ok(0)
}

// `QueryUnbiasedInterruptTime` returns 100ns units since boot, EXCLUDING time
// spent suspended.
#[cfg(target_os = "windows")]
pub fn read_monotonic_clock_ms() -> CoreResult<i64> {
    use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;
    let mut time_100ns = 0u64;
    let _ = unsafe { QueryUnbiasedInterruptTime(&mut time_100ns) };
    Ok((time_100ns / 10_000) as i64)
}

#[cfg(not(target_os = "windows"))]
pub fn read_monotonic_clock_ms() -> CoreResult<i64> {
    Ok(0)
}

// True if a screensaver is running or the session is locked. Both checks fail safe
// to false (fall back to the diff gate) when the state can't be determined.
#[cfg(target_os = "windows")]
pub fn read_locked_or_screensaver() -> bool {
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP, OpenInputDesktop,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETSCREENSAVERRUNNING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    use windows::core::BOOL;

    unsafe {
        let mut running = BOOL(0);
        let screensaver = SystemParametersInfoW(
            SPI_GETSCREENSAVERRUNNING,
            0,
            Some(&mut running as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && running.as_bool();
        if screensaver {
            return true;
        }

        // When the workstation is locked the input desktop becomes the secure
        // (Winlogon) desktop, so a normal-privilege OpenInputDesktop fails.
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
            Ok(desktop) => {
                let _ = CloseDesktop(desktop);
                false
            }
            Err(_) => true,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_locked_or_screensaver() -> bool {
    false
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

impl ScreenshotHooks for WindowsPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes =
            capture_screen_png().map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes,
            content_type: "image/png".to_string(),
        })
    }

    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        Ok(read_locked_or_screensaver())
    }
}

impl LifecycleHooks for WindowsPlatformHooks {
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        read_last_login_utc_ms()
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        read_last_logout_utc_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_eventlog_timecreated_ms;

    #[test]
    fn parse_eventlog_timecreated_ms_extracts_newest_entry() {
        // `wevtutil qe ... /f:xml` single-quotes attributes, unlike typical XML.
        let xml = r#"<Events><Event><System><TimeCreated SystemTime='2026-07-10T01:06:12.345Z'/></System></Event></Events>"#;
        assert_eq!(parse_eventlog_timecreated_ms(xml), Some(1783645572345));
    }

    #[test]
    fn parse_eventlog_timecreated_ms_returns_none_on_empty_result() {
        assert_eq!(parse_eventlog_timecreated_ms("<Events></Events>"), None);
        assert_eq!(parse_eventlog_timecreated_ms(""), None);
    }

    #[test]
    fn parse_eventlog_timecreated_ms_returns_none_on_malformed_timestamp() {
        let xml = r#"<TimeCreated SystemTime='not-a-timestamp'/>"#;
        assert_eq!(parse_eventlog_timecreated_ms(xml), None);
    }

    #[test]
    fn parse_eventlog_timecreated_ms_handles_real_wevtutil_output() {
        let xml = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Kernel-General' Guid='{a68ca8b7-004f-d7b6-a698-07e2de0f1f5d}'/><EventID>16</EventID><TimeCreated SystemTime='2026-07-10T06:17:42.6765986Z'/><EventRecordID>4988</EventRecordID></System></Event>"#;
        assert_eq!(parse_eventlog_timecreated_ms(xml), Some(1783664262676));
    }
}
