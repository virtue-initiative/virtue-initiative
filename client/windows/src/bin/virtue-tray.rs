#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

use std::process::Command;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::OnceLock;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateIcon, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, HICON, HMENU, IDC_ARROW, IDI_APPLICATION, IMAGE_ICON,
    LR_DEFAULTSIZE, LR_LOADFROMFILE, LoadCursorW, LoadIconW, LoadImageW, MF_STRING, MSG,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, TPM_LEFTALIGN,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE,
    WM_NCDESTROY, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

use virtue_windows::config::ClientPaths;
use virtue_windows::runtime_env::apply_runtime_env;
use virtue_windows::service_log::ServiceLogger;
use virtue_windows::win_text::to_wide;

const WINDOW_CLASS: PCWSTR = w!("VirtueTrayWindow");
const WINDOW_TITLE: PCWSTR = w!("Virtue");
const BUILD_LABEL: &str = env!("CARGO_PKG_VERSION");

const ID_TRAY_OPEN: u16 = 2001;
const ID_TRAY_EXIT: u16 = 2002;
const TRAY_INSTANCE_MUTEX_NAME: PCWSTR = w!("Local\\VirtueTrayInstance");

const WM_TRAYICON: u32 = WM_APP + 1;
static TASKBAR_CREATED_MSG: OnceLock<u32> = OnceLock::new();

struct AppState {
    hwnd: HWND,
    tray_added: bool,
    tray_add_logged: bool,
    tray_add_warned: bool,
    tray_icon: HICON,
    tray_menu: HMENU,
    logger: Arc<ServiceLogger>,
}

impl AppState {
    fn new(logger: Arc<ServiceLogger>) -> Self {
        Self {
            hwnd: HWND(null_mut()),
            tray_added: false,
            tray_add_logged: false,
            tray_add_warned: false,
            tray_icon: HICON(null_mut()),
            tray_menu: HMENU(null_mut()),
            logger,
        }
    }

    unsafe fn init_ui(&mut self) {
        self.tray_menu = CreatePopupMenu().expect("failed to create tray menu");
        let _ = AppendMenuW(self.tray_menu, MF_STRING, ID_TRAY_OPEN as usize, w!("Open"));
        let _ = AppendMenuW(self.tray_menu, MF_STRING, ID_TRAY_EXIT as usize, w!("Exit"));

        self.add_tray_icon();
    }

    unsafe fn add_tray_icon(&mut self) {
        if self.tray_added {
            return;
        }

        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
            uCallbackMessage: WM_TRAYICON,
            hIcon: self.ensure_tray_icon(),
            ..Default::default()
        };

        let tip = to_wide(&format!("Virtue {BUILD_LABEL}"));
        for (idx, ch) in tip.iter().take(data.szTip.len()).enumerate() {
            data.szTip[idx] = *ch;
        }

        let added = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
        self.tray_added = added;
        if added {
            if !self.tray_add_logged {
                self.logger.info("tray icon registered");
                self.tray_add_logged = true;
            }
            if self.tray_add_warned {
                self.logger.info("tray icon registration recovered");
                self.tray_add_warned = false;
            }
        } else if !self.tray_add_warned {
            self.logger.warn("tray icon registration failed");
            self.tray_add_warned = true;
        }
    }

    unsafe fn resolve_tray_icon() -> HICON {
        if let Ok(exe_path) = std::env::current_exe() {
            let icon_path = exe_path.with_file_name("app-icon.ico");
            if icon_path.exists() {
                let icon_path_wide = to_wide(icon_path.to_string_lossy().as_ref());
                if let Ok(handle) = LoadImageW(
                    None,
                    PCWSTR(icon_path_wide.as_ptr()),
                    IMAGE_ICON,
                    0,
                    0,
                    LR_LOADFROMFILE | LR_DEFAULTSIZE,
                ) && !handle.is_invalid()
                {
                    return HICON(handle.0);
                }
            }
        }

        create_green_circle_icon()
            .unwrap_or_else(|| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }

    unsafe fn remove_tray_icon(&mut self) {
        if !self.tray_added {
            return;
        }

        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        };

        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        self.tray_added = false;
        self.tray_add_logged = false;
    }

    unsafe fn ensure_tray_icon(&mut self) -> HICON {
        if !self.tray_icon.0.is_null() {
            return self.tray_icon;
        }

        self.tray_icon = Self::resolve_tray_icon();
        self.tray_icon
    }

    unsafe fn destroy_tray_icon(&mut self) {
        if self.tray_icon.0.is_null() {
            return;
        }
        let _ = DestroyIcon(self.tray_icon);
        self.tray_icon = HICON(null_mut());
    }

    fn open_auth_dialog(&self) {
        let ui_path = match std::env::current_exe() {
            Ok(path) => path.with_file_name("virtue-auth-ui.exe"),
            Err(err) => {
                self.logger
                    .warn(&format!("cannot resolve ui path from current exe: {err}"));
                return;
            }
        };

        if !ui_path.exists() {
            self.logger.warn(&format!(
                "auth ui executable missing: {}",
                ui_path.display()
            ));
            return;
        }

        match Command::new(&ui_path).spawn() {
            Ok(_) => self.logger.info("auth ui launch requested"),
            Err(err) => self
                .logger
                .warn(&format!("failed to launch auth ui process: {err}")),
        }
    }

    unsafe fn show_tray_menu(&self) {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(self.hwnd);
        let _ = TrackPopupMenu(
            self.tray_menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            Some(0),
            self.hwnd,
            None,
        );
    }
}

