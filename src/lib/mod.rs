use anyhow;
use clap::Parser;
// Replaced log imports

// New tracing imports
use tracing_subscriber::{self, fmt::writer::MakeWriterExt, prelude::*};
//use tracing_log::LogTracer;
// Or just use tracing::Level if it compiles

// A wrapper for initializing tracing-subscriber (Robust version)
pub fn setup_tracing_logging(
    path: &PathBuf,
    level: log::LevelFilter,
    to_console: bool,
) -> anyhow::Result<()> {
    // Convert log::LevelFilter to log::Level, then to tracing::Level
    let converted_log_level = level.to_level().unwrap_or(log::Level::Info);
    let converted_tracing_level = map_log_level_to_tracing_level(converted_log_level);

    let file = std::fs::File::create(path)?;
    let file_appender = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file);

    let filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(
            tracing_subscriber::filter::LevelFilter::from_level(converted_tracing_level).into(),
        )
        .from_env_lossy(); // Removed .build()?

    // Initialize the registry based on to_console
    let init_result = if to_console {
        let console_appender = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_writer(std::io::stdout.with_max_level(converted_tracing_level));
        tracing_subscriber::registry()
            .with(file_appender)
            .with(console_appender)
            .with(filter) // Add the filter here
            .try_init() // Use try_init()
    } else {
        tracing_subscriber::registry()
            .with(file_appender)
            .with(filter) // Add the filter here
            .try_init() // Use try_init()
    };

    if let Err(e) = init_result {
        eprintln!("Warning: Failed to initialize tracing subscriber: {}", e);
        // Do not return early, as LogTracer might still need to be set up or another logger is active
    }

    // Route log messages through tracing
    //let log_tracer_init_result = LogTracer::builder()
    //    .with_max_level(level)
    //    .init();

    //if let Err(e) = log_tracer_init_result {
    //    eprintln!("Warning: Failed to initialize LogTracer: {}", e);
    //}

    Ok(())
}

// Helper function to map log::Level to tracing::Level
fn map_log_level_to_tracing_level(level: log::Level) -> tracing::Level {
    // Explicitly use tracing::Level here
    match level {
        log::Level::Error => tracing::Level::ERROR,
        log::Level::Warn => tracing::Level::WARN,
        log::Level::Info => tracing::Level::INFO,
        log::Level::Debug => tracing::Level::DEBUG,
        log::Level::Trace => tracing::Level::TRACE,
    }
}
#[cfg(unix)]
use libc::{SIGINT, kill};
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{Duration as TokioDuration, timeout};

#[derive(Parser, Debug)]
#[command(author, version, about = "A detached Rust background service")]
pub struct Args {
    #[arg(long, default_value_t = false)]
    pub detach: bool,

    #[arg(long = "no-detach")]
    pub no_detach: bool,

    #[arg(long, default_value_t = false, conflicts_with = "detach")]
    pub tail: bool,

    #[arg(long, default_value = "./detach.log")]
    pub log_file: PathBuf,

    #[arg(long, short, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    #[arg(long, short, value_name = "LEVEL", value_enum)]
    pub logging: Option<log::LevelFilter>,

    #[arg(long, value_name = "COMMAND", conflicts_with_all = ["detach", "tail"])]
    pub command: Option<String>,
}

#[cfg(unix)]
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, dup2, fork, setsid};
#[cfg(unix)]
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

