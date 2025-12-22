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
    #[arg(long)]
    detach: bool,

    /// Path to the log file
    #[arg(short, long, default_value = "app.log")]
    log_file: PathBuf,
}





#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.detach {
        println!("Detaching process... Check logs at {:?}", args.log_file);
        daemonize(&args.log_file)?;
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
    }

    info!("Service shutting down.");
    Ok(())
}
