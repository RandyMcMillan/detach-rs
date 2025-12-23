use tracing::{error, info, warn};
use tracing_subscriber::{self, filter::EnvFilter};

fn main() {
    // Set up tracing-subscriber for console output with Info level
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    // Test logging
    info!("Logger initialized on platform: {}", std::env::consts::OS);
    warn!("This is a cross-platform warning!");
    error!(
        "If you see this, the logger is working on {}!",
        std::env::consts::OS
    );
}
