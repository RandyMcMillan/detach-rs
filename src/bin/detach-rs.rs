#![allow(unused)]
use clap::Parser;
#[cfg(unix)]
use libc::{dup2, fork, setsid, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use log::info;
use std::fs::File as StdFile; // Rename to avoid conflict with tokio::fs::File
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::fs::File;

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
    #[arg(long, short, default_value_t = false)]
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

    let mut should_detach = args.detach && !args.no_detach && !args.tail;

    #[cfg(not(unix))]
    {
        if should_detach {
            eprintln!("Daemonization is not supported on this operating system.");
            should_detach = false;
        }
    }

    if should_detach {
        println!("Detaching process... Check logs at {:?}", log_file_path);
        daemonize(&log_file_path)?;
    } else {
        // If not detaching, setup simple console logging or tailing
        if args.tail {
            // Setup a basic logger that will output to stderr as usual,
            // but also start a tailing process.
            env_logger::init();
            println!("Tailing log file: {:?}", log_file_path);

            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                use tokio::fs::File;
                use tokio::time::sleep;
                use std::time::Duration;

                // Loop until the file is created.
                loop {
                    match File::open(&log_file_path).await {
                        Ok(file) => {
                            let mut reader = BufReader::new(file);
                            let mut buffer = String::new();
                            let mut offset = 0; // Keep track of the read offset

                            loop {
                                buffer.clear();
                                let bytes_read = reader.read_line(&mut buffer).await.unwrap_or(0);

                                if bytes_read == 0 {
                                    // End of file, wait and try again
                                    sleep(Duration::from_millis(500)).await;
                                } else {
                                    // New data, print it
                                    print!("{}", buffer);
                                    offset += bytes_read as u64;
                                }
                            }
                        },
                        Err(e) => {
                            eprintln!("Error opening log file for tailing: {:?}. Retrying...", e);
                            sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            });
        } else {
            // If not detaching and not tailing, just setup simple console logging
            env_logger::init();
        }
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
