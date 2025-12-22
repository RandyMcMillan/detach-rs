// This async function will contain the core service logic
async fn run_service_async() -> anyhow::Result<()> {
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

    // Create the service future (heartbeat loop)
    let service_future = run_service_async();

    if should_detach {
        println!("Detaching process... Check logs at {:?}", log_file_path);
        // daemonize will now handle tokio runtime, logging, and timeout
        daemonize(&log_file_path, log_level, args.timeout, service_future)?;
        // daemonize does not return in the child process, it exits.
        // So, this part is only reached by the parent process, which then exits.
        Ok(())
    } else {
        // If not detaching, setup simple console logging or tailing
        if args.tail {
            env_logger::Builder::new()
                .filter_level(log_level)
                .init();
            println!("Tailing log file: {:?}", log_file_path);

            // Spawn tailing task
            let tail_log_file_path = log_file_path.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                use tokio::fs::File;
                use tokio::time::sleep;
                use std::time::Duration;

                loop {
                    match File::open(&tail_log_file_path).await {
                        Ok(file) => {
                            let mut reader = BufReader::new(file);
                            let mut buffer = String::new();
                            let mut offset = 0;

                            loop {
                                buffer.clear();
                                let bytes_read = reader.read_line(&mut buffer).await.unwrap_or(0);

                                if bytes_read == 0 {
                                    sleep(Duration::from_millis(500)).await;
                                } else {
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

        // Run the async service directly with a new tokio runtime
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(service_future)?;

        info!("Service shutting down.");
        Ok(())
    }
}
