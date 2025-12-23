
use anyhow;
use clap::Parser;
use log::{info, warn};
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{timeout, Duration as TokioDuration};
#[cfg(unix)]
use libc::{kill, SIGINT};


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
    info!("Executing command: \"{}\"", cmd_str);



    let mut command = Command::new("sh")
        .arg("-c")
        .arg(&cmd_str)
        .spawn()?;

    let status_result = if let Some(seconds) = timeout_seconds {
        info!("Command will timeout after {} seconds.", seconds);
        match timeout(TokioDuration::from_secs(seconds), command.wait()).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to wait for command: {}", e)),
                        Err(_elapsed) => {
                warn!(
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
                    warn!("Cannot send SIGINT equivalent on non-Unix platforms. Proceeding to hard kill if necessary.");
                }


                tokio::time::sleep(TokioDuration::from_millis(2000)).await;


                if command.try_wait()?.is_none() {
                    warn!("Process did not exit after SIGINT. Sending SIGKILL.");
                    command.kill().await?;
                }
                command.wait().await?;
                info!("Command timed out after {} seconds.", seconds);
                return Ok(());
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
        setup_logging(log_path, level, false)?;
    }

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
                    info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
                }
            }
        } else {
            service_future.await.expect("Service future failed"); // Unwraps Result, will panic on error
        }

        info!("Daemon process shutting down.");
        tokio::time::sleep(TokioDuration::from_millis(100)).await;
        std::process::exit(0);
    });
    // This part is unreachable as std::process::exit(0) is called above.
    // However, Rust requires a return type for all branches.
    unreachable!()
}

#[cfg(not(unix))]
pub fn daemonize<F>(
    __log_path: &PathBuf, // Marked as unused
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
