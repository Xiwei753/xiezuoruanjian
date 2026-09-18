//! 真实 Qt runtime helper — Issue #707 评论 5724685300。
//!
//! 本文件提供真实 Qt 行为测试的公共基础设施：
//! - `run_on_qt_thread(|| { ... })`: 在固定 Qt 测试线程执行闭包。
//!
//! Issue #707 评论 5724685300: 不再用 `ensure_qt_application()`。
//! Qt 官方线程规则把创建 QGuiApplication 的线程视为 GUI/main thread，
//! GUI 相关对象应在该线程使用。Rust 测试默认并行跑，`ensure_qt_application()`
//! 只保证创建一次，不能保证后续每个 `#[test]` 都在创建 QGuiApplication 的
//! 同一线程执行。
//!
//! `run_on_qt_thread` 建一个专用线程，在该线程内创建 QGuiApplication 并运行
//! 通道循环。所有需要 Qt 的测试逻辑通过 `run_on_qt_thread(|| { ... })` 发到
//! 这同一个线程执行。闭包内构造所有 `!Send` 的对象（如 `AppRef`），不跨线程
//! 传递。如果闭包 panic，在调用线程 re-panic，使测试失败正确传播。
//!
//! 所有 #707 真实行为测试通过 `#[path = "common/qt_runtime.rs"] mod qt_runtime;`
//! 引入，调用 `run_on_qt_thread(|| { ... 测试逻辑 ... })`。

/// 在固定 Qt 测试线程执行闭包。
///
/// 每条真实 Qt 行为测试应把测试逻辑放在 `run_on_qt_thread(|| { ... })` 中。
/// 闭包内构造所有 `!Send` 的对象（如 `AppRef`、`SujianEditorItem`），
/// 不跨线程传递。
pub fn run_on_qt_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    sujian_linux_qt::editor::layout::run_on_qt_thread(f);
}
