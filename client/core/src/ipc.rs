//! Cross-process daemon IPC (Linux/macOS only).
//!
//! Windows, Android, and iOS each hold one process-global `Arc<Daemon<..>>`
//! and call its public methods directly, in-process — that already *is* the
//! "thread channel" this crate offers those platforms, via the request
//! channel in `daemon.rs`. Nothing in this file applies to them.
//!
//! Linux and macOS additionally run their CLI/tray as a *separate OS
//! process* from the resident daemon, so they need a real cross-process
//! transport on top of the same `Daemon` methods. This file is that
//! transport: a thin newline-delimited-JSON protocol over a Unix socket that
//! decodes a [`WireRequest`], calls the matching `Daemon` method (which
//! internally uses the same request channel any in-process caller would),
//! and encodes the result back as a [`WireReply`].
//!
//! Only one client is ever connected at a time — the CLI/tray talks to the
//! daemon one command at a time — so the server is a single thread that
//! loops `accept` -> serve that connection to completion -> `accept` again;
//! a second client's `connect()` simply blocks until the first disconnects,
//! via the OS listen backlog.
//!
//! This module gates itself rather than being gated at its `mod` declaration,
//! so `target_os` lives in exactly one file: everything platform-conditional
//! about the client is the existence of this transport.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::daemon::Daemon;
use crate::error::{CoreError, CoreResult};
use crate::model::{Redacted, ServiceStatus};
use crate::module::upload::Upload;
use crate::platform::PlatformHooks;

// ── Wire protocol ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
enum WireRequest {
    Login {
        email: String,
        password: Redacted<String>,
        device_name: Option<String>,
    },
    Logout,
    Status,
    NoteUserStop {
        source: String,
    },
    QueueUpload {
        upload: Upload,
    },
    FlushBatchNow,
    ForceCapture,
}

#[derive(Debug, Serialize, Deserialize)]
enum WireReply {
    LoginResult {
        success: bool,
        error: Option<String>,
        device_id: Option<String>,
    },
    LogoutResult {
        success: bool,
        error: Option<String>,
    },
    Status {
        status: ServiceStatus,
    },
    Ack,
}

fn write_line<W: Write>(writer: &mut W, reply: &WireReply) -> std::io::Result<()> {
    let json = serde_json::to_string(reply).expect("WireReply always serializes");
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn dispatch<P, A>(daemon: &Daemon<P, A>, request: WireRequest) -> WireReply
where
    P: PlatformHooks,
    A: ApiTransport + Send + Sync + 'static,
{
    match request {
        WireRequest::Login {
            email,
            password,
            device_name,
        } => match daemon.login(&email, &password.0, device_name.as_deref()) {
            Ok(device_id) => WireReply::LoginResult {
                success: true,
                error: None,
                device_id: Some(device_id),
            },
            Err(err) => WireReply::LoginResult {
                success: false,
                error: Some(err.to_string()),
                device_id: None,
            },
        },
        WireRequest::Logout => match daemon.logout() {
            Ok(()) => WireReply::LogoutResult {
                success: true,
                error: None,
            },
            Err(err) => WireReply::LogoutResult {
                success: false,
                error: Some(err.to_string()),
            },
        },
        WireRequest::Status => WireReply::Status {
            status: daemon.status(),
        },
        WireRequest::NoteUserStop { source } => {
            daemon.note_user_stop(&source);
            WireReply::Ack
        }
        WireRequest::QueueUpload { upload } => {
            daemon.queue_upload(upload);
            WireReply::Ack
        }
        WireRequest::FlushBatchNow => {
            daemon.flush_batch_now();
            WireReply::Ack
        }
        WireRequest::ForceCapture => {
            daemon.force_capture_now();
            WireReply::Ack
        }
    }
}

/// Spawn the one IPC-serving thread, listening at `sock_path` for the
/// lifetime of the process. Logs and returns without spawning on bind
/// failure.
pub fn spawn_server<P, A>(sock_path: PathBuf, daemon: Arc<Daemon<P, A>>)
where
    P: PlatformHooks,
    A: ApiTransport + Send + Sync + 'static,
{
    let _ = std::fs::remove_file(&sock_path);
    let listener = match UnixListener::bind(&sock_path) {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(
                error = %err,
                path = %sock_path.display(),
                "daemon: failed to bind IPC listener"
            );
            return;
        }
    };

    thread::spawn(move || {
        loop {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(err) => {
                    tracing::error!(error = %err, "daemon: ipc accept error");
                    continue;
                }
            };
            serve_connection(stream, &daemon);
        }
    });
}

