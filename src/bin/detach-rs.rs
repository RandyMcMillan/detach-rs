// This async function will contain the core service logic
async fn run_service_async(args: Args, log_file_path: PathBuf, log_level: LevelFilter) -> anyhow::Result<()> {
    // If not detaching, setup simple console logging or tailing
    if args.tail {
        // Setup a basic logger that will output to stderr as usual,
        // but also start a tailing process.
        env_logger::Builder::new()
            .filter_level(log_level)
            .init();
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
        env_logger::Builder::new()
            .filter_level(log_level)
            .init();
    }

    info!("Service started. PID: {}", std::process::id());

    // Simulated background task
    let mut count = 0;
    let main_loop_future = async {
        loop {
            info!("Service heartbeat #{}", count);
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            count += 1;

            if count > 100 { break; }
            info!("count: {}", count);
        }
    };

    if let Some(timeout_seconds) = args.timeout {
        info!("Setting timeout for {} seconds.", timeout_seconds);
        tokio::select! {
            _ = main_loop_future => {
                info!("Main loop finished before timeout.");
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_seconds)) => {
                info!("Timeout reached after {} seconds. Terminating service.", timeout_seconds);
            }
        }
    } else {
        main_loop_future.await;
    }

    info!("Service shutting down.");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let log_file_path = if args.log_file.is_relative() {
        std::env::current_dir()?.join(&args.log_file)
    } else {
        args.log_file.clone()
    };

    let log_level = args.logging.unwrap_or(LevelFilter::Info);

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
        // daemonize call will cause parent to exit. Child continues here.
        daemonize(&log_file_path, log_level, args.timeout)?;

        // IMPORTANT: Re-initialize tokio runtime AFTER daemonization
        // This prevents issues with forking a multi-threaded runtime.
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_service_async(args, log_file_path, log_level))
    } else {
        // If not detaching, just run the async service directly with a new tokio runtime
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_service_async(args, log_file_path, log_level))
    }
}
