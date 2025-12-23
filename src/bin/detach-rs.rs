#![allow(unused)]

use chrono::Local;
#[cfg(unix)]
use clap::{Parser, ValueEnum};
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, dup2, fork, setsid};
use log::{LevelFilter, debug, info, trace, warn};
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
// REMOVED: use std::process::Command;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use detach::Args;
use detach::daemonize;
use detach::run_command_and_exit;
use detach::run_service_async;
use detach::setup_logging;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Define the default log file path
    let default_log_file = PathBuf::from("./detach.log");

    let log_file_path = if args.log_file == default_log_file {
        // If the default log file is used, append a timestamp
        let now = Local::now();
        let timestamp_str = now.format("%Y%m%d-%H%M%S").to_string();
        let timestamped_filename = format!("detach-{}.log", timestamp_str);
        std::env::current_dir()?.join(timestamped_filename)
    } else if args.log_file.is_relative() {
        // If a custom relative path is provided, resolve it
        std::env::current_dir()?.join(&args.log_file)
    } else {
        // If an absolute path is provided, use it as-is
        args.log_file.clone()
    };

    let log_level = args.logging.unwrap_or(log::LevelFilter::Info);

    // Build the tokio runtime once
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        // Wrap the main logic in an async block
        // --- NEW LOGIC FOR --command FLAG ---
        if let Some(cmd_str) = args.command {
            match run_command_and_exit(cmd_str, &log_file_path, log_level, args.timeout).await {
                Ok(_) => std::process::exit(0),
                Err(e) => {
                    eprintln!("Command execution failed: {}", e);
                    std::process::exit(1); // Exit with a non-zero code for failure
                }
            }
        }
        // --- END NEW LOGIC ---

        debug!("debug");
        info!("info");
        trace!("trace");
        warn!("warn");

        let mut should_detach = args.detach && !args.no_detach && !args.tail;

        #[cfg(not(unix))]
        {
            if should_detach {
                eprintln!("Daemonization is not supported on this operating system.");
                should_detach = false;
            }
        }

        // Create the service future (heartbeat loop)
        let service_future = run_service_async();

        if should_detach {
            debug!("Detaching process... Check logs at {:?}", log_file_path);
            // daemonize will now handle tokio runtime, logging, and timeout
            daemonize(
                &log_file_path,
                log_level,
                args.timeout,
                service_future,
                false,
            )?; // false for to_console
            // daemonize does not return in the child process, it exits.
            // So, this part is only reached by the parent process, which then exits.
            Ok(())
        } else {
            // This else block already has a tokio runtime in place from before.
            // We need to move the logging setup from here outside to avoid double setup
            // when --command is used.
            // Logging setup and tailing logic (if args.tail)
            if args.tail {
                // Use setup_logging for file and console output
                setup_logging(&log_file_path, log_level, true)?;
                debug!("Tailing log file: {:?}", log_file_path);

                // Spawn tailing task
                let tail_log_file_path = log_file_path.clone();
                tokio::spawn(async move {
                    use std::time::Duration;
                    use tokio::fs::File;
                    use tokio::io::{AsyncBufReadExt, BufReader};
                    use tokio::time::sleep;

                    loop {
                        match File::open(&tail_log_file_path).await {
                            Ok(file) => {
                                let mut reader = BufReader::new(file);
                                let mut buffer = String::new();
                                let mut offset = 0;

                                loop {
                                    buffer.clear();
                                    let bytes_read =
                                        reader.read_line(&mut buffer).await.unwrap_or(0);

                                    if bytes_read == 0 {
                                        sleep(Duration::from_millis(500)).await;
                                    } else {
                                        debug!("{}", buffer);
                                        offset += bytes_read as u64;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "Error opening log file for tailing: {:?}. Retrying...",
                                    e
                                );
                                sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                });
            } else {
                // If not detaching and not tailing, just setup simple console logging
                setup_logging(&log_file_path, log_level, true)?;
            }

            debug!("Service started. PID: {}", std::process::id());

            // Run the async service directly
            service_future.await?;

            info!("Service shutting down.");
            Ok(())
        }
    }) // End of rt.block_on(async { ... })
}
