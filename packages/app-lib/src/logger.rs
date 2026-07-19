/*
    tracing is set basd on the environment variable RUST_LOG=xxx, depending on the amount of logs to show
        ERROR > WARN > INFO > DEBUG > TRACE
    eg. RUST_LOG=info will show info, warn, and error logs
        RUST_LOG="theseus=trace" will show *all* messages but from theseus only (and not dependencies using similar crates)

    Error messages returned to Tauri will display as traced error logs if they return an error.
    This will also include an attached span trace if the error is from a tracing error, and the level is set to info, debug, or trace

    on unix:
        RUST_LOG="theseus=trace" {run command}

    The default is theseus=info, meaning only logs from theseus will be displayed, and at the info or higher level.

    Both debug and release builds write a timestamped `session_*.log` file to the
    launcher logs directory; debug builds additionally echo to the console.
*/

use tracing_subscriber::prelude::*;

/// Opens a fresh timestamped `session_*.log` file in the launcher logs directory,
/// creating the directory if needed.
fn open_session_log_file(app_identifier: &str) -> Option<std::fs::File> {
    use crate::prelude::DirectoryInfo;
    use chrono::Local;
    use std::fs::OpenOptions;

    let logs_dir = DirectoryInfo::launcher_logs_dir_path(app_identifier)?;
    if let Err(err) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("Could not create logs directory: {err}");
        return None;
    }

    let log_file_name =
        format!("session_{}.log", Local::now().format("%Y%m%d_%H%M%S"));
    let log_file_path = logs_dir.join(log_file_name);

    match OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_file_path)
    {
        Ok(file) => Some(file),
        Err(e) => {
            eprintln!("Could not open log file: {e}");
            None
        }
    }
}

fn env_filter(default: &str) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default))
}

// Development: log to the console AND to a session file (terminal scrollback is
// limited, so a persistent copy is always kept).
#[cfg(debug_assertions)]
pub fn start_logger(app_identifier: &str) -> Option<()> {
    use tracing_subscriber::fmt::time::ChronoLocal;

    let file_layer = open_session_log_file(app_identifier).map(|file| {
        tracing_subscriber::fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .with_timer(ChronoLocal::rfc_3339())
    });

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(file_layer)
        .with(env_filter("theseus=info,theseus_gui=info,cef_console=info"))
        .with(tracing_error::ErrorLayer::default())
        .init();
    Some(())
}

// Production: log to a session file in the logs directory (no console output).
#[cfg(not(debug_assertions))]
pub fn start_logger(app_identifier: &str) -> Option<()> {
    use tracing_subscriber::fmt::time::ChronoLocal;

    let file = open_session_log_file(app_identifier)?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file)
                .with_ansi(false)
                .with_timer(ChronoLocal::rfc_3339()),
        )
        .with(env_filter("theseus=info,cef_console=info"))
        .with(tracing_error::ErrorLayer::default())
        .init();

    Some(())
}
