//! Minimal file logging so a release build (stderr hidden) still leaves a trail.
//! Writes a daily-rotating log to `%LOCALAPPDATA%/QuotaPeek/logs` (or the platform
//! equivalent).

use tracing_subscriber::EnvFilter;

pub fn init() {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("QuotaPeek")
        .join("logs");
    let _ = std::fs::create_dir_all(&dir);

    let appender = tracing_appender::rolling::daily(&dir, "quotapeek.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    // The guard flushes on drop; keep it alive for the whole process.
    Box::leak(Box::new(guard));

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,quotapeek_lib=info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(writer)
        .try_init();
}
