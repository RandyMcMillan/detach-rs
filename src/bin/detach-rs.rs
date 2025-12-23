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

    let should_detach_initial = args.detach && !args.no_detach && !args.tail; // Determine this earlier

    // Determine `to_console` based on command, tail, or detach status
    let to_console = args.command.is_some() || args.tail || !should_detach_initial; // Log to console if command, tail, or not detaching

    setup_logging(&log_file_path, log_level, to_console)?; // SINGLE setup_logging call

    // Build the tokio runtime once
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = rt.block_on(async {
        // Wrap the main logic in an async block
        // --- NEW LOGIC FOR --command FLAG ---
        if let Some(cmd_str) = args.command {
            return match run_command_and_exit(cmd_str, &log_file_path, log_level, args.timeout).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
        }
        // --- END NEW LOGIC ---

        // These debug/info/trace/warn calls should be after setup_logging
        debug!("debug");
        info!("info");
        trace!("trace");
        warn!("warn");

        let mut should_detach = should_detach_initial; // Use the initial determination

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
                false, // to_console is false for daemonize
            )?;
            Ok(())
        } else {
            // All setup_logging calls removed from here
            debug!("Service started. PID: {}", std::process::id());

            // Run the async service directly
            service_future.await?;

            info!("Service shutting down.");
            Ok(())
        }
    }); // End of rt.block_on(async { ... })
    result // Main function returns the result of the async block
}
