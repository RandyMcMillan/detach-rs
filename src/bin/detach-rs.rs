#![allow(unused)]

use clap::{Parser, ValueEnum};
use chrono::Local;
#[cfg(unix)]
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, dup2, fork, setsid};
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use detach::Args;
use detach::daemonize;
use detach::run_command_and_exit;
use detach::run_service_async;
use detach::setup_tracing_logging; // Changed from setup_logging
use tracing::{info, debug, trace, warn}; // Added direct tracing macros

fn main() -> anyhow::Result<()> {
    let args = Args::parse();


    let default_log_file = PathBuf::from("./detach.log");

    let log_file_path = if args.log_file == default_log_file {

        let now = Local::now();
        let timestamp_str = now.format("%Y%m%d-%H%M%S").to_string();
        let timestamped_filename = format!("detach-{}.log", timestamp_str);
        std::env::current_dir()?.join(timestamped_filename)
    } else if args.log_file.is_relative() {

        std::env::current_dir()?.join(&args.log_file)
    } else {

        args.log_file.clone()
    };

    let log_level = args.logging.unwrap_or(log::LevelFilter::Info);

    let should_detach_initial = args.detach && !args.no_detach && !args.tail;


    let to_console = args.command.is_some() || args.tail || !should_detach_initial;

    // Call the new tracing setup function conditionally
    if !should_detach_initial {
        setup_tracing_logging(&log_file_path, log_level, to_console)?;
    }


    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = rt.block_on(async {


        if let Some(cmd_str) = args.command {
            return match run_command_and_exit(cmd_str, &log_file_path, log_level, args.timeout).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
        }


        // Removed: debug!("debug"); info!("info"); trace!("trace"); warn!("warn");

        let mut should_detach = should_detach_initial;

        #[cfg(not(unix))]
        {
            if should_detach {
                eprintln!("Daemonization is not supported on this operating system.");
                should_detach = false;
            }
        }


        let service_future = run_service_async();

        if should_detach {
            tracing::debug!("Detaching process... Check logs at {:?}", log_file_path); // Changed

            daemonize(
                &log_file_path,
                log_level,
                args.timeout,
                service_future,
            )?;
            Ok(())
        } else {

            tracing::debug!("Service started. PID: {}", std::process::id()); // Changed

            // Run the async service directly, respecting timeout if present
            if let Some(timeout_seconds) = args.timeout {
                tracing::debug!("Setting timeout for {} seconds.", timeout_seconds); // Changed
                tokio::select! {
                    _ = service_future => {
                        tracing::debug!("Service future finished before timeout."); // Changed
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(timeout_seconds)) => {
                        tracing::info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds); // Changed
                    }
                }
            } else {
                service_future.await?;
            }

            tracing::info!("Service shutting down."); // Changed
            Ok(())
        }
    });
    result
}
