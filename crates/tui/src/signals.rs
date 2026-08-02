//! 进程终止信号（SIGINT / SIGTERM / SIGHUP）的清理处理。
//!
//! 从 `lib.rs` 抽离而来，避免在 crate 根堆积与 CLI 派发无关的平台相关代码。

/// 启动一个 tokio 任务监听终止信号（SIGINT 始终监听；Unix 上还监听
/// SIGTERM 与 SIGHUP），收到后恢复终端模式并以约定俗成的 `128 + 信号编号`
/// 退出码退出。
///
/// 允许多次投递：清理一旦执行，第二次信号会直接短路为普通退出，
/// 以免卡住的清理逻辑把反复按 Ctrl+C 的用户困住。
///
/// 调用点见 `main`（原因见 #1583）。
pub fn spawn_signal_cleanup_task() {
    tokio::spawn(async {
        let exit_code = wait_for_terminating_signal().await;
        // 走到这里说明收到了致命信号。恢复终端并退出。
        // 清理期间的第二次信号会重新进入此路径并通过 `std::process::exit` 直接中止。
        static CLEANED_UP: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !CLEANED_UP.swap(true, std::sync::atomic::Ordering::SeqCst) {
            crate::tui::ui::emergency_restore_terminal();
        }
        std::process::exit(exit_code);
    });
}

#[cfg(unix)]
async fn wait_for_terminating_signal() -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    // 单个流注册失败不致命：我们仍希望其余流正常工作。
    // 当某个流注册失败时，永不解析的 future 让 `select!` 保持类型正确。
    let mut sigint = signal(SignalKind::interrupt()).ok();
    let mut sigterm = signal(SignalKind::terminate()).ok();
    let mut sighup = signal(SignalKind::hangup()).ok();
    tokio::select! {
        _ = async { match sigint.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 130,
        _ = async { match sigterm.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 143,
        _ = async { match sighup.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 129,
    }
}

#[cfg(not(unix))]
async fn wait_for_terminating_signal() -> i32 {
    // Windows 上：tokio::signal::ctrl_c 同时覆盖 Ctrl+C 与 Ctrl+Break
    // （CTRL_C_EVENT / CTRL_BREAK_EVENT）。关闭控制台、注销与关机事件
    // 目前未通过 tokio 路由。
    let _ = tokio::signal::ctrl_c().await;
    130
}
