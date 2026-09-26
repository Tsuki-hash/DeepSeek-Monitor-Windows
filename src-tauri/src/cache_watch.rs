//! 缓存目录变更信号（Windows `ReadDirectoryChangesW`）。
//!
//! 同步 watcher 原先以固定节奏（1.5s→4s 退避）全量枚举缓存目录，目录大起来
//! 以后每轮的 `read_dir` + 逐文件 metadata 都是空转。本模块把「该扫了」的时机
//! 交给目录变更事件：WebView2 落盘一批缓存就立刻唤醒扫描（登录后数百毫秒内
//! 即可命中），空闲期间零 IO。
//!
//! 失败路径全部退化为「调用方按原节奏定时轮询」，不改变正确性语义：
//! - 目录打不开（还不存在 / 被重建中）：`spawn` 返回 `None`，调用方退回定时轮询；
//! - 监听中途失效（目录被重建等）：`is_alive` 变 `false`，调用方重新 `spawn`；
//! - 通知缓冲溢出丢事件：调用方的兜底轮询超时后总会再扫一轮。
//!
//! 与 DPAPI（见 `credentials.rs`）同理，不引入 `notify` 等新依赖：
//! `windows` crate 已是直接依赖，这里只是多开几个 feature。

use std::path::Path;

#[cfg(windows)]
pub use windows_impl::CacheChangeSignal;

#[cfg(windows)]
pub fn spawn(dir: &Path) -> Option<CacheChangeSignal> {
    windows_impl::spawn(dir)
}
/// 非 Windows：没有这套 Win32 API 语义，恒为 `None`，调用方走纯定时轮询。
#[cfg(not(windows))]
pub struct CacheChangeSignal;

#[cfg(not(windows))]
pub fn spawn(_dir: &Path) -> Option<CacheChangeSignal> {
    None
}

#[cfg(not(windows))]
impl CacheChangeSignal {
    pub fn wait(&mut self, _timeout: std::time::Duration) -> bool {
        false
    }

