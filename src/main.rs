#![allow(unused)]
use clap::Parser;
use libc::{dup2, fork, setsid, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use log::info;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use detach::daemonize;

#[derive(Parser, Debug)]
#[command(author, version, about = "A detached Rust background service")]
struct Args {
    /// Run the process in the background
    #[arg(long, default_value_t = true)]
    detach: bool,

    /// Run the process in the foreground (disable detachment)
    #[arg(long = "no-detach")]
    no_detach: bool,

    /// tail logging
    #[arg(long, short, default_value_t = true)]
    tail: bool,

    /// Path to the log file
    //TODO handle canonical relative path
    #[arg(short, long, default_value = "./detach.log")]
    log_file: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let log_file_path = if args.log_file.is_relative() {
        std::env::current_dir()?.join(&args.log_file)
    } else {
        args.log_file.clone()
    };

    if args.detach && !args.no_detach {
        println!("Detaching process... Check logs at {:?}", log_file_path);
        daemonize(&log_file_path)?;
    } else {
        // If not detaching, just setup simple console logging
        env_logger::init();
    }

    info!("Service started. PID: {}", std::process::id());

    // Simulated background task
    let mut count = 0;
    loop {
        info!("Service heartbeat #{}", count);
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        count += 1;
        
        if count > 100 { break; }
        info!("count: {}", count);
    }

    info!("Service shutting down.");
    Ok(())
}
