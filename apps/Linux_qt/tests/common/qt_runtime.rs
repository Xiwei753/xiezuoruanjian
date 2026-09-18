//! 真实 Qt runtime helper — Issue #707 评论 5723616999。
//!
//! 本文件提供真实 Qt 行为测试的公共基础设施：
//! - `ensure_qt_application()`: 确保测试进程已创建 QGuiApplication（offscreen）。
//!
//! 本 helper 不再读取源码字符串。源码字符串守卫 helper 已拆分到
//! `common/source_guard.rs`，供旧 WHITE_BOX 测试使用。
//!
//! 所有 #707 真实行为测试通过 `#[path = "common/qt_runtime.rs"] mod qt_runtime;`
//! 引入，调用 `ensure_qt_application()` 后再构造生产对象。

/// 确保测试进程已创建 QGuiApplication（offscreen platform）。
///
/// Qt GUI 对象（QFont/QTextLayout/QTextLine/LinuxThemeController 等）必须在
/// QGuiApplication 创建之后使用。本函数委托给生产代码
/// `sujian_linux_qt::editor::layout::ensure_qt_application`，后者用进程级
/// 静态变量保证只创建一次。
///
/// 每条真实 Qt 行为测试应在首行调用此函数。
pub fn ensure_qt_application() {
    sujian_linux_qt::editor::layout::ensure_qt_application();
}