    pub fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE,
    };
    use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

    /// 单次变更通知的缓冲区。一次页面加载会落盘一批小文件，64KB 足以承接一批
    /// 完整通知；真溢出只会丢事件，由调用方的定时轮询兜底，不影响正确性。
    const NOTIFY_BUFFER_BYTES: usize = 64 * 1024;

    /// Drop 等线程收摊的上限。线程几乎总在 `WaitForMultipleObjects` 上被即时
    /// 唤醒，超时属于极端异常：宁可泄漏句柄也不悬垂释放。
    const SHUTDOWN_TIMEOUT_MS: u32 = 2000;

    /// `HANDLE` 与裸指针一样不带 `Send`。这里在所有权固定的前提下手动放行：
    /// stop/exited 句柄归信号结构体独占，change 句柄与目录文件归通知线程独占，
    /// 两边都只在各自线程里使用，没有跨线程并发调用。
    struct SendHandle(HANDLE);
    unsafe impl Send for SendHandle {}

    pub struct CacheChangeSignal {
        receiver: mpsc::Receiver<()>,
        /// 收摊信号与完成确认。两个事件句柄归本结构体所有，Drop 里等线程
        /// 确认退出后再关闭，避免「SetEvent 时句柄已被系统回收复用」。
        stop_event: HANDLE,
        exited_event: HANDLE,
        alive: Arc<AtomicBool>,
    }

    impl CacheChangeSignal {
        /// 等待目录变更信号：`true` = 收到（调用方应尽快扫描），
        /// `false` = 超时 / 线程已退出（调用方按定时轮询节奏处理）。
        pub fn wait(&mut self, timeout: Duration) -> bool {
            match self.receiver.recv_timeout(timeout) {
                Ok(()) => true,
                Err(mpsc::RecvTimeoutError::Timeout) => false,
                // 线程已退出（多半 `is_alive` 已变 false）：睡满整个间隔，
                // 避免调用方在下一个轮次复查存活前被空转
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(timeout);
                    false
                }
            }
        }

        /// 监听线程是否仍在工作；`false` 说明监听已失效（如目录被重建），
        /// 调用方应重新 `spawn`。
        pub fn is_alive(&self) -> bool {
            self.alive.load(Ordering::Acquire)
        }
    }

    impl Drop for CacheChangeSignal {
        fn drop(&mut self) {
            // 次序很重要：先发收摊信号，再等线程确认退出（它内部会取消挂起的
            // 读请求并等 IO 真正结束），最后才关闭本结构体持有的事件句柄。
            unsafe {
                let _ = SetEvent(self.stop_event);
                let _ = WaitForSingleObject(self.exited_event, SHUTDOWN_TIMEOUT_MS);
                let _ = CloseHandle(self.stop_event);
                let _ = CloseHandle(self.exited_event);
            }
        }
    }

    /// 监听一个目录的变更，返回信号接收端。
    ///
    /// 目录打不开（典型：首次登录时 WebView2 还没创建缓存目录）返回 `None`，
    /// 调用方应退回定时轮询，并在后续轮次里重试本函数。
    pub fn spawn(dir: &Path) -> Option<CacheChangeSignal> {
        // 打开目录句柄：只读 + 全共享，不干扰 WebView2 的正常读写；
        // FILE_FLAG_BACKUP_SEMANTICS 是打开目录的必要条件，
        // FILE_FLAG_OVERLAPPED 让 ReadDirectoryChangesW 异步化、由事件驱动。
        let dir_file = OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2 | 0x4)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OVERLAPPED.0)
            .open(dir)
            .ok()?;

        // change：auto-reset，IO 完成时内核置位；stop：auto-reset，Drop 置位收摊；
        // exited：manual-reset，线程退出前置位，Drop 据此确认线程已收摊。
        // 部分创建失败时逐个补关，不留悬垂句柄（CreateEventW 失败极罕见）。
        let change_event = match unsafe { CreateEventW(None, false, false, None) } {
            Ok(handle) => handle,
            Err(_) => return None,
        };
        let stop_event = match unsafe { CreateEventW(None, false, false, None) } {
            Ok(handle) => handle,
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(change_event);
                }
                return None;
            }
        };
        let exited_event = match unsafe { CreateEventW(None, true, false, None) } {
            Ok(handle) => handle,
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(change_event);
                    let _ = CloseHandle(stop_event);
                }
                return None;
            }
        };

        let (sender, receiver) = mpsc::channel::<()>();
        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = Arc::clone(&alive);

        // 先在闭包外包好 SendHandle：闭包按值捕获裸 HANDLE 会让整个闭包 !Send。
        // 信号结构体自留的两个句柄值先取出（HANDLE 是 Copy），再整体移交线程。
        let change_handle = SendHandle(change_event);
        let stop_handle = SendHandle(stop_event);
        let exited_handle = SendHandle(exited_event);
        let signal_stop = stop_handle.0;
        let signal_exited = exited_handle.0;

        // std::thread::spawn 失败即 panic（OOM 级异常），无需在此处理失败分支
        thread::spawn(move || {
            watch_loop(
                dir_file,
                change_handle,
                stop_handle,
                exited_handle,
                sender,
                thread_alive,
            );
        });

        Some(CacheChangeSignal {
            receiver,
            stop_event: signal_stop,
            exited_event: signal_exited,
            alive,
        })
    }

    /// 通知线程主体：反复发出异步的目录变更读请求，change 事件置位就向调用方
    /// 发一个信号；stop 事件置位（信号结构体被 Drop）或出错时收摊。
    fn watch_loop(
        dir_file: File,
        change_event: SendHandle,
        stop_event: SendHandle,
        exited_event: SendHandle,
        sender: mpsc::Sender<()>,
        alive: Arc<AtomicBool>,
    ) {
        let dir_handle = HANDLE(dir_file.as_raw_handle());
        let mut buffer = vec![0u8; NOTIFY_BUFFER_BYTES];
        // OVERLAPPED 含 union 字段无法字面量构造，先零初始化（全零是合法的
        // 未发起状态）再补 hEvent。它与 buffer 必须活得比挂起的读请求久，都放在
        // 本函数栈/堆上，收摊前先取消 IO 并等它真正结束，保证没有悬垂写入。
        let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
        overlapped.hEvent = change_event.0;

        let notify_filter =
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;

        let mut read_pending = false;
        loop {
            if !read_pending {
                let issued = unsafe {
                    ReadDirectoryChangesW(
                        dir_handle,
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as u32,
                        false, // 只关注本层文件，与缓存扫描的范围一致
                        notify_filter,
                        None,
                        Some(&mut overlapped),
                        None,
                    )
                };
                match issued {
                    Ok(()) => read_pending = true,
                    // 目录句柄失效（被删除/重建）：退出，调用方按 is_alive 重拉
                    Err(_) => break,
                }
            }

            let handles = [change_event.0, stop_event.0];
            let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) }.0;
            if wait == WAIT_OBJECT_0.0 {
                // 一批变更到达，读请求已完成：发信号后重新排队下一发
                read_pending = false;
                if sender.send(()).is_err() {
                    // 接收端已释放（信号结构体先于线程消亡的兜底路径）：收摊
                    break;
                }
            } else if wait == WAIT_OBJECT_0.0 + 1 {
                break; // stop 事件：正常收摊
            } else {
                break; // 等待异常失败：退回调用方的定时轮询
            }
        }

        unsafe {
            if read_pending {
                let _ = CancelIo(dir_handle);
                let mut transferred = 0u32;
                let _ = GetOverlappedResult(dir_handle, &overlapped, &mut transferred, true);
            }
            let _ = CloseHandle(change_event.0);
        }
        // stop_event / exited_event 归信号结构体，这里不关；目录句柄由 dir_file
        // 的 Drop 关闭。
        alive.store(false, Ordering::Release);
        let _ = unsafe { SetEvent(exited_event.0) };
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn 目录不存在时返回_none() {
            let missing = std::env::temp_dir().join("dsm-watch-no-such-dir");
            assert!(spawn(&missing).is_none());
        }

        #[test]
        fn 目录变更触发信号_无变更则超时() {
            let dir = std::env::temp_dir().join(format!("dsm-watch-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("创建测试目录");

            let mut signal = spawn(&dir).expect("监听应建立");
            // 目录静默：短窗口内不应有信号
            assert!(!signal.wait(Duration::from_millis(300)));
            assert!(signal.is_alive());

            // 落盘一个文件：应在宽限窗口内收到变更信号
            std::fs::write(dir.join("f_000001"), b"probe").expect("写入测试文件");
            assert!(
                signal.wait(Duration::from_secs(5)),
                "写入后 5 秒内应收到变更信号"
            );

            // Drop 走收摊握手，不应挂起；随后目录可被清理
            drop(signal);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
