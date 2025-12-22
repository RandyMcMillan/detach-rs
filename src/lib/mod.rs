use anyhow;
use std::path::PathBuf;
use log::LevelFilter;

#[cfg(unix)]
use libc::{dup2, fork, setsid, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
#[cfg(unix)]
use std::fs::File as StdFile;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Performs the double-fork routine to completely detach from the terminal session.
#[cfg(unix)]
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
        let dev_null = StdFile::open("/dev/null")?;
        let fd = dev_null.as_raw_fd();
        dup2(fd, STDIN_FILENO);
        dup2(fd, STDOUT_FILENO);
        dup2(fd, STDERR_FILENO);
    }

    // Setup file logging since we no longer have a stdout
    setup_logging(log_path)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn daemonize(_log_path: &PathBuf) -> Result<(), anyhow::Error> {
    eprintln!("Daemonization is not supported on this operating system.");
    Ok(()) // Or return an error if you want to explicitly signal failure
}

#[cfg(unix)]
pub fn setup_logging(path: &PathBuf, level: log::LevelFilter) -> Result<(), anyhow::Error> {
    use log4rs::append::file::FileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;

    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}\n")))
        .build(path)?;

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(level))?;

    log4rs::init_config(config)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn setup_logging(_path: &PathBuf) -> Result<(), anyhow::Error> {
    eprintln!("File logging with log4rs is not supported on this operating system when daemonizing.");
    // For non-unix, if daemonize is called (which it won't be if cfg(not(unix)))
    // then we would rely on main to setup a console logger if not tailing.
    Ok(())
}
