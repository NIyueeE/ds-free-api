//! 运行日志 —— 自定义 log::Log 实现，三路输出 + 文件轮转
//!
//! 三路：stderr（终端可见）+ 内存环形缓冲区（API 可查）+ 文件（持久化）
//! 文件轮转：单文件 10MB，保留 3 个历史文件，总上限 ~40MB

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::Arc;

use chrono::Local;
use serde::Serialize;
use tokio::sync::Mutex;

/// 环形缓冲区容量
const BUFFER_CAPACITY: usize = 2000;
/// 单个日志文件最大字节数（10MB）
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// 保留的历史日志文件数
const MAX_HISTORY_FILES: usize = 3;

/// 单条运行日志
#[derive(Serialize, Clone, Debug)]
pub struct RuntimeLogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 自定义 Logger
pub struct DualLogger {
    /// 内存环形缓冲区
    buffer: Mutex<VecDeque<RuntimeLogEntry>>,
    /// 当前日志文件（std::sync::Mutex 用于 log 路径的非阻塞写入）
    file: std::sync::Mutex<File>,
    /// 日志文件路径
    log_path: String,
    /// 最大日志级别
    max_level: log::LevelFilter,
}

impl std::fmt::Debug for DualLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DualLogger")
            .field("log_path", &self.log_path)
            .field("max_level", &self.max_level)
            .finish()
    }
}

impl DualLogger {
    fn new(log_path: &str, max_level: log::LevelFilter) -> Self {
        if let Some(parent) = std::path::Path::new(log_path).parent() {
            let _ = fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("无法打开日志文件");

        Self {
            buffer: Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
            file: std::sync::Mutex::new(file),
            log_path: log_path.to_string(),
            max_level,
        }
    }

    fn rotate_if_needed(&self) {
        let size = self.file.lock().ok().and_then(|f| f.metadata().ok().map(|m| m.len())).unwrap_or(0);
        if size < MAX_FILE_SIZE {
            return;
        }

        for i in (1..=MAX_HISTORY_FILES).rev() {
            let old = format!("{}.{}", self.log_path, i);
            if i == MAX_HISTORY_FILES {
                let _ = fs::remove_file(&old);
            } else {
                let new = format!("{}.{}", self.log_path, i + 1);
                let _ = fs::rename(&old, &new);
            }
        }
        let _ = fs::rename(&self.log_path, format!("{}.1", self.log_path));

        if let Ok(new_file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            if let Ok(mut file_guard) = self.file.lock() {
                *file_guard = new_file;
            }
        }
    }

    pub async fn query_logs(&self, offset: usize, limit: usize) -> (usize, Vec<RuntimeLogEntry>) {
        self.rotate_if_needed();
        let buffer = self.buffer.lock().await;
        let total = buffer.len();
        let logs: Vec<RuntimeLogEntry> = buffer
            .iter()
            .rev()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        (total, logs)
    }
}

impl log::Log for DualLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();
        let level = record.level().as_str().to_string();
        let target = record.target().to_string();
        let message = format!("{}", record.args());

        // 1. 写 stderr
        eprintln!("[{} {:5}  {}] {}", timestamp, level, target, message);

        // 2. 写文件
        let file_line = format!("[{} {:5}  {}] {}\n", timestamp, level, target, message);
        if let Ok(mut file_guard) = self.file.lock() {
            let _ = file_guard.write_all(file_line.as_bytes());
            let _ = file_guard.flush();
        }

        // 3. 写环形缓冲区（try_lock 避免阻塞 log 路径）
        let entry = RuntimeLogEntry {
            timestamp,
            level,
            target,
            message,
        };
        if let Ok(mut buffer) = self.buffer.try_lock() {
            if buffer.len() >= BUFFER_CAPACITY {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }
    }

    fn flush(&self) {
        if let Ok(mut file_guard) = self.file.lock() {
            let _ = file_guard.flush();
        }
    }
}

/// 全局 Logger 引用
static GLOBAL_LOGGER: std::sync::OnceLock<Arc<DualLogger>> = std::sync::OnceLock::new();

/// 初始化自定义 Logger，替换 env_logger
pub fn init(log_path: &str) {
    let max_level = match std::env::var("RUST_LOG") {
        Ok(ref v) if !v.is_empty() => parse_level(v),
        _ => log::LevelFilter::Info,
    };

    let logger = Arc::new(DualLogger::new(log_path, max_level));
    GLOBAL_LOGGER.set(logger.clone()).expect("Logger 已初始化");

    // Arc::into_inner 需要 Arc 引用计数为 1，但 GLOBAL_LOGGER 持有一份
    // 所以用 Box::new 包装 Arc clone
    let boxed: Box<dyn log::Log> = Box::new(LoggerWrapper { inner: logger });
    log::set_boxed_logger(boxed).expect("Logger 设置失败");
    log::set_max_level(max_level);
}

/// 包装 Arc<DualLogger> 实现 Log（因为 set_boxed_logger 需要 Box<dyn Log>）
struct LoggerWrapper {
    inner: Arc<DualLogger>,
}

impl log::Log for LoggerWrapper {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.inner.enabled(metadata)
    }
    fn log(&self, record: &log::Record) {
        self.inner.log(record)
    }
    fn flush(&self) {
        self.inner.flush()
    }
}

fn parse_level(s: &str) -> log::LevelFilter {
    let first = s.split(',').next().unwrap_or(s);
    let level = first.split('=').next().unwrap_or(first);
    match level.trim() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Info,
    }
}

/// 查询运行日志（分页，从最新往旧倒序）
pub async fn query_logs(offset: usize, limit: usize) -> (usize, Vec<RuntimeLogEntry>) {
    let logger = GLOBAL_LOGGER.get().expect("Logger 未初始化");
    logger.query_logs(offset, limit).await
}
