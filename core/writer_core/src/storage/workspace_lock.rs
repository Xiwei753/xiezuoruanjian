//! 跨进程工作区写锁。
//!
//! 桌面端与本地 MCP 服务可能各自持有 Core API。所有星图/注释写入口在
//! 修改磁盘前获取同一个工作区锁，保证同一时刻只有一个进程执行
//! read-modify-write。锁文件位于应用数据根目录，`.lock` 后缀不会进入
//! 同步/历史。

use std::cell::{Cell, RefCell};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use fs2::FileExt;

use crate::error::Result;

static PROCESS_LOCK: Mutex<()> = Mutex::new(());
const LOCK_FILE: &str = "workspace.write.lock";

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    static LOCK_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// RAII 工作区写锁。同一线程嵌套获取时复用最外层锁，避免自锁。
pub struct WorkspaceWriteLock {
    _process_guard: Option<MutexGuard<'static, ()>>,
    file: Option<File>,
    /// 最外层 guard 对应的应用数据根；嵌套 guard 必须指向同一根。
    root: Option<PathBuf>,
}

impl WorkspaceWriteLock {
    /// 获取工作区写锁，不做任何目录扫描或事务恢复。
    ///
    /// 恢复只在工作区启动/打开时显式执行一次（[`Self::acquire_and_recover`]），
    /// 避免每次编辑取锁都遍历 contribution/journal 目录。
    pub fn acquire(app_data_root: &Path) -> Result<Self> {
        let root =
            std::fs::canonicalize(app_data_root).unwrap_or_else(|_| app_data_root.to_path_buf());
        let nested = DEPTH.with(|depth| {
            let value = depth.get();
            depth.set(value + 1);
            value > 0
        });
        if nested {
            let same_root = LOCK_ROOT.with(|current| {
                current
                    .borrow()
                    .as_deref()
                    .is_some_and(|path| path == root.as_path())
            });
            if !same_root {
                DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
                return Err(crate::error::Error::Other(format!(
                    "nested workspace write lock for a different app data root: {}",
                    root.display()
                )));
            }
            return Ok(Self {
                _process_guard: None,
                file: None,
                root: Some(root),
            });
        }

        let process_guard = match PROCESS_LOCK.lock() {
            Ok(guard) => guard,
            Err(_) => {
                DEPTH.with(|depth| depth.set(0));
                return Err(crate::error::Error::Other(
                    "workspace write lock poisoned".into(),
                ));
            }
        };
        let file = match open_lock_file(app_data_root) {
            Ok(file) => file,
            Err(error) => {
                DEPTH.with(|depth| depth.set(0));
                return Err(error);
            }
        };
        if let Err(error) = file.lock_exclusive() {
            DEPTH.with(|depth| depth.set(0));
            return Err(error.into());
        }
        LOCK_ROOT.with(|current| *current.borrow_mut() = Some(root.clone()));
        Ok(Self {
            _process_guard: Some(process_guard),
            file: Some(file),
            root: Some(root),
        })
    }

    /// 获取写锁并完成一次启动期恢复。仅供 bootstrap/工作区打开使用。
    pub fn acquire_and_recover(app_data_root: &Path) -> Result<Self> {
        let lock = Self::acquire(app_data_root)?;
        crate::writing_stats::contribution_transaction::recover_pending(app_data_root)?;
        Ok(lock)
    }
}

fn open_lock_file(app_data_root: &Path) -> Result<File> {
    std::fs::create_dir_all(app_data_root)?;
    let path = app_data_root.join(LOCK_FILE);
    Ok(OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?)
}

impl Drop for WorkspaceWriteLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
        }
        DEPTH.with(|depth| {
            let value = depth.get();
            depth.set(value.saturating_sub(1));
            if value <= 1 {
                LOCK_ROOT.with(|current| *current.borrow_mut() = None);
            }
        });
        drop(self._process_guard.take());
    }
}
