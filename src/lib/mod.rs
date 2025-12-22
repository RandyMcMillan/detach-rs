//! This crate provides utilities for daemonizing Rust processes.
//!
//! # How to use `detach-rs` (the binary)
//!
//! The `detach-rs` binary (located at `src/bin/detach-rs.rs`) is a detached Rust background service
//! that can be controlled via command-line arguments.
//!
//! ## Command-Line Arguments:
//!
//! *   **`--detach`**:
//!     Run the process in the background. This is the default behavior.
//!
//! *   **`--no-detach`**:
//!     Run the process in the foreground, disabling daemonization.
//!
//! *   **`--tail`**:
//!     Enables log tailing. When used, the service will run in the foreground and
//!     output its logs directly to the console while also writing them to the log file.
//!
//! *   **`--log-file <PATH>`**:
//!     Specifies the path to the log file. Defaults to `./detach.log`.
//!     Example: `--log-file /var/log/my_service.log`
//!
//! *   **`-t, --timeout <SECONDS>`**:
//!     Sets a timeout (in seconds) after which the service will automatically terminate.
//!     This applies to both detached and non-detached modes.
//!     Example: `--timeout 60` (service will run for 60 seconds)
//!
//! *   **`-l, --logging <LEVEL>`**:
//!     Sets the logging level for the service.
//!     Supported levels: `error`, `warn`, `info`, `debug`, `trace`.
//!     Defaults to `info`.
//!     Example: `--logging debug`
//!
//! ## Examples:
//!
//! *   **Run in background with default settings:**
//!     ```bash
//!     ./target/release/detach-rs
//!     ```
//!
//! *   **Run in foreground with debug logging:**
//!     ```bash
//!     ./target/release/detach-rs --no-detach --logging debug
//!     ```
//!
//! *   **Run in background, log to a specific file, and terminate after 5 minutes:**
//!     ```bash
//!     ./target/release/detach-rs --log-file /tmp/my_daemon.log --timeout 300
//!     ```
//!
//! *   **Tail logs of a foreground service:**
//!     ```bash
//!     ./target/release/detach-rs --no-detach --tail
//!     ```
//!
//! Note: On non-Unix systems, daemonization is not supported, and `--detach` will be ignored.
use anyhow;
use std::path::PathBuf;

#[cfg(unix)]
use libc::{dup2, fork, setsid, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
#[cfg(unix)]
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Performs the double-fork routine to completely detach from the terminal session.
#[cfg(unix)]
pub fn daemonize<F>(log_path: &PathBuf, level: log::LevelFilter, timeout: Option<u64>, service_future: F) -> Result<(), anyhow::Error>
where
    F: std::future::Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    unsafe {
        // 1. First fork: Parent exits, child continues
        let pid = fork();
        if pid < 0 { return Err(anyhow::anyhow!("First fork failed")); }
        if pid > 0 { std::process::exit(0); }

        // 2. Create a new session to lose the controlling TTY
        if setsid() < 0 { return Err(anyhow::anyhow!("Failed to create new session")); }

        // 3. Second fork: Prevents the process from re-acquiring a TTY
        let pid = fork();
        if pid < 0 { return Err(anyhow::anyhow!("Second fork failed")); }
        if pid > 0 { std::process::exit(0); }

        // 4. Change working directory to root to avoid locking the mount point
        std::env::set_current_dir("/")?;

        // 5. Redirect standard I/O to /dev/null
        let dev_null = StdFile::open("/dev/null")?;
        let fd = dev_null.as_raw_fd();
        dup2(fd, STDIN_FILENO);
        dup2(fd, STDOUT_FILENO);
        dup2(fd, STDERR_FILENO);
    }

    // Setup file logging since we no longer have a stdout
    setup_logging(log_path, level)?;

    // IMPORTANT: Re-initialize tokio runtime AFTER daemonization
    // This prevents issues with forking a multi-threaded runtime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap(); // Panics if runtime cannot be built

    rt.block_on(async {
        use log::info; // Import info here as well
        use tokio::time::sleep;
        use std::time::Duration;

        info!("Daemon process started. PID: {}", std::process::id());

        if let Some(timeout_seconds) = timeout {
            info!("Setting timeout for {} seconds.", timeout_seconds);
            tokio::select! {
                _ = service_future => {
                    info!("Service future finished before timeout.");
                }
                _ = sleep(Duration::from_secs(timeout_seconds)) => {
                    info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
                }
            }
        } else {
            service_future.await.expect("Service future failed"); // Unwraps Result, will panic on error
        }

        info!("Daemon process shutting down.");
        std::process::exit(0);
    });
    // This part is unreachable as std::process::exit(0) is called above.
    // However, Rust requires a return type for all branches.
    unreachable!()
}

#[cfg(not(unix))]
pub fn daemonize<F>(_log_path: &PathBuf, _level: log::LevelFilter, _timeout: Option<u64>, _service_future: F) -> Result<(), anyhow::Error>
where
    F: std::future::Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    eprintln!("Daemonization is not supported on this operating system.");
    Ok(()) // Or return an error if you want to explicitly signal failure
}

#[cfg(unix)]
pub fn setup_logging(path: &PathBuf, level: log::LevelFilter) -> Result<(), anyhow::Error> {
    use log4rs::append::file::FileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;

    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}\n")))
        .build(path)?;

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(level))?;

    log4rs::init_config(config)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn setup_logging(_path: &PathBuf, _level: log::LevelFilter) -> Result<(), anyhow::Error> {
    eprintln!("File logging with log4rs is not supported on this operating system when daemonizing.");
    // For non-unix, if daemonize is called (which it won't be if cfg(not(unix)))
    // then we would rely on main to setup a console logger if not tailing.
    Ok(())
}
