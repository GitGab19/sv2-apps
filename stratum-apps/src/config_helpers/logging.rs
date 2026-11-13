use std::{
    backtrace::Backtrace,
    fs::OpenOptions,
    io::{self, IsTerminal},
    panic,
    path::Path,
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    EnvFilter, Registry,
    filter::{ParseError, Targets},
    fmt,
    prelude::*,
};

/// Initialize logging to stdout and optionally to a file.
///
/// If `log_file` is Some, logs will be written to both stdout and the file.
///
/// `RUST_LOG` takes `tracing` filter directives, applied on top of INFO: a bare level such as
/// `debug` replaces INFO everywhere, and a directive naming a target, such as
/// `translator_sv2::io_task=trace`, sets that target's level and leaves the rest at INFO. A value
/// that does not parse is reported once logging is up, and logging falls back to INFO.
pub fn init_logging(log_file: Option<&Path>) {
    let rust_log = std::env::var("RUST_LOG").unwrap_or_default();
    let (env_filter, dependency_caps, invalid_rust_log) = log_filters(&rust_log);
    let stdout_layer = fmt::layer()
        .with_writer(io::stdout)
        .with_ansi(io::stdout().is_terminal());

    let subscriber: Box<dyn tracing::Subscriber + Send + Sync> = match log_file {
        Some(path) => {
            // Log to both file and stdout
            let path = path.to_owned();
            // Open file only once, and not on every write.
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("Failed to open log file");
            let file_layer = fmt::layer().with_writer(file).with_ansi(false);
            Box::new(
                Registry::default()
                    .with(env_filter)
                    .with(dependency_caps)
                    .with(stdout_layer)
                    .with(file_layer),
            )
        }
        None => {
            // Log only to stdout
            Box::new(
                Registry::default()
                    .with(env_filter)
                    .with(dependency_caps)
                    .with(stdout_layer),
            )
        }
    };

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global subscriber");

    if let Some(e) = invalid_rust_log {
        tracing::warn!("RUST_LOG={rust_log:?} is not a valid log filter ({e}); logging at INFO");
    }

    // Set up a panic hook that records panic information and a backtrace
    // as tracing events, ensuring they are persisted in the log file.
    let default_panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let backtrace = Backtrace::force_capture();
        tracing::error!("panic: {panic_info}");
        tracing::error!("Backtrace: {backtrace}");
        default_panic_hook(panic_info);
    }));
}

// Builds the two filters every log line has to pass: the one `RUST_LOG` describes, laid over INFO,
// and the caps on dependencies that log too much below INFO. When `rust_log` does not parse, the
// first falls back to INFO and the parse error comes back with it.
fn log_filters(rust_log: &str) -> (EnvFilter, Targets, Option<ParseError>) {
    // A directive replaces any earlier one for the same target, so whatever `RUST_LOG` says
    // overrides the INFO in front of it, and whatever it leaves unsaid stays at INFO.
    let (env_filter, invalid_rust_log) = match EnvFilter::try_new(format!("info,{rust_log}")) {
        Ok(filter) => (filter, None),
        Err(e) => (EnvFilter::new("info"), Some(e)),
    };
    // Miner telemetry probes every address in its discovery ranges through asic-rs, which logs
    // each probe at DEBUG, as does `hyper_util` for every HTTP connection those probes open. Both
    // are held at INFO at most, whatever `RUST_LOG` asks for. Being a filter of its own rather
    // than directives in `env_filter`, this can only lower their level, never raise it above what
    // `RUST_LOG` sets. A target matches by prefix, so `asic_rs` also covers the `asic_rs_*` crates.
    //
    // Every other target passes up to the most verbose level `env_filter` enables rather than
    // TRACE, so this filter never lifts the global maximum level to TRACE: that maximum is the
    // cheap check every `trace!` and `debug!` makes before anything else.
    let dependency_caps = Targets::new()
        .with_default(env_filter.max_level_hint().unwrap_or(LevelFilter::TRACE))
        .with_target("hyper_util", LevelFilter::INFO)
        .with_target("asic_rs", LevelFilter::INFO);
    (env_filter, dependency_caps, invalid_rust_log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::{Level, enabled, subscriber::with_default};

    // Runs `check` with the filters `init_logging` builds from `rust_log` as the current
    // subscriber, so `enabled!` inside it answers as it would in a running app.
    fn with_filters(rust_log: &str, check: impl FnOnce()) {
        let (env_filter, dependency_caps, _) = log_filters(rust_log);
        with_default(
            Registry::default().with(env_filter).with(dependency_caps),
            check,
        );
    }

    #[test]
    fn unset_rust_log_logs_at_info() {
        with_filters("", || {
            assert!(enabled!(target: "translator_sv2", Level::INFO));
            assert!(!enabled!(target: "translator_sv2", Level::DEBUG));
        });
    }

    #[test]
    fn bare_level_applies_everywhere_but_the_capped_dependencies() {
        with_filters("trace", || {
            assert!(enabled!(target: "translator_sv2::io_task", Level::TRACE));
            assert!(enabled!(target: "hyper_util::client", Level::INFO));
            assert!(!enabled!(target: "hyper_util::client", Level::DEBUG));
            assert!(!enabled!(target: "asic_rs_makes", Level::DEBUG));
        });
    }

    #[test]
    fn caps_never_raise_a_dependency_above_rust_log() {
        with_filters("warn", || {
            assert!(!enabled!(target: "translator_sv2", Level::INFO));
            assert!(!enabled!(target: "hyper_util", Level::INFO));
            assert!(!enabled!(target: "asic_rs", Level::INFO));
            assert!(enabled!(target: "asic_rs", Level::WARN));
        });
    }

    #[test]
    fn target_directive_sets_that_target_and_keeps_the_rest_at_info() {
        for rust_log in [
            "info,translator_sv2::io_task=trace",
            "translator_sv2::io_task=trace",
        ] {
            with_filters(rust_log, || {
                assert!(enabled!(target: "translator_sv2::io_task", Level::TRACE));
                assert!(!enabled!(target: "translator_sv2::sv1", Level::DEBUG));
                assert!(enabled!(target: "stratum_apps::task_manager", Level::INFO));
                assert!(!enabled!(target: "stratum_apps::task_manager", Level::TRACE));
            });
        }
    }

    #[test]
    fn invalid_rust_log_falls_back_to_info_and_is_reported() {
        let (_, _, invalid_rust_log) = log_filters("translator_sv2=degub");
        assert!(invalid_rust_log.is_some());
        with_filters("translator_sv2=degub", || {
            assert!(enabled!(target: "translator_sv2", Level::INFO));
            assert!(!enabled!(target: "translator_sv2", Level::DEBUG));
        });
    }
}
