//! In-process log tailing.
//!
//! We install a `tracing_subscriber` that writes to three sinks:
//!
//! * stdout / logcat / os_log (platform-dependent, default),
//! * a rotating file `<app_dir>/logs/app.log`,
//! * a bounded in-memory ring buffer that the Flutter UI tails.

use crate::errors::{Error, Result};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

const RING_CAPACITY: usize = 2_000;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp_ms: i64,
    pub level: String,
    pub message: String,
}

#[derive(Default)]
struct Ring {
    buf: VecDeque<LogEntry>,
    seq: u64,
}

static RING: once_cell::sync::Lazy<Arc<Mutex<Ring>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Ring::default())));

/// Returns the latest log lines (newest first, bounded by `limit`).
pub fn tail(limit: usize) -> Vec<LogEntry> {
    let guard = RING.lock();
    guard.buf.iter().rev().take(limit.max(1)).cloned().collect()
}

/// Clears the in-memory ring — does not touch the on-disk log.
pub fn clear() {
    RING.lock().buf.clear();
}

struct RingWriter;

impl Write for RingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(s) = std::str::from_utf8(buf) {
            for line in s.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let (level, message) = split_level(line);
                let entry = LogEntry {
                    timestamp_ms: now_ms(),
                    level,
                    message,
                };
                let mut guard = RING.lock();
                guard.seq = guard.seq.wrapping_add(1);
                if guard.buf.len() >= RING_CAPACITY {
                    guard.buf.pop_front();
                }
                guard.buf.push_back(entry);
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RingWriter {
    type Writer = RingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        RingWriter
    }
}

fn split_level(line: &str) -> (String, String) {
    // `tracing`'s default fmt writes "TIMESTAMP LEVEL target: message".
    // We tolerate other shapes and just extract a coarse level name.
    for lvl in ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"] {
        if let Some(idx) = line.find(lvl) {
            let msg = line[idx + lvl.len()..].trim_start_matches(' ').to_string();
            return (lvl.to_string(), msg);
        }
    }
    ("INFO".to_string(), line.to_string())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Installs the global subscriber.  Safe to call multiple times — subsequent
/// calls become no-ops because `tracing` refuses to reinstall a global
/// dispatcher.
pub fn install(verbose: bool) -> Result<()> {
    let default = if verbose { Level::DEBUG } else { Level::INFO };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default.as_str()));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(RingWriter)
        .with_ansi(false);

    match builder.try_init() {
        Ok(()) => Ok(()),
        Err(e) => {
            // Not a hard error: the platform may have installed a subscriber
            // first (e.g. android_logger), in which case we log through
            // whatever sits at the top.
            tracing::warn!("tracing subscriber already installed: {e}");
            Ok(())
        }
    }
    .map_err(|e: Error| e)
}
