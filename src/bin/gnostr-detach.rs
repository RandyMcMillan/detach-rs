#![allow(unused)]

use chrono::Local;
use clap::{Parser, ValueEnum};
#[cfg(unix)]
use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, dup2, fork, setsid};
use log::{LevelFilter, debug, info, trace, warn};
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
use detach::setup_logging;


use anyhow::Result;
use gnostr_relay::App;
use tracing::info as tracing_info;
use tracing_subscriber::{fmt, EnvFilter};

#[actix_web::main]
async fn stat_relay() -> Result<()> {
    let args = Args::parse();
    let filter = EnvFilter::new(args.logging.map(|l| l.as_str()).unwrap_or("info"));
    fmt().with_env_filter(filter).init();
    info!("Start relay server");

    let local_set = tokio::task::LocalSet::new();

    local_set
        .run_until(async move {
            let app_data = gnostr_relay::App::create(
                Some("config/gnostr.toml"),
                true,
                Some("NOSTR".to_owned()),
                None,
            )
            .map_err(anyhow::Error::from)?;
            app_data.web_server()?.await.map_err(anyhow::Error::from)
        })
        .await?;

    info!("Relay server shutdown");
    Ok(())
}



fn main() -> anyhow::Result<()> {
    let args = Args::parse();


    let default_log_file = PathBuf::from("./gnostr-detach.log");

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

    setup_logging(&log_file_path, log_level, to_console)?;


    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = rt.block_on(async {


            return match run_command_and_exit(String::from("gnostr"), &log_file_path, log_level, args.timeout).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };



        debug!("debug");
        info!("info");
        trace!("trace");
        warn!("warn");

        let mut should_detach = should_detach_initial;

        //#[cfg(not(unix))]
        //{
        //    if should_detach {
        //        eprintln!("Daemonization is not supported on this operating system.");
        //        should_detach = false;
        //    }
        //}


        let service_future = run_service_async();

        if should_detach {
            debug!("Detaching process... Check logs at {:?}", log_file_path);

            daemonize(
                &log_file_path,
                log_level,
                args.timeout,
                service_future,
            )?;
            Ok(())
        } else {

            debug!("Service started. PID: {}", std::process::id());

            // Run the async service directly, respecting timeout if present
            if let Some(timeout_seconds) = args.timeout {
                debug!("Setting timeout for {} seconds.", timeout_seconds);
                tokio::select! {
                    _ = service_future => {
                        debug!("Service future finished before timeout.");
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(timeout_seconds)) => {
                        info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
                    }
                }
            } else {
                service_future.await?;
            }

            info!("Service shutting down.");
            Ok(())
        }
    });
    result
}
