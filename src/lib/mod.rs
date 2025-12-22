use anyhow;
use libc::{dup2, fork, setsid, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

/// Performs the double-fork routine to completely detach from the terminal session.
pub fn daemonize(log_path: &PathBuf) -> Result<(), anyhow::Error> {
    unsafe {
        // 1. First fork: Parent exits, child continues
        let pid = fork();
        if pid < 0 { return Err(anyhow::anyhow!("First fork failed")); }
        if pid > 0 { std::process::exit(0); }

        // 2. Create a new session to lose the controlling TTY
        if setsid() < 0 { return Err(anyhow::anyhow!("Failed to create new session")); }

        // 3. Second fork: Prevents the process from re-acquiring a TTY
        let pid = fork();
        if pid < 0 { return Err(anyhow::anyhow!("Second fork failed")); }
        if pid > 0 { std::process::exit(0); }

        // 4. Change working directory to root to avoid locking the mount point
        std::env::set_current_dir("/")?;

        // 5. Redirect standard I/O to /dev/null
        let dev_null = File::open("/dev/null")?;
        let fd = dev_null.as_raw_fd();
        dup2(fd, STDIN_FILENO);
        dup2(fd, STDOUT_FILENO);
        dup2(fd, STDERR_FILENO);
    }
    
    // Setup file logging since we no longer have a stdout
    setup_logging(log_path)?;
    Ok(())
}

pub fn setup_logging(path: &PathBuf) -> Result<(), anyhow::Error> {
    use log4rs::append::file::FileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;

    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}\n")))
        .build(path)?;

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(log::LevelFilter::Info))?;

    log4rs::init_config(config)?;
    Ok(())
}
