//! 持久 CLI 子进程的所有者线程。
//!
//! Linux 的 `PR_SET_PDEATHSIG`（bubblewrap `--die-with-parent` 依赖它）绑定
//! 的是创建子进程的**线程**，而不是整个进程。OpenCode/Codex 的启动发生在
//! 短命的 `agent-runtime-prompt` 线程里，如果直接在该线程 `spawn`，线程返回
//! 时内核会立刻杀掉 bwrap 和 CLI（表现为 SIGKILL）。
//!
//! 因此每个持久 CLI 实例都由一个独立的所有者线程执行 `spawn`，并一直存活到
//! 子进程退出、被 `wait` 回收为止。`RpcClient` 只使用所有者线程交回的管道和
//! 共享退出状态，不再直接持有或回收 `Child`。关闭、超时回收都只由这个线程
//! 执行，避免 reader EOF 与外部 close 同时 `kill`。

use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::rpc::CloseReason;

/// 等待所有者线程完成 `spawn` 并把管道交回调用方。
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// 素笺主动关闭时等待 stdin EOF 后子进程自行退出的时间。
pub const CLOSE_GRACE: Duration = Duration::from_secs(3);
/// 所有者线程轮询子进程状态的间隔，也用于及时响应关闭请求。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub type SharedStdin = Arc<Mutex<Option<ChildStdin>>>;

#[derive(Debug, Clone)]
pub struct ExitInfo {
    pub status: Option<ExitStatus>,
    /// 素笺显式请求结束的原因；`None` 表示对端自行退出。
    pub requested_close: Option<CloseReason>,
    /// 是否由所有者线程在宽限期后发送 SIGKILL 回收。
    pub forced: bool,
    /// 触发强制回收的原因。
    pub forced_reason: Option<CloseReason>,
}

struct RuntimeState {
    requested_close: Option<CloseReason>,
    exit: Option<ExitInfo>,
}

type SharedState = Arc<(Mutex<RuntimeState>, Condvar)>;

enum ControlMessage {
    /// 显式关闭：所有者线程关闭 stdin，等待协议退出，超时才回收。
    Close(CloseReason),
}

struct SupervisorInner {
    label: String,
    pid: u32,
    owner_thread_id: String,
    dispatch_thread_id: String,
    control: Sender<ControlMessage>,
    shared: SharedState,
}

/// 已启动实例的控制句柄。克隆只共享控制通道与退出状态，不复制进程。
#[derive(Clone)]
pub struct ProcessSupervisor {
    inner: Arc<SupervisorInner>,
}

impl ProcessSupervisor {
    pub fn pid(&self) -> u32 {
        self.inner.pid
    }

    /// CLI 进程的所有者线程；bwrap 的父线程死亡语义绑定在它身上。
    pub fn owner_thread_id(&self) -> &str {
        &self.inner.owner_thread_id
    }

    /// 发起本次 `spawn` 的线程，仅用于诊断父线程提前结束的问题。
    pub fn dispatch_thread_id(&self) -> &str {
        &self.inner.dispatch_thread_id
    }

    pub fn label(&self) -> &str {
        &self.inner.label
    }

    /// 请求所有者线程关闭并回收子进程。重复请求由所有者线程忽略。
    pub fn close(&self, reason: CloseReason) {
        let _ = self.inner.control.send(ControlMessage::Close(reason));
    }

    /// 所有者线程已接受的关闭原因；对端自行退出时为 `None`。
    pub fn close_reason(&self) -> Option<CloseReason> {
        self.inner
            .shared
            .0
            .lock()
            .ok()
            .and_then(|state| state.requested_close.clone())
    }

    /// 等待子进程被所有者线程回收。返回 `None` 表示超时前仍未退出。
    pub fn wait_exit(&self, timeout: Duration) -> Option<ExitInfo> {
        let (state, changed) = &*self.inner.shared;
        let mut guard = state.lock().ok()?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(info) = guard.exit.as_ref() {
                return Some(info.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, _) = changed.wait_timeout(guard, remaining).ok()?;
            guard = next;
        }
    }
}

pub struct SpawnedProcess {
    pub pid: u32,
    pub stdin: SharedStdin,
    pub stdout: ChildStdout,
    pub stderr: Option<ChildStderr>,
    pub supervisor: ProcessSupervisor,
}

struct StartedChild {
    pid: u32,
    stdin: SharedStdin,
    stdout: ChildStdout,
    stderr: Option<ChildStderr>,
    owner_thread_id: String,
}

