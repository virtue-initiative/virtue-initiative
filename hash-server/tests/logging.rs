//! HASH-014 logging, checked through what a real server writes to the log.
//!
//! These tests live in their own test binary, so they run in their own process with a
//! global subscriber. A thread-local subscriber in a binary whose other tests run in
//! parallel can miss events, because `tracing` caches which callsites are enabled
//! across all threads.

mod common;

use std::sync::{Arc, Mutex, OnceLock};

use common::*;
use tokio::sync::MutexGuard;

/// A `tracing` writer that appends to a shared buffer.
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

static LOG: OnceLock<LogBuffer> = OnceLock::new();
static LOG_CAPTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Installs the global subscriber once and empties the log. Hold the guard for the
/// whole test, so the log only holds this test's lines.
async fn capture_logs() -> MutexGuard<'static, ()> {
    let guard = LOG_CAPTURE.lock().await;
    let buffer = LOG.get_or_init(|| {
        let buffer = LogBuffer::default();
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(buffer.clone())
            .init();
        buffer
    });
    buffer.0.lock().unwrap().clear();
    guard
}

fn captured_log() -> String {
    String::from_utf8(LOG.get().unwrap().0.lock().unwrap().clone()).unwrap()
}

// HASH-014: every request is logged at debug level with its method and path, and the
// request body never appears in the log.
#[tokio::test]
async fn requests_are_logged_at_debug_and_bodies_are_not() {
    let _capture = capture_logs().await;

    let s = spawn_server().await;
    // A hash whose hex and decimal forms are easy to look for.
    let marker = [0xC7u8; 32];
    assert_eq!(s.post(DEVICE_A, 4_242_424, 7, marker).await.status(), 201);
    assert_eq!(s.post(DEVICE_A, 1, 7, marker).await.status(), 409);
    s.get_devices(&format!("devices={DEVICE_A}")).await;
    s.delete_device(&format!("device={DEVICE_A}")).await;
    s.client.get(s.url("/")).send().await.unwrap();

    let log = captured_log();
    let debug_requests: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("DEBUG") && l.contains("hash_server") && l.contains("request"))
        .collect();
    assert!(
        debug_requests.len() >= 5,
        "expected one debug line per request, got:\n{log}"
    );
    for (method, path) in [
        ("POST", "/hash"),
        ("GET", "/hash"),
        ("DELETE", "/hash"),
        ("GET", "/"),
    ] {
        assert!(
            debug_requests
                .iter()
                .any(|l| l.contains(method) && l.ends_with(&format!("path={path}"))),
            "no debug line for {method} {path}:\n{log}"
        );
    }
    assert!(
        !log.contains(&hex::encode(marker)),
        "body hash leaked:\n{log}"
    );
    assert!(!log.contains("4242424"), "body unix_time leaked:\n{log}");
}

// HASH-014: expected errors (4xx) are not logged as internal errors.
#[tokio::test]
async fn client_errors_are_not_logged_at_error_level() {
    let _capture = capture_logs().await;

    let s = spawn_server().await;
    s.post(DEVICE_A, 1, 1, [1; 32]).await;
    s.post(DEVICE_A, 1, 1, [1; 32]).await;
    s.get_devices("devices=nope").await;
    s.client.post(s.url("/hash")).send().await.unwrap();

    let log = captured_log();
    // The capture works: the requests above were logged.
    assert!(
        log.lines()
            .any(|l| l.contains("POST") && l.ends_with("path=/hash")),
        "no request was captured:\n{log}"
    );
    assert!(!log.lines().any(|l| l.contains("ERROR")), "{log}");
}
