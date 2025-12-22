use log::{error, info, warn};
use log4rs;

fn main() {
    // 1. Define a configuration programmatically (highly portable)
    // You can also use a YAML file, but this code-based approach
    // ensures no "file not found" errors during initial cross-platform testing.
    let stdout = log4rs::append::console::ConsoleAppender::builder().build();

    let config = log4rs::config::Config::builder()
        .appender(log4rs::config::Appender::builder().build("stdout", Box::new(stdout)))
        .build(
            log4rs::config::Root::builder()
                .appender("stdout")
                .build(log::LevelFilter::Info),
        )
        .unwrap();

    // 2. Initialize the logger
    let _handle = log4rs::init_config(config).unwrap();

    // 3. Test logging
    info!("Logger initialized on platform: {}", std::env::consts::OS);
    warn!("This is a cross-platform warning!");
    error!(
        "If you see this, the logger is working on {}!",
        std::env::consts::OS
    );
}