/// 由一个长期存活的所有者线程执行 `spawn`。
///
/// `command` 的 working dir、环境与管道配置由调用方完成；本函数负责把真正的
/// `Command::spawn()` 放进所有者线程，并在子进程存活期间保持该线程运行。
pub fn spawn(label: &str, mut command: Command) -> Result<SpawnedProcess, String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let label = label.to_string();
    let dispatch_thread_id = current_thread_id();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<StartedChild, String>>(1);
    let (control_tx, control_rx) = mpsc::channel();
    let shared: SharedState = Arc::new((
        Mutex::new(RuntimeState {
            requested_close: None,
            exit: None,
        }),
        Condvar::new(),
    ));

    let thread_label = label.clone();
    let thread_dispatch = dispatch_thread_id.clone();
    let thread_shared = shared.clone();
    std::thread::Builder::new()
        .name("agent-process-owner".to_string())
        .spawn(move || {
            owner_main(
                thread_label,
                command,
                thread_dispatch,
                ready_tx,
                control_rx,
                thread_shared,
            );
        })
        .map_err(|error| format!("无法启动 {label} 进程所有者线程：{error}"))?;

    match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(started)) => {
            let profile = format!(
                "PID {}，所有者线程 {}，派发线程 {}",
                started.pid, started.owner_thread_id, dispatch_thread_id
            );
            log::info!("[agent] 启动 {label}（{profile}）");
            Ok(SpawnedProcess {
                pid: started.pid,
                stdin: started.stdin,
                stdout: started.stdout,
                stderr: started.stderr,
                supervisor: ProcessSupervisor {
                    inner: Arc::new(SupervisorInner {
                        label,
                        pid: started.pid,
                        owner_thread_id: started.owner_thread_id,
                        dispatch_thread_id,
                        control: control_tx,
                        shared,
                    }),
                },
            })
        }
        Ok(Err(error)) => Err(error),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "启动 {label} 超时（{} 秒内未完成进程创建）",
            START_TIMEOUT.as_secs()
        )),
        Err(RecvTimeoutError::Disconnected) => {
            Err(format!("启动 {label} 失败：进程所有者线程提前退出"))
        }
    }
}

fn owner_main(
    label: String,
    mut command: Command,
    dispatch_thread_id: String,
    ready: SyncSender<Result<StartedChild, String>>,
    control: Receiver<ControlMessage>,
    shared: SharedState,
) {
    let owner_thread_id = current_thread_id();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = ready.send(Err(format!("无法启动 {label}：{error}")));
            return;
        }
    };
    let pid = child.id();
    let stdin: SharedStdin = Arc::new(Mutex::new(child.stdin.take()));
    let Some(stdout) = child.stdout.take() else {
        let _ = ready.send(Err(format!("{label} 没有 stdout")));
        reap_unstarted(&mut child, &label, pid, &owner_thread_id);
        return;
    };
    let stderr = child.stderr.take();

    let started = StartedChild {
        pid,
        stdin: stdin.clone(),
        stdout,
        stderr,
        owner_thread_id: owner_thread_id.clone(),
    };
    if ready.send(Ok(started)).is_err() {
        // 调用方已经放弃等待（超时或线程异常）。子进程没有接收者，必须由
        // 所有者线程立即回收，不能留下孤儿 CLI。
        log::warn!(
            "[agent] {label}（PID {pid}，所有者线程 {owner_thread_id}）启动结果无人接收，主动回收"
        );
        close_stdin(&stdin);
        let _ = child.kill();
        let _ = child.wait();
        return;
    }

    supervise(
        label,
        pid,
        owner_thread_id,
        dispatch_thread_id,
        child,
        stdin,
        control,
        shared,
    );
}

