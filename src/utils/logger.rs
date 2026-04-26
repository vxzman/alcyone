//! 日志模块
//! 
//! 提供多级别日志记录功能

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
    Success = 5,
}

impl LogLevel {
    fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "[DEBUG]",
            LogLevel::Info => "[INFO]",
            LogLevel::Warning => "[WARNING]",
            LogLevel::Error => "[ERROR]",
            LogLevel::Success => "[SUCCESS]",
        }
    }

    fn as_color(&self) -> &'static str {
        match self {
            LogLevel::Debug => "\x1b[90m",
            LogLevel::Info => "\x1b[36m",
            LogLevel::Warning => "\x1b[33m",
            LogLevel::Error => "\x1b[31m",
            LogLevel::Success => "\x1b[32m",
        }
    }
}

const COLOR_RESET: &str = "\x1b[0m";

/// 日志器
pub struct Logger {
    level: Mutex<LogLevel>,
    output: Mutex<LoggerOutput>,
}

enum LoggerOutput {
    Stdout(io::Stdout),
    File(File),
}

impl Write for LoggerOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            LoggerOutput::Stdout(stdout) => stdout.write(buf),
            LoggerOutput::File(file) => file.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            LoggerOutput::Stdout(stdout) => stdout.flush(),
            LoggerOutput::File(file) => file.flush(),
        }
    }
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// 初始化日志器
pub fn init(output: &str) -> anyhow::Result<()> {
    let logger = if output == "shell" || output.is_empty() {
        Logger {
            level: Mutex::new(LogLevel::Debug),
            output: Mutex::new(LoggerOutput::Stdout(io::stdout())),
        }
    } else {
        if let Some(parent) = Path::new(output).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(output)?;
        Logger {
            level: Mutex::new(LogLevel::Debug),
            output: Mutex::new(LoggerOutput::File(file)),
        }
    };

    LOGGER.set(logger).map_err(|_| anyhow::anyhow!("Logger already initialized"))?;
    Ok(())
}

fn get_logger() -> Option<&'static Logger> {
    LOGGER.get()
}

/// 内部日志函数
fn log_line(level: LogLevel, msg: &str) {
    let Some(logger) = get_logger() else {
        eprintln!("Logger not initialized: {}", msg);
        return;
    };

    let current_level = logger.level.lock().map(|l| *l).unwrap_or(LogLevel::Info);
    if level < current_level {
        return;
    }

    let color = level.as_color();
    let level_str = level.as_str();
    let log_msg = format!("{}{}{} {}\n", color, level_str, COLOR_RESET, msg);

    if let Ok(mut output) = logger.output.lock() {
        let _ = output.write_all(log_msg.as_bytes());
        let _ = output.flush();
    }
}

/// Debug 日志
#[inline]
pub fn debug(msg: &str) {
    log_line(LogLevel::Debug, msg);
}

/// Info 日志
#[inline]
pub fn info(msg: &str) {
    log_line(LogLevel::Info, msg);
}

/// Warning 日志
#[inline]
pub fn warning(msg: &str) {
    log_line(LogLevel::Warning, msg);
}

/// Error 日志
#[inline]
pub fn error(msg: &str) {
    log_line(LogLevel::Error, msg);
}

/// Success 日志
#[inline]
pub fn success(msg: &str) {
    log_line(LogLevel::Success, msg);
}
