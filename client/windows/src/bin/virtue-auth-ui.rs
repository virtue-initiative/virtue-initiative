#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]

use anyhow::Result;
use i_slint_backend_winit::WinitWindowAccessor;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;
use winit::dpi::PhysicalSize;
use winit::platform::windows::{IconExtWindows, WindowExtWindows};
use winit::window::Icon;

use virtue_windows::config::{ClientPaths, build_core_config};
use virtue_windows::runtime_env::apply_runtime_env;
use virtue_windows::service_log::ServiceLogger;
use virtue_windows::session::SessionManager;
use virtue_windows::win_text::to_wide;

slint::slint! {
    import { Button, LineEdit, VerticalBox, HorizontalBox } from "std-widgets.slint";

    export component AuthWindow inherits Window {
        in property <string> build_label;

        title: "Virtue - virtueinitiative.org " + build_label;
        width: 420px;
        height: 320px;

        in-out property <bool> logged_in;
        in-out property <string> account_email;
        in-out property <string> email_input;
        in-out property <string> password_input;
        in-out property <string> status_text;
        in-out property <string> api_base_url;

        callback login_request(string, string);
        callback logout_request();
        callback close_request();
        callback open_website_request();

        Rectangle {
            background: #eef3f8;

            Rectangle {
                x: 20px;
                y: 20px;
                width: parent.width - 40px;
                height: parent.height - 40px;
                background: white;
                border-radius: 12px;
                border-width: 1px;
                border-color: #dbe3ee;

                VerticalBox {
                    x: 20px;
                    y: 20px;
                    width: parent.width - 40px;
                    height: parent.height - 40px;
                    spacing: 12px;

                    Text {
                        text: logged_in ? "Virtue account" : "Sign in";
                        color: #0f172a;
                        font-size: 24px;
                        font-weight: 700;
                    }

                    Text {
                        text: "Build " + build_label;
                        color: #64748b;
                        font-size: 12px;
                    }

                    Text {
                        text: status_text;
                        color: #334155;
                        font-size: 14px;
                        wrap: word-wrap;
                    }

                    Text {
                        text: "API: " + api_base_url;
                        color: #64748b;
                        font-size: 12px;
                        wrap: word-wrap;
                    }

                    if logged_in : VerticalBox {
                        spacing: 8px;

                        Text {
                            text: "Signed in as " + account_email;
                            color: #0f172a;
                            font-size: 14px;
                        }

                        Button {
                            text: "Open virtueinitiative.org";
                            clicked => {
                                root.open_website_request();
                            }
                        }

                        HorizontalBox {
                            spacing: 10px;

                            Button {
                                text: "Sign out";
                                clicked => {
                                    root.logout_request();
                                }
                            }

                            Button {
                                text: "Close";
                                clicked => {
                                    root.close_request();
                                }
                            }
                        }
                    }

                    if !logged_in : VerticalBox {
                        spacing: 8px;

                        LineEdit {
                            text <=> root.email_input;
                            placeholder-text: "Email";
                        }

                        LineEdit {
                            text <=> root.password_input;
                            placeholder-text: "Password";
                            input-type: InputType.password;
                        }

                        Button {
                            text: "Open virtueinitiative.org";
                            clicked => {
                                root.open_website_request();
                            }
                        }

                        HorizontalBox {
                            spacing: 10px;

                            Button {
                                text: "Sign in";
                                clicked => {
                                    root.login_request(root.email_input, root.password_input);
                                }
                            }

                            Button {
                                text: "Close";
                                clicked => {
                                    root.close_request();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

const BUILD_LABEL: &str = virtue_core::BUILD_LABEL;
const VIRTUE_WEBSITE_URL: &str = "https://virtueinitiative.org";

fn main() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = ServiceLogger::new(paths.log_file.clone());
    logger.info(&format!("auth ui starting ({BUILD_LABEL})"));

    slint::BackendSelector::new()
        .backend_name("winit".to_string())
        .renderer_name("software".to_string())
        .select()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let session = SessionManager::new()?;

    let ui = AuthWindow::new().map_err(|err| anyhow::anyhow!(err.to_string()))?;
    configure_taskbar_icon(&ui);
    let initial = session.status()?;
    let mut core_config = build_core_config(&paths);
    core_config.refresh_from_runtime_file()?;

    ui.set_build_label(BUILD_LABEL.into());
    ui.set_logged_in(initial.logged_in);
    ui.set_api_base_url(core_config.api_base_url.clone().into());
    ui.set_account_email(initial.email.clone().unwrap_or_default().into());
    ui.set_email_input(initial.email.unwrap_or_default().into());
    if ui.get_logged_in() {
        ui.set_status_text("Monitoring is active on this device".into());
    } else {
        ui.set_status_text("Sign in to start monitoring".into());
    }

    ui.on_close_request(|| {
        let _ = slint::quit_event_loop();
    });

    ui.on_open_website_request(|| {
        let _ = open_virtue_website();
    });

    let login_weak = ui.as_weak();
    let login_session = session.clone();
    ui.on_login_request(move |email, password| {
        let email = email.trim().to_string();
        let password = password.to_string();

        let Some(window) = login_weak.upgrade() else {
            return;
        };

        if email.is_empty() || password.is_empty() {
            window.set_status_text("Email and password are required".into());
            return;
        }

        window.set_status_text("Signing in...".into());

        let device_name = hostname::get()
            .ok()
            .and_then(|value| value.into_string().ok())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "windows-device".to_string());

        match login_session.login_blocking(&email, &password, &device_name) {
            Ok(_) => {
                window.set_logged_in(true);
                window.set_account_email(email.clone().into());
                window.set_email_input(email.clone().into());
                window.set_password_input("".into());
                window.set_status_text("Monitoring is active on this device".into());
            }
            Err(err) => {
                window.set_status_text(format!("Sign in failed: {err}").into());
            }
        }
    });

    let logout_weak = ui.as_weak();
    let logout_session = session.clone();
    ui.on_logout_request(move || {
        let Some(window) = logout_weak.upgrade() else {
            return;
        };

        window.set_status_text("Signing out...".into());

        match logout_session.logout_blocking() {
            Ok(()) => {
                window.set_logged_in(false);
                window.set_account_email("".into());
                window.set_password_input("".into());
                window.set_status_text("Signed out".into());
            }
            Err(err) => {
                window.set_status_text(format!("Sign out failed: {err}").into());
            }
        }
    });

    match ui.run().map_err(|err| anyhow::anyhow!(err.to_string())) {
        Ok(()) => {
            logger.info(&format!("auth ui closed ({BUILD_LABEL})"));
            Ok(())
        }
        Err(err) => {
            logger.warn(&format!("auth ui failed: {err:#}"));
            Err(err)
        }
    }
}

fn configure_taskbar_icon(ui: &AuthWindow) {
    let ui_weak = ui.as_weak();
    let _ = slint::spawn_local(async move {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        let Ok(winit_window) = ui.window().winit_window().await else {
            return;
        };

        let Ok(exe_path) = std::env::current_exe() else {
            return;
        };
        let icon_path = exe_path.with_file_name("app-icon.ico");
        if !icon_path.exists() {
            return;
        }

        let Ok(icon) = Icon::from_path(&icon_path, Some(PhysicalSize::new(256, 256))) else {
            return;
        };

        winit_window.set_taskbar_icon(Some(icon.clone()));
        winit_window.set_window_icon(Some(icon));
    });
}

fn open_virtue_website() -> Result<()> {
    let operation = to_wide("open");
    let target = to_wide(VIRTUE_WEBSITE_URL);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };

    if (result.0 as usize) <= 32 {
        Err(anyhow::anyhow!(
            "ShellExecuteW failed with code {}",
            result.0 as usize
        ))
    } else {
        Ok(())
    }
}
