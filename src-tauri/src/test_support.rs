//! 测试专用工具。
//!
//! 配置读写是并发安全的（多个 `#[cfg(test)]` 在同一进程内并行跑），而 `DSM_CONFIG_DIR`
//! 是进程级环境变量——若各测试各自设置，会互相看见对方的临时目录。这里用一把全局锁把
//! 「设置变量 + 创建目录 + 清理目录」串成临界区，确保同一时刻只有一个测试持有配置目录。

#![cfg(test)]

use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

fn config_dir_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 指向一个独立临时目录的守卫。持有期间 `DSM_CONFIG_DIR` 指向该目录，
/// 析构时还原环境变量并删除目录。
pub struct TempConfigDir {
    path: PathBuf,
    previous: Option<std::ffi::OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl TempConfigDir {
    pub fn new(tag: &str) -> Self {
        let guard = config_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("dsm-test-{tag}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("创建测试配置目录失败");
        let previous = std::env::var_os("DSM_CONFIG_DIR");
        std::env::set_var("DSM_CONFIG_DIR", &path);
        Self {
            path,
            previous,
            _guard: guard,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempConfigDir {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("DSM_CONFIG_DIR", value),
            None => std::env::remove_var("DSM_CONFIG_DIR"),
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl Deref for TempConfigDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