fn loword(value: usize) -> u16 {
    (value & 0xFFFF) as u16
}

unsafe fn create_green_circle_icon() -> Option<HICON> {
    let (width, height, rgba) = build_default_tray_icon_rgba();
    let mut xor_bits = vec![0u8; (width * height * 4) as usize];
    let and_stride = width.div_ceil(32) * 4;
    let mut and_bits = vec![0u8; (and_stride * height) as usize];

    for y in 0..height as usize {
        for x in 0..width as usize {
            let src = (y * width as usize + x) * 4;
            let dst = src;
            let r = rgba[src];
            let g = rgba[src + 1];
            let b = rgba[src + 2];
            let a = rgba[src + 3];

            xor_bits[dst] = b;
            xor_bits[dst + 1] = g;
            xor_bits[dst + 2] = r;
            xor_bits[dst + 3] = a;

            if a == 0 {
                let mask_index = y * and_stride as usize + (x / 8);
                and_bits[mask_index] |= 0x80u8 >> (x % 8);
            }
        }
    }

    let icon = CreateIcon(
        None,
        width as i32,
        height as i32,
        1,
        32,
        and_bits.as_ptr(),
        xor_bits.as_ptr(),
    )
    .ok()?;

    if icon.0.is_null() { None } else { Some(icon) }
}

fn build_default_tray_icon_rgba() -> (u32, u32, Vec<u8>) {
    let width = 16u32;
    let height = 16u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * width + x) * 4) as usize;
            if dist <= 6.0 {
                rgba[idx] = 0x28;
                rgba[idx + 1] = 0xa7;
                rgba[idx + 2] = 0x45;
                rgba[idx + 3] = 0xff;
            }
        }
    }

    (width, height, rgba)
}

unsafe fn app_state_mut(hwnd: HWND) -> Option<&'static mut AppState> {
    let ptr = GetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA)
        as *mut AppState;
    if ptr.is_null() { None } else { Some(&mut *ptr) }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(taskbar_created) = TASKBAR_CREATED_MSG.get()
        && msg == *taskbar_created
    {
        if let Some(state) = app_state_mut(hwnd) {
            state.tray_added = false;
            state.add_tray_icon();
        }
        return LRESULT(0);
    }

    match msg {
        WM_NCCREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let ptr = create.lpCreateParams as *mut AppState;
            if !ptr.is_null() {
                (*ptr).hwnd = hwnd;
                let _ = SetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    ptr as isize,
                );
            }
            return LRESULT(1);
        }
        WM_CREATE => {
            if let Some(state) = app_state_mut(hwnd) {
                state.init_ui();
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            let command_id = loword(wparam.0);
            if let Some(state) = app_state_mut(hwnd) {
                match command_id {
                    ID_TRAY_OPEN => {
                        state.open_auth_dialog();
                        return LRESULT(0);
                    }
                    ID_TRAY_EXIT => {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
        }
        WM_TRAYICON => match lparam.0 as u32 {
            WM_LBUTTONUP => {
                if let Some(state) = app_state_mut(hwnd) {
                    state.open_auth_dialog();
                }
                return LRESULT(0);
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                if let Some(state) = app_state_mut(hwnd) {
                    state.show_tray_menu();
                }
                return LRESULT(0);
            }
            _ => {}
        },
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            if let Some(state) = app_state_mut(hwnd) {
                state.remove_tray_icon();
            }
            PostQuitMessage(0);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            let ptr =
                GetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA)
                    as *mut AppState;
            if !ptr.is_null() {
                let mut boxed = Box::from_raw(ptr);
                boxed.remove_tray_icon();
                boxed.destroy_tray_icon();
                let _ = SetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    0,
                );
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() -> anyhow::Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);

    let startup_logger = Arc::new(ServiceLogger::new(paths.log_file.clone()));
    startup_logger.info(&format!("build {BUILD_LABEL}"));

    let instance = unsafe { CreateMutexW(None, false, TRAY_INSTANCE_MUTEX_NAME)? };
    let last_error = unsafe { windows::Win32::Foundation::GetLastError() };
    if last_error == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(instance) };
        startup_logger.info("tray instance already running");
        return Ok(());
    }

    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hInstance: hinstance.into(),
        lpszClassName: WINDOW_CLASS,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };

    unsafe {
        RegisterClassW(&class);
    }

    let taskbar_created = unsafe {
        windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(w!("TaskbarCreated"))
    };
    let _ = TASKBAR_CREATED_MSG.set(taskbar_created);

    let state = Box::new(AppState::new(startup_logger.clone()));
    let state_ptr = Box::into_raw(state);

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            WINDOW_TITLE,
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinstance.into()),
            Some(state_ptr.cast()),
        )
    }?;

    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = DestroyWindow(hwnd);
        let _ = CloseHandle(instance);
    }

    Ok(())
}
