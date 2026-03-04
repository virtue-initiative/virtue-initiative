#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]

use std::sync::Arc;

use anyhow::Result;
use tokio::runtime::Builder;

use virtue_windows_client::config::ClientPaths;
use virtue_windows_client::runtime_env::apply_runtime_env;
use virtue_windows_client::service_log::ServiceLogger;
use virtue_windows_client::session::SessionManager;

slint::slint! {
    import { Button, LineEdit, VerticalBox, HorizontalBox } from "std-widgets.slint";

    export component AuthWindow inherits Window {
        title: "Virtue";
        width: 420px;
        height: 320px;

        in-out property <bool> logged_in;
        in-out property <string> account_email;
        in-out property <string> email_input;
        in-out property <string> password_input;
        in-out property <string> status_text;

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
                        text: status_text;
                        color: #334155;
                        font-size: 14px;
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

fn main() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = ServiceLogger::new(paths.log_file.clone());

    // Force software rendering: winit backend expects "software"/"sw" renderer names.
    slint::BackendSelector::new()
        .backend_name("winit".to_string())
        .renderer_name("software".to_string())
        .select()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let session = SessionManager::new()?;
    let runtime = Arc::new(Builder::new_multi_thread().enable_all().build()?);

    let ui = AuthWindow::new().map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let initial = session.status()?;

    ui.set_logged_in(initial.logged_in);
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
    let login_runtime = runtime.clone();
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

        match login_session.login_blocking(login_runtime.as_ref(), &email, &password, &device_name)
        {
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
    let logout_runtime = runtime.clone();
    ui.on_logout_request(move || {
        let Some(window) = logout_weak.upgrade() else {
            return;
        };

        window.set_status_text("Signing out...".into());

        match logout_session.logout_blocking(logout_runtime.as_ref()) {
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
            logger.info("auth ui closed");
            Ok(())
        }
        Err(err) => {
            logger.warn(&format!("auth ui failed: {err:#}"));
            Err(err)
        }
    }
}
