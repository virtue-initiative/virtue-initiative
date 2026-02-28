#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ptr::null_mut;

use tokio::runtime::Builder;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD,
    GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU,
    IDC_ARROW, IDI_APPLICATION, LoadCursorW, LoadIconW, MF_STRING, MSG, PostQuitMessage,
    RegisterClassW, SW_HIDE, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY,
    WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONUP, WNDCLASSW, WS_BORDER, WS_CHILD,
    WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use virtue_windows_client::session::SessionManager;
use virtue_windows_client::win_text::to_wide;

const WINDOW_CLASS: PCWSTR = w!("VirtueTrayWindow");
const WINDOW_TITLE: PCWSTR = w!("Virtue");

const ID_EMAIL_INPUT: isize = 1001;
const ID_PASSWORD_INPUT: isize = 1002;
const ID_LOGIN_BUTTON: isize = 1003;
const ID_LOGOUT_BUTTON: isize = 1004;

const ID_TRAY_OPEN: u16 = 2001;
const ID_TRAY_EXIT: u16 = 2002;

const WM_TRAYICON: u32 = WM_APP + 1;

struct AppState {
    hwnd: HWND,
    status_label: HWND,
    email_input: HWND,
    password_input: HWND,
    login_button: HWND,
    logout_button: HWND,
    tray_added: bool,
    tray_menu: HMENU,
    runtime: tokio::runtime::Runtime,
    session: SessionManager,
}

impl AppState {
    fn new() -> anyhow::Result<Self> {
        Ok(Self {
            hwnd: HWND(null_mut()),
            status_label: HWND(null_mut()),
            email_input: HWND(null_mut()),
            password_input: HWND(null_mut()),
            login_button: HWND(null_mut()),
            logout_button: HWND(null_mut()),
            tray_added: false,
            tray_menu: HMENU(null_mut()),
            runtime: Builder::new_multi_thread().enable_all().build()?,
            session: SessionManager::new()?,
        })
    }

    unsafe fn init_ui(&mut self) {
        self.status_label = create_control(
            self.hwnd,
            w!("STATIC"),
            "Starting...",
            20,
            20,
            370,
            22,
            WS_CHILD | WS_VISIBLE,
            0,
        );

        let _ = create_control(
            self.hwnd,
            w!("STATIC"),
            "Email",
            20,
            56,
            370,
            20,
            WS_CHILD | WS_VISIBLE,
            0,
        );

        self.email_input = create_control(
            self.hwnd,
            w!("EDIT"),
            "",
            20,
            76,
            370,
            26,
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            ID_EMAIL_INPUT,
        );

        let _ = create_control(
            self.hwnd,
            w!("STATIC"),
            "Password",
            20,
            112,
            370,
            20,
            WS_CHILD | WS_VISIBLE,
            0,
        );

        self.password_input = create_control(
            self.hwnd,
            w!("EDIT"),
            "",
            20,
            132,
            370,
            26,
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | WINDOW_STYLE(ES_AUTOHSCROLL as u32)
                | WINDOW_STYLE(ES_PASSWORD as u32),
            ID_PASSWORD_INPUT,
        );

        self.login_button = create_control(
            self.hwnd,
            w!("BUTTON"),
            "Sign in",
            20,
            176,
            180,
            34,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_LOGIN_BUTTON,
        );

        self.logout_button = create_control(
            self.hwnd,
            w!("BUTTON"),
            "Sign out",
            20,
            176,
            180,
            34,
            WS_CHILD | WS_TABSTOP,
            ID_LOGOUT_BUTTON,
        );

        let _ = ShowWindow(self.logout_button, SW_HIDE);

        self.tray_menu = CreatePopupMenu().expect("failed to create tray menu");
        let _ = AppendMenuW(self.tray_menu, MF_STRING, ID_TRAY_OPEN as usize, w!("Open"));
        let _ = AppendMenuW(self.tray_menu, MF_STRING, ID_TRAY_EXIT as usize, w!("Exit"));

        self.add_tray_icon();
        self.refresh_logged_in_ui();
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
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            ..Default::default()
        };

        let tip = to_wide("Virtue");
        for (idx, ch) in tip.iter().take(data.szTip.len()).enumerate() {
            data.szTip[idx] = *ch;
        }