fn serve_connection<P, A>(stream: UnixStream, daemon: &Arc<Daemon<P, A>>)
where
    P: PlatformHooks,
    A: ApiTransport + Send + Sync + 'static,
{
    let Ok(mut write_half) = stream.try_clone() else {
        tracing::error!("daemon: ipc failed to clone connection for writing");
        return;
    };

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF: peer disconnected
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(error = %err, "daemon: ipc read error");
                break;
            }
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let request: WireRequest = match serde_json::from_str(trimmed) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!(error = %err, "daemon: ipc decode error");
                continue;
            }
        };
        let reply = dispatch(daemon, request);
        if write_line(&mut write_half, &reply).is_err() {
            break;
        }
    }
}

// ── Client ────────────────────────────────────────────────────────────────

fn connect_error(err: std::io::Error) -> CoreError {
    if matches!(
        err.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    ) {
        CoreError::Ipc("daemon is not running".to_string())
    } else {
        CoreError::Io(err)
    }
}

/// IPC client used by the CLI/tray to talk to the resident daemon: login,
/// logout, status, and friends. A stable boundary every Linux/macOS platform
/// crate depends on.
pub struct ClientController {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl ClientController {
    /// Connect to the daemon listening at `path`.
    pub fn connect(path: &Path) -> CoreResult<Self> {
        let stream = UnixStream::connect(path).map_err(connect_error)?;
        let reader_stream = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(reader_stream),
            writer: stream,
        })
    }

    /// Send `request` and block for its reply. The daemon only ever writes
    /// in response to a request, so the next line on the socket is always
    /// this call's reply.
    fn call(&mut self, request: WireRequest) -> CoreResult<WireReply> {
        let json = serde_json::to_string(&request)?;
        self.writer.write_all(json.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(CoreError::Ipc("connection to daemon closed".to_string()));
        }
        Ok(serde_json::from_str(line.trim_end())?)
    }

    /// Send a login request and block until the reply is received. Returns
    /// the device ID on success.
    pub fn login(
        &mut self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        match self.call(WireRequest::Login {
            email: email.into(),
            password: Redacted(password.into()),
            device_name: device_name.map(String::from),
        })? {
            WireReply::LoginResult {
                success: true,
                device_id,
                ..
            } => Ok(device_id.unwrap_or_default()),
            WireReply::LoginResult {
                success: false,
                error,
                ..
            } => Err(CoreError::Remote(
                error
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "login failed".to_string()),
            )),
            _ => Err(CoreError::Ipc("unexpected reply to login".to_string())),
        }
    }

    /// Send a logout request and block until the reply is received.
    pub fn logout(&mut self) -> CoreResult<()> {
        match self.call(WireRequest::Logout)? {
            WireReply::LogoutResult { success: true, .. } => Ok(()),
            WireReply::LogoutResult {
                success: false,
                error,
            } => Err(CoreError::Remote(
                error
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "logout failed".to_string()),
            )),
            _ => Err(CoreError::Ipc("unexpected reply to logout".to_string())),
        }
    }

    /// Send a status request and block until the reply is received.
    pub fn get_status(&mut self) -> CoreResult<ServiceStatus> {
        match self.call(WireRequest::Status)? {
            WireReply::Status { status } => Ok(status),
            _ => Err(CoreError::Ipc("unexpected reply to status".to_string())),
        }
    }

    pub fn request_user_stop(&mut self, source: &str) -> CoreResult<()> {
        self.call(WireRequest::NoteUserStop {
            source: source.into(),
        })?;
        Ok(())
    }

    /// Queue `upload` into the daemon's live batch/hash pipeline.
    pub fn queue_upload(&mut self, upload: Upload) -> CoreResult<()> {
        self.call(WireRequest::QueueUpload { upload })?;
        Ok(())
    }

    /// Ask the daemon to flush its currently queued batch items now, instead
    /// of waiting for the batch interval timer.
    pub fn flush_batch_now(&mut self) -> CoreResult<()> {
        self.call(WireRequest::FlushBatchNow)?;
        Ok(())
    }

    /// Ask the daemon to force an immediate screenshot capture (bypassing the
    /// normal interval-due gate, but still honoring the locked/screensaver
    /// and fingerprint-dedup gates) and flush it out right away.
    pub fn force_capture_now(&mut self) -> CoreResult<()> {
        self.call(WireRequest::ForceCapture)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::testing::{MockApiClient, TestPlatformHooks};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    fn test_sock_path(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "virtue-ipc-test-{name}-{}-{}.sock",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn spawn_test_daemon(sock: &Path) -> Arc<Daemon<TestPlatformHooks, MockApiClient>> {
        let state_dir = std::env::temp_dir().join(format!(
            "virtue-ipc-test-state-{}-{}",
            std::process::id(),
            COUNTER_FOR_STATE_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let config = Config::new(
            "https://ipc-test.invalid",
            "test-device",
            "test-platform",
            state_dir.clone(),
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let daemon = Arc::new(
            Daemon::new(
                config,
                TestPlatformHooks::new(),
                MockApiClient::new(),
                state_dir.join("event_state.json"),
            )
            .expect("daemon must construct"),
        );
        spawn_server(sock.to_path_buf(), Arc::clone(&daemon));
        // Give the server thread a moment to bind before the client connects.
        std::thread::sleep(Duration::from_millis(50));
        daemon
    }

    static COUNTER_FOR_STATE_DIR: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn status_round_trips_over_the_socket() {
        let sock = test_sock_path("status");
        let _daemon = spawn_test_daemon(&sock);

        let mut controller = ClientController::connect(&sock).expect("connect");
        let status = controller.get_status().expect("status");
        assert!(status.is_running);
        assert!(!status.is_authenticated);

        let _ = std::fs::remove_file(&sock);
    }

    #[test]
    fn force_capture_round_trips_over_the_socket() {
        let sock = test_sock_path("force-capture");
        let _daemon = spawn_test_daemon(&sock);

        let mut controller = ClientController::connect(&sock).expect("connect");
        controller.force_capture_now().expect("force capture");

        let _ = std::fs::remove_file(&sock);
    }

    #[test]
    fn a_second_client_is_served_after_the_first_disconnects() {
        let sock = test_sock_path("single-client");
        let _daemon = spawn_test_daemon(&sock);

        let mut first = ClientController::connect(&sock).expect("connect first");
        assert!(first.get_status().is_ok());
        drop(first);

        // The single serving thread has returned to `accept()` since the
        // first connection dropped, so a second connection is served
        // promptly rather than hanging forever.
        let mut second = ClientController::connect(&sock).expect("connect second");
        assert!(second.get_status().is_ok());

        let _ = std::fs::remove_file(&sock);
    }

    /// A hand-rolled daemon-side stub (not a real `Daemon`) that replies to
    /// one request with a failure carrying an empty error string, mimicking
    /// a daemon-reported failure with no message.
    fn spawn_empty_error_stub(sock: &Path) {
        let _ = std::fs::remove_file(sock);
        let listener = UnixListener::bind(sock).expect("bind");
        let sock = sock.to_path_buf();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let reply = if line.contains("\"Login\"") {
                WireReply::LoginResult {
                    success: false,
                    error: Some(String::new()),
                    device_id: None,
                }
            } else {
                WireReply::LogoutResult {
                    success: false,
                    error: Some(String::new()),
                }
            };
            let _ = write_line(&mut writer, &reply);
            // Keep the connection alive long enough for the client to read.
            thread::sleep(Duration::from_secs(2));
            let _ = sock;
        });
        thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn empty_login_error_becomes_default_remote_message() {
        let sock = test_sock_path("login-empty-error");
        spawn_empty_error_stub(&sock);

        let mut controller = ClientController::connect(&sock).expect("connect");
        let err = controller
            .login("user@example.com", "password", None)
            .expect_err("login should fail");

        match err {
            CoreError::Remote(message) => assert_eq!(message, "login failed"),
            other => panic!("expected CoreError::Remote, got {other:?}"),
        }
        let _ = std::fs::remove_file(&sock);
    }

    #[test]
    fn empty_logout_error_becomes_default_remote_message() {
        let sock = test_sock_path("logout-empty-error");
        spawn_empty_error_stub(&sock);

        let mut controller = ClientController::connect(&sock).expect("connect");
        let err = controller.logout().expect_err("logout should fail");

        match err {
            CoreError::Remote(message) => assert_eq!(message, "logout failed"),
            other => panic!("expected CoreError::Remote, got {other:?}"),
        }
        let _ = std::fs::remove_file(&sock);
    }
}
