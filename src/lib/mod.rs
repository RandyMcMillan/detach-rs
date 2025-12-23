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
use clap::Parser;
use log::{info, warn}; // Added warn for timeout message
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{timeout, Duration as TokioDuration};

#[derive(Parser, Debug)]
#[command(author, version, about = "A detached Rust background service")]
pub struct Args {
    /// Run the process in the background
    #[arg(long, default_value_t = false)]
    pub detach: bool,

    /// Run the process in the foreground (disable detachment)
    #[arg(long = "no-detach")]
    pub no_detach: bool,

    /// tail logging
    #[arg(long, default_value_t = false, conflicts_with = "detach")]
    pub tail: bool,

    /// Path to the log file
    //TODO handle canonical relative path
    #[arg(long, default_value = "./detach.log")]
    pub log_file: PathBuf,

    /// Timeout after a specified number of seconds
    #[arg(long, short, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// Set the logging level (e.g., "error", "warn", "info", "debug", "trace")
    #[arg(long, short, value_name = "LEVEL", value_enum)]
    pub logging: Option<log::LevelFilter>,

    /// Command to run
    #[arg(long, value_name = "COMMAND", conflicts_with_all = ["detach", "tail"])]
    pub command: Option<String>,
}

#[cfg(unix)]
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, dup2, fork, setsid};
#[cfg(unix)]
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Executes a given command string and exits the process with the command's exit status.
///
/// This function sets up logging, executes the command using `sh -c`, and
/// then terminates the current process, returning the command's exit code.
///
/// # Arguments
/// - `cmd_str`: The command string to be executed (e.g., "ls -la", "echo hello | grep he").
/// - `log_file_path`: The path to the log file for setting up logging.
/// - `log_level`: The minimum log level to use for output.
///
/// # Returns
/// This function does not return `Result` in the traditional sense, as it
/// explicitly calls `std::process::exit()`. It returns `()` for compilation.
pub async fn run_command_and_exit(
    cmd_str: String,
    log_file_path: &PathBuf,
    log_level: log::LevelFilter,
    timeout_seconds: Option<u64>,
) -> anyhow::Result<()> {
    // Setup logging for the command execution (always to console for immediate feedback)
    setup_logging(log_file_path, log_level, true)?;

    info!("Executing command: \"{}\"", cmd_str);

    let mut command = Command::new("sh") // Use sh to allow complex commands
        .arg("-c")
        .arg(&cmd_str)
        .spawn()?; // Use spawn instead of status directly

    let status_result = if let Some(seconds) = timeout_seconds {
        info!("Command will timeout after {} seconds.", seconds);
        match timeout(TokioDuration::from_secs(seconds), command.wait()).await {
            Ok(Ok(status)) => Ok(status), // Command completed within timeout
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to wait for command: {}", e)), // Error waiting for command
                        Err(_elapsed) => { // Timeout occurred
                warn!(
                    "Command timed out after {} seconds. Killing process.",
                    seconds
                );
                command.kill().await?; // Kill the process
                command.wait().await?; // Wait for it to be killed
                return Err(anyhow::anyhow!("Command timed out.")); // Indicate timeout as an error
            }
        }
    } else {
        Ok(command.wait().await?)
    };

        let status_result_unwrapped = status_result?;

    

        if status_result_unwrapped.success() {

            info!("Command executed successfully.");

            Ok(())

        } else {

            let exit_code = status_result_unwrapped.code().unwrap_or(1);

            Err(anyhow::anyhow!("Command failed with exit code: {}", exit_code))

        }
}

