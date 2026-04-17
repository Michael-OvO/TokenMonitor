use chrono::Utc;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

type ReloadHandle = reload::Handle<EnvFilter, Registry>;

pub struct LoggingState {
    reload_handle: ReloadHandle,
    frontend_writer: Mutex<RollingFileAppender>,
    current_level: Mutex<String>,
    pub log_dir: PathBuf,
    _backend_guard: WorkerGuard,
}

impl LoggingState {
    pub fn set_level(&self, level: &str) -> Result<(), String> {
        let new_filter =
            EnvFilter::try_new(level).map_err(|e| format!("Invalid log level '{level}': {e}"))?;
        self.reload_handle
            .reload(new_filter)
            .map_err(|e| format!("Failed to reload log filter: {e}"))?;

        if let Ok(mut current_level) = self.current_level.lock() {
            *current_level = normalize_log_level(level).to_string();
        }

        Ok(())
    }

    pub fn write_frontend_log(&self, level: &str, category: &str, message: &str) {
        let line = format!(
            "{:>5} [frontend:{}] {}\n",
            level.to_uppercase(),
            category,
            message
        );
        if let Ok(mut writer) = self.frontend_writer.lock() {
            if let Err(e) = writer.write_all(line.as_bytes()) {
                tracing::warn!("Failed to write frontend log: {e}");
            }
            if let Err(e) = writer.flush() {
                tracing::warn!("Failed to flush frontend log: {e}");
            }
        }
    }

    pub fn get_level(&self) -> String {
        self.current_level
            .lock()
            .map(|level| level.clone())
            .unwrap_or_else(|_| String::from("info"))
    }
}

pub fn init_logging(app_data_dir: &Path) -> LoggingState {
    let log_dir = app_data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).ok();

    // Backend: tracing subscriber with daily-rotating file appender
    let backend_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "backend.log");
    let (non_blocking, backend_guard) = tracing_appender::non_blocking(backend_appender);

    // Frontend: separate daily-rotating file appender (written to directly via IPC)
    let frontend_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "frontend.log");

    // Reloadable EnvFilter for runtime level switching
    let default_filter_spec =
        std::env::var("TOKENMONITOR_LOG").unwrap_or_else(|_| String::from("info"));
    let default_filter =
        EnvFilter::try_new(&default_filter_spec).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload_handle) = reload::Layer::new(default_filter);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(false)
                .with_writer(non_blocking),
        )
        .init();

    cleanup_old_logs(&log_dir, 7);

    tracing::info!(log_dir = %log_dir.display(), "Logging initialized");

    LoggingState {
        reload_handle,
        frontend_writer: Mutex::new(frontend_appender),
        current_level: Mutex::new(normalize_log_level(&default_filter_spec).to_string()),
        log_dir,
        _backend_guard: backend_guard,
    }
}

fn normalize_log_level(level: &str) -> &'static str {
    let level = level.trim().to_ascii_lowercase();

    if level.contains("debug") || level.contains("trace") {
        "debug"
    } else if level.contains("warn") {
        "warn"
    } else if level.contains("error") {
        "error"
    } else {
        "info"
    }
}

/// Remove log files older than `max_days` from the log directory.
fn cleanup_old_logs(log_dir: &Path, max_days: i64) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };
    let cutoff = Utc::now() - chrono::Duration::days(max_days);

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "log") {
            // Also match files like "backend.log.2026-03-29"
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.contains(".log") {
                continue;
            }
        }
        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                let modified: chrono::DateTime<Utc> = modified.into();
                if modified < cutoff {
                    tracing::debug!(file = %path.display(), "Removing old log file");
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!(file = %path.display(), "Failed to remove old log: {e}");
                    }
                }
            }
        }
    }
}