pub async fn run_command_and_exit(
    cmd_str: String,
    _log_file_path: &PathBuf,
    _log_level: log::LevelFilter,
    timeout_seconds: Option<u64>,
) -> anyhow::Result<()> {
    tracing::info!("Executing command: \"{}\"", cmd_str);

    let mut command = Command::new("sh").arg("-c").arg(&cmd_str).spawn()?;

    let status_result = if let Some(seconds) = timeout_seconds {
        tracing::info!("Command will timeout after {} seconds.", seconds);
        match timeout(TokioDuration::from_secs(seconds), command.wait()).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to wait for command: {}", e)),
            Err(_elapsed) => {
                tracing::warn!(
                    "Command timed out after {} seconds. Attempting graceful shutdown (SIGINT).",
                    seconds
                );
                #[cfg(unix)]
                {
                    let pid = command.id().expect("Failed to get child process ID");
                    unsafe {
                        kill(pid as i32, SIGINT);
                    }
                }
                #[cfg(not(unix))] // For non-Unix, we can't send SIGINT directly.
                {
                    // On Windows, there isn't a direct equivalent to SIGINT for graceful shutdown
                    // through the standard library process API. The `command.kill()` will send a
                    // more forceful termination signal. For now, we'll just log and proceed to the
                    // hard kill if the process doesn't exit after the sleep.
                    tracing::warn!(
                        "Cannot send SIGINT equivalent on non-Unix platforms. Proceeding to hard kill if necessary."
                    );
                }

                tokio::time::sleep(TokioDuration::from_millis(2000)).await;

                if command.try_wait()?.is_none() {
                    tracing::warn!("Process did not exit after SIGINT. Sending SIGKILL.");
                    command.kill().await?;
                }
                command.wait().await?;
                tracing::info!("Command timed out after {} seconds.", seconds);
                return Ok(());
            }
        }
    } else {
        Ok(command.wait().await?)
    };

    let status_result_unwrapped = status_result?;

    if status_result_unwrapped.success() {
        tracing::info!("Command executed successfully.");

        Ok(())
    } else {
        let exit_code = status_result_unwrapped.code().unwrap_or(1);

        Err(anyhow::anyhow!(
            "Command failed with exit code: {}",
            exit_code
        ))
    }
}

#[cfg(unix)]
pub fn daemonize<F>(
    log_path: &PathBuf,
    level: log::LevelFilter,
    timeout: Option<u64>,
    service_future: F,
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
        // Re-initialize logging in the daemonized child process
        setup_tracing_logging(log_path, level, false)?; // Use passed level

        // Temporary direct write for debugging
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log_path) {
            writeln!(
                file,
                "DEBUG: Direct write from daemonize after logging setup."
            )
            .expect("Failed to write debug message directly.");
            file.flush()
                .expect("Failed to flush log file after direct write."); // Add flush
        }
    }

    // IMPORTANT: Re-initialize tokio runtime AFTER daemonization
    // This prevents issues with forking a multi-threaded runtime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {


        use tokio::time::sleep;


        tracing::debug!("Daemon process started. PID: {}", std::process::id());
        tracing::trace!("Daemon process started. PID: {}", std::process::id());
        tracing::warn!("Daemon process started. PID: {}", std::process::id());

        if let Some(timeout_seconds) = timeout {
            tracing::trace!("Before tokio::select! in daemonize. Timeout: {}s", timeout_seconds);
            tokio::select! {
                _ = service_future => {
                    tracing::debug!("Service future finished before timeout.");
                    tracing::debug!("Setting timeout for {} seconds.", timeout_seconds);
                }
                _ = sleep(TokioDuration::from_secs(timeout_seconds)) => {
                    tracing::trace!("Timeout branch taken in daemonize.");
                    // Attempt to log with tracing, but also ensure direct write
                    tracing::info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
                    // Direct write as a fallback to ensure the message is in the log file
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log_path) {
                        writeln!(file, "Timeout reached after {} seconds. Terminating service.", timeout_seconds).expect("Failed to write timeout message directly.");
                        file.flush().expect("Failed to flush log file after direct write."); // Add flush
                    }
                }
            }
            tracing::trace!("After tokio::select! in daemonize.");
        } else {
            service_future.await.expect("Service future failed");
        }

        tracing::info!("Daemon process shutting down.");
        tokio::time::sleep(TokioDuration::from_secs(1)).await;
        std::process::exit(0);
    });
    // This part is unreachable as std::process::exit(0) is called above.
    // However, Rust requires a return type for all branches.
    unreachable!()
}

#[cfg(not(unix))]
pub fn daemonize<F>(
    __log_path: &PathBuf,      // Marked as unused
    __level: log::LevelFilter, // Marked as unused
    _timeout: Option<u64>,
    _service_future: F,
) -> Result<(), anyhow::Error>
where
    F: std::future::Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    eprintln!("Daemonization is not supported on this operating system.");
    Ok(()) // Or return an error if you want to explicitly signal failure
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
    use std::env; // Import std::env
    let mut count = 0;
    let max_heartbeats = env::var("DETACH_TEST_HEARTBEATS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(100); // Default to 100 if env var not set or invalid

    loop {
        tracing::debug!("Service heartbeat #{}", count);
        tokio::time::sleep(TokioDuration::from_secs(10)).await;
        count += 1;

        if count >= max_heartbeats {
            // Changed to >= for clarity, though > 100 also works
            break;
        }
        tracing::debug!("count: {}", count);
    }
    tracing::info!("Service shutting down.");
    Ok(())
}
