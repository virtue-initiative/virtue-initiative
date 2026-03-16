#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]

use anyhow::Result;

use virtue_windows::config::{ClientPaths, build_core_config};
use virtue_windows::runtime_env::apply_runtime_env;
use virtue_windows::service_log::ServiceLogger;
use virtue_windows::session::SessionManager;

slint::slint! {
    import { Button, LineEdit, VerticalBox, HorizontalBox } from "std-widgets.slint";

    export component AuthWindow inherits Window {
        in property <string> build_label;

        title: "Virtue " + build_label;
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
                        text: logged_in ? "Virtue Account" : "Sign In";
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

const BUILD_LABEL: &str = env!("CARGO_PKG_VERSION");

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