fn supervise(
    label: String,
    pid: u32,
    owner_thread_id: String,
    dispatch_thread_id: String,
    mut child: Child,
    stdin: SharedStdin,
    control: Receiver<ControlMessage>,
    shared: SharedState,
) {
    let mut requested_close: Option<CloseReason> = None;
    let mut deadline: Option<Instant> = None;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                finish(
                    &shared,
                    ExitInfo {
                        status: Some(status),
                        requested_close,
                        forced: false,
                        forced_reason: None,
                    },
                    &label,
                    pid,
                    &owner_thread_id,
                    &dispatch_thread_id,
                );
                return;
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!(
                    "[agent] 查询 {label}（PID {pid}，所有者线程 {owner_thread_id}）状态失败：{error}"
                );
            }
        }

        let wait = deadline
            .map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(POLL_INTERVAL)
            })
            .unwrap_or(POLL_INTERVAL);
        match control.recv_timeout(wait) {
            Ok(ControlMessage::Close(reason)) => {
                if requested_close.is_none() {
                    // 关闭请求到达前进程可能已经自然退出；先读取真实状态，
                    // 不能把对端退出误记成素笺主动结束。
                    if let Ok(Some(status)) = child.try_wait() {
                        finish(
                            &shared,
                            ExitInfo {
                                status: Some(status),
                                requested_close: None,
                                forced: false,
                                forced_reason: None,
                            },
                            &label,
                            pid,
                            &owner_thread_id,
                            &dispatch_thread_id,
                        );
                        return;
                    }
                    requested_close = Some(reason.clone());
                    if let Ok(mut state) = shared.0.lock() {
                        state.requested_close = Some(reason.clone());
                    }
                    close_stdin(&stdin);
                    deadline = Some(Instant::now() + CLOSE_GRACE);
                    log::info!(
                        "[agent] {label}（PID {pid}，所有者线程 {owner_thread_id}）收到关闭请求：{}；已关闭 stdin，等待协议退出",
                        reason.description()
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // 调用方没有显式关闭就释放了全部控制句柄：按应用退出回收，
                // 避免留下仍占用沙箱的孤儿进程。
                if requested_close.is_none() {
                    if let Ok(Some(status)) = child.try_wait() {
                        finish(
                            &shared,
                            ExitInfo {
                                status: Some(status),
                                requested_close: None,
                                forced: false,
                                forced_reason: None,
                            },
                            &label,
                            pid,
                            &owner_thread_id,
                            &dispatch_thread_id,
                        );
                        return;
                    }
                    let reason = CloseReason::AppExit;
                    if let Ok(mut state) = shared.0.lock() {
                        state.requested_close = Some(reason.clone());
                    }
                    requested_close = Some(reason);
                    close_stdin(&stdin);
                    deadline = Some(Instant::now() + CLOSE_GRACE);
                    log::warn!(
                        "[agent] {label}（PID {pid}，所有者线程 {owner_thread_id}）控制通道断开，按应用退出回收"
                    );
                }
            }
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let reason = requested_close
                .clone()
                .unwrap_or_else(|| CloseReason::ChannelError(format!("{label} 未随协议通道退出")));
            log::warn!(
                "[agent] 素笺主动回收 {label} 进程（PID {pid}，所有者线程 {owner_thread_id}），发送 SIGKILL；原因：{}",
                reason.description()
            );
            let _ = child.kill();
            let status = child.wait().ok();
            finish(
                &shared,
                ExitInfo {
                    status,
                    requested_close,
                    forced: true,
                    forced_reason: Some(reason),
                },
                &label,
                pid,
                &owner_thread_id,
                &dispatch_thread_id,
            );
            return;
        }
    }
}

fn finish(
    shared: &SharedState,
    info: ExitInfo,
    label: &str,
    pid: u32,
    owner_thread_id: &str,
    dispatch_thread_id: &str,
) {
    let status = match info.status {
        Some(status) => match status.code() {
            Some(code) => format!("退出码 {code}"),
            None => format!("被信号终止（{status}）"),
        },
        None => "退出状态未知".to_string(),
    };
    let cause = if info.forced {
        format!(
            "素笺主动发送 SIGKILL 回收（原因：{}）",
            info.forced_reason
                .as_ref()
                .map(CloseReason::description)
                .unwrap_or_else(|| "未知".to_string())
        )
    } else if let Some(reason) = info.requested_close.as_ref() {
        format!("素笺主动请求结束（原因：{}）", reason.description())
    } else if info.status.and_then(|status| status.signal()).is_some() {
        "结束来源：外部信号（素笺未发送结束信号）".to_string()
    } else {
        "结束来源未知（素笺未发送结束信号，仅有退出码）".to_string()
    };
    log::info!(
        "[agent] {label} 进程退出（PID {pid}，所有者线程 {owner_thread_id}，派发线程 {dispatch_thread_id}）：{status}；{cause}"
    );
    if let Ok(mut state) = shared.0.lock() {
        state.exit = Some(info);
    }
    shared.1.notify_all();
}

fn close_stdin(stdin: &SharedStdin) {
    if let Ok(mut guard) = stdin.lock() {
        guard.take();
    }
}

fn reap_unstarted(child: &mut Child, label: &str, pid: u32, owner_thread_id: &str) {
    log::warn!("[agent] {label}（PID {pid}，所有者线程 {owner_thread_id}）启动不完整，主动回收");
    let _ = child.kill();
    let _ = child.wait();
}

fn current_thread_id() -> String {
    format!("{:?}", std::thread::current().id())
}