        let added = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
        self.tray_added = added;
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
    }

    unsafe fn on_login(&mut self) {
        let email = self.get_text(self.email_input);
        let password = self.get_text(self.password_input);

        if email.trim().is_empty() || password.is_empty() {
            self.set_status("Email and password are required");
            return;
        }

        self.set_status("Signing in...");

        let device_name = hostname::get()
            .ok()
            .and_then(|value| value.into_string().ok())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "windows-device".to_string());

        let result = self
            .session
            .login_blocking(&self.runtime, &email, &password, &device_name);

        match result {
            Ok(device_id) => {
                self.set_status(&format!("Signed in. Device id: {device_id}"));
                self.refresh_logged_in_ui();
            }
            Err(err) => self.set_status(&format!("Login failed: {err:#}")),
        }
    }

    unsafe fn on_logout(&mut self) {
        self.set_status("Signing out...");

        let result = self.session.logout_blocking(&self.runtime);

        match result {
            Ok(()) => {
                self.set_status("Signed out");
                self.refresh_logged_in_ui();
            }
            Err(err) => self.set_status(&format!("Sign out warning: {err:#}")),
        }
    }

    unsafe fn refresh_logged_in_ui(&mut self) {
        let status = self.session.status();

        match status {
            Ok(session) if session.logged_in => {
                let _ = ShowWindow(self.email_input, SW_HIDE);
                let _ = ShowWindow(self.password_input, SW_HIDE);
                let _ = ShowWindow(self.login_button, SW_HIDE);
                let _ = ShowWindow(self.logout_button, SW_SHOW);
                if let Some(device_id) = session.device_id {
                    self.set_status(&format!("Signed in. Device id: {device_id}"));
                } else {
                    self.set_status("Signed in");
                }
            }
            Ok(_) => {
                let _ = ShowWindow(self.email_input, SW_SHOW);
                let _ = ShowWindow(self.password_input, SW_SHOW);
                let _ = ShowWindow(self.login_button, SW_SHOW);
                let _ = ShowWindow(self.logout_button, SW_HIDE);
                self.set_status("Sign in to start monitoring");
            }
            Err(err) => {
                self.set_status(&format!("State error: {err:#}"));
            }
        }
    }

    unsafe fn set_status(&self, value: &str) {
        let text = to_wide(value);
        let _ = SetWindowTextW(self.status_label, PCWSTR(text.as_ptr()));
    }

    unsafe fn get_text(&self, hwnd: HWND) -> String {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        let _ = GetWindowTextW(hwnd, &mut buf);
        let slice = &buf[..len as usize];
        String::from_utf16_lossy(slice)
    }

    unsafe fn show_window(&self) {
        let _ = ShowWindow(self.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(self.hwnd);
    }

    unsafe fn hide_window(&self) {
        let _ = ShowWindow(self.hwnd, SW_HIDE);
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

unsafe fn create_control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
    id: isize,
) -> HWND {
    let wtext = to_wide(text);
    CreateWindowExW(
        WS_EX_CLIENTEDGE,
        class,
        PCWSTR(wtext.as_ptr()),
        style,
        x,
        y,
        width,
        height,
        Some(parent),
        Some(HMENU(id as *mut _)),
        None,
        None,
    )
    .expect("failed creating control")
}

fn loword(value: usize) -> u16 {
    (value & 0xFFFF) as u16
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
                match command_id as isize {
                    ID_LOGIN_BUTTON => {
                        state.on_login();
                        return LRESULT(0);
                    }
                    ID_LOGOUT_BUTTON => {
                        state.on_logout();
                        return LRESULT(0);
                    }
                    id if id as u16 == ID_TRAY_OPEN => {
                        state.show_window();
                        return LRESULT(0);
                    }
                    id if id as u16 == ID_TRAY_EXIT => {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
        }
        WM_TRAYICON => {
            if let Some(state) = app_state_mut(hwnd) {
                match lparam.0 as u32 {
                    WM_LBUTTONUP => {
                        state.show_window();
                        return LRESULT(0);
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        state.show_tray_menu();
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
        }
        WM_CLOSE => {
            if let Some(state) = app_state_mut(hwnd) {
                state.hide_window();
                return LRESULT(0);
            }
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
                let _ = Box::from_raw(ptr);
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
    unsafe {
        let hinstance = GetModuleHandleW(None)?;

        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };

        let atom = RegisterClassW(&class);
        if atom == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }

        let app = Box::new(AppState::new()?);
        let app_ptr = Box::into_raw(app);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WINDOW_CLASS,
            WINDOW_TITLE,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            430,
            290,
            None,
            None,
            Some(hinstance.into()),
            Some(app_ptr.cast()),
        )?;

        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}