/// Performs the double-fork routine to completely detach a process from its controlling terminal.
///
/// This function is specifically designed for Unix-like operating systems (`cfg(unix)`).
/// On non-Unix systems, it will print an error message and return immediately without performing
/// any daemonization.
///
/// The daemonization process involves a "double-fork" technique to ensure that the process
/// fully detaches from the controlling terminal, cannot reacquire one, and is not terminated
/// when the parent shell exits.
///
/// # Stages of Daemonization:
///
/// 1.  **First Fork**: The parent process forks, and the original parent immediately exits.
///     This ensures that the child process is not a process group leader and is adopted by `init` (PID 1).
///
/// 2.  **Create New Session (`setsid`)**: The child process creates a new session and becomes the
///     session leader. This detaches it from its controlling terminal.
///
/// 3.  **Second Fork**: The session leader forks again, and the session leader (first child) exits.
///     This ensures that the new child process is no longer a session leader, preventing it from
///     reacquiring a controlling terminal.
///
/// 4.  **Change Working Directory**: The process changes its current working directory to the root (`/`).
///     This is done to avoid keeping any mount points busy, which could prevent unmounting.
///
/// 5.  **Redirect Standard I/O**: Standard input, output, and error streams (`stdin`, `stdout`, `stderr`)
///     are redirected to `/dev/null`. This prevents the daemon from attempting to read from or
///     write to a terminal that no longer exists, and ensures it runs silently in the background.
///
/// # Asynchronous Execution and Timeout Management:
///
/// After successful daemonization, this function initializes a `tokio` multi-threaded runtime
/// within the child process. It then executes the provided `service_future` within this runtime.
///
/// -   **Logging**: Logging is set up to write to the specified `log_path` with the given `level`.
/// -   **Timeout**: If a `timeout` duration is provided, the function will use `tokio::select!`
///     to concurrently await either the completion of the `service_future` or the expiration of
///     the timeout. The process will terminate when the first of these events occurs.
/// -   **Process Termination**: The daemon process will explicitly call `std::process::exit(0)`
///     upon successful completion of the `service_future` or when the timeout is reached.
///
/// # Parameters:
///
/// -   `log_path`: A `PathBuf` indicating the file where the daemon's logs should be written.
/// -   `level`: A `log::LevelFilter` specifying the minimum level of log messages to record.
/// -   `timeout`: An `Option<u64>` representing the maximum duration (in seconds) the daemon
///     should run. If `Some(seconds)`, the daemon will terminate after `seconds`. If `None`,
///     it will run until the `service_future` completes.
/// -   `service_future`: An asynchronous future (`F`) that represents the main logic of the
///     daemon service. This future must implement `Future<Output = Result<(), anyhow::Error>> + Send + 'static`.
///     The daemon will execute this future and terminate upon its completion or timeout.
///
/// # Returns:
///
/// -   `Ok(())`: This function only returns `Ok(())` in the *original parent process* after the
///     first fork. The child process (daemon) does not return from this function; instead, it
///     executes the `service_future` and eventually calls `std::process::exit(0)`.
/// -   `Err(anyhow::Error)`: If any step of the daemonization process (forking, `setsid`, I/O redirection)
///     fails, an error is returned.
///
/// # Panics:
///
/// -   This function will panic if the `tokio` runtime cannot be built (e.g., due to system resource
///     limitations), or if the `service_future` itself panics.
/// -   If `service_future` returns an `Err`, `expect` will cause a panic.
///
/// # Safety:
///
/// This function uses `unsafe` blocks for `fork`, `setsid`, and `dup2` calls, which are POSIX
/// system calls. Care has been taken to ensure their correct usage for daemonization.
#[cfg(unix)]
pub fn daemonize<F>(
    log_path: &PathBuf,
    level: log::LevelFilter,
    timeout: Option<u64>,
    service_future: F,
    to_console: bool,
) -> Result<(), anyhow::Error>
where
    F: std::future::Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    unsafe {
        // 1. First fork: Parent exits, child continues
        let pid = fork();
        if pid < 0 {
            return Err(anyhow::anyhow!("First fork failed"));
        }
        if pid > 0 {
            std::process::exit(0);
        }

        // 2. Create a new session to lose the controlling TTY
        if setsid() < 0 {
            return Err(anyhow::anyhow!("Failed to create new session"));
        }

        // 3. Second fork: Prevents the process from re-acquiring a TTY
        let pid = fork();
        if pid < 0 {
            return Err(anyhow::anyhow!("Second fork failed"));
        }
        if pid > 0 {
            std::process::exit(0);
        }

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
    setup_logging(log_path, level, to_console)?;

    // IMPORTANT: Re-initialize tokio runtime AFTER daemonization
    // This prevents issues with forking a multi-threaded runtime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        use log::{debug, info, trace, warn};
        use tokio::time::sleep;


        debug!("Daemon process started. PID: {}", std::process::id());
        trace!("Daemon process started. PID: {}", std::process::id());
        warn!("Daemon process started. PID: {}", std::process::id());

        if let Some(timeout_seconds) = timeout {
            debug!("Setting timeout for {} seconds.", timeout_seconds);
            tokio::select! {
                _ = service_future => {
                    debug!("Service future finished before timeout.");
                }
                _ = sleep(TokioDuration::from_secs(timeout_seconds)) => { // Use TokioDuration here
                    debug!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
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
pub fn daemonize<F>(
    _log_path: &PathBuf,
    _level: log::LevelFilter,
    _timeout: Option<u64>,
    _service_future: F,
    _to_console: bool,
) -> Result<(), anyhow::Error>
where
    F: std::future::Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    eprintln!("Daemonization is not supported on this operating system.");
    Ok(()) // Or return an error if you want to explicitly signal failure
}

#[cfg(unix)]
pub fn setup_logging(
    path: &PathBuf,
    level: log::LevelFilter,
    to_console: bool,
) -> Result<(), anyhow::Error> {
    use log4rs::append::console::ConsoleAppender;
    use log4rs::append::file::FileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;

    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}\n")))
        .build(path)?;

    let mut config_builder = Config::builder();
    let mut root_builder = Root::builder();

    config_builder =
        config_builder.appender(Appender::builder().build("logfile", Box::new(logfile)));
    root_builder = root_builder.appender("logfile");

    if to_console {
        let stdout = ConsoleAppender::builder()
            .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}\n")))
            .build();
        config_builder =
            config_builder.appender(Appender::builder().build("stdout", Box::new(stdout)));
        root_builder = root_builder.appender("stdout");
    }

    let config = config_builder.build(root_builder.build(level))?;

    log4rs::init_config(config)?;
    Ok(())
}

#[cfg(not(unix))]

pub fn setup_logging(
    _path: &PathBuf,
    _level: log::LevelFilter,
    _to_console: bool,
) -> Result<(), anyhow::Error> {
    eprintln!(
        "File logging with log4rs is not supported on this operating system when daemonizing."
    );
    // For non-unix, if daemonize is called (which it won't be if cfg(not(unix)))
    // then we would rely on main to setup a console logger if not tailing.
    Ok(())
}

/// A default asynchronous service future that simulates a background task with heartbeats.
///
/// This function can be used as the `service_future` parameter for `daemonize` to create
/// a simple detached service that logs its heartbeat every 10 seconds and terminates
/// after 100 heartbeats.
///
/// # Returns
///
/// - `Ok(())`: If the service completes its simulated task.
/// - `Err(anyhow::Error)`: If an error occurs during its execution.
pub async fn run_service_async() -> anyhow::Result<()> {
    use log::debug;
    let mut count = 0;
    loop {
        debug!("Service heartbeat #{}", count);
        tokio::time::sleep(TokioDuration::from_secs(10)).await;
        count += 1;

        if count > 100 {
            break;
        }
        debug!("count: {}", count);
    }
    info!("Service shutting down.");
    Ok(())
}
