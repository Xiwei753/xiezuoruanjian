//! Issue #692 评论 5692612221: 屏幕可用几何查询平台封装
//!
//! 将 Qt C++ `QScreen::availableGeometry()`（已扣 KDE 面板/任务栏等窗口管理器
//! 保留区域）暴露给 Rust，供 AppBackend.available_screen_geometry_json() 转发，
//! 最终供 main.qml applyInitialWindowSize() 做一次性初始尺寸收口。
//!
//! 归属平台层原因：cpp!(unsafe[]) FFI 边界只允许出现在平台封装目录
//! （见 tools/check_rust_safety_patterns.py 的 _CPP_UNSAFE_ALLOWED_FILES 与
//! 根 AGENTS.md「unsafe 只放必要的 FFI/平台边界」）。业务后端 app_backend.rs
//! 不直接调用 cpp!(unsafe)。

use cpp::cpp;
use qmetaobject::QString;

cpp! {{
    #include <QtGlobal>
    #include <QGuiApplication>
    #include <QScreen>
    #include <QWindow>
    #include <QRect>
    #include <QJsonDocument>
    #include <QJsonObject>
    #include <QJsonValue>
}}

/// 查询当前窗口所属屏幕的可用几何（QScreen::availableGeometry，已扣任务栏/面板等
/// 窗口管理器保留区域）。
///
/// 返回紧凑 JSON: `{"valid":bool,"x":int,"y":int,"width":int,"height":int}`，
/// 单位为 Qt 6 设备无关逻辑像素，与 QML 坐标空间一致。
///
/// 选择策略：优先取 `QGuiApplication::topLevelWindows()` 中第一个可见窗口的
/// `QWindow::screen()`（当前窗口真正关联的屏幕）；找不到则回退到
/// `QGuiApplication::primaryScreen()`。两者都拿不到时返回 `valid:false`。
///
/// 仅在 GUI 线程调用；不捕获任何 Rust 数据，纯平台全局查询。
pub fn available_screen_geometry_json() -> QString {
    cpp!(unsafe [] -> QString as "QString" {
        QScreen* screen = nullptr;
        const QWindowList windows = QGuiApplication::topLevelWindows();
        for (QWindow* w : windows) {
            if (w && w->isVisible()) {
                screen = w->screen();
                if (screen) break;
            }
        }
        if (!screen) {
            screen = QGuiApplication::primaryScreen();
        }
        QJsonObject obj;
        if (screen) {
            const QRect g = screen->availableGeometry();
            obj.insert(QStringLiteral("valid"), true);
            obj.insert(QStringLiteral("x"), g.x());
            obj.insert(QStringLiteral("y"), g.y());
            obj.insert(QStringLiteral("width"), g.width());
            obj.insert(QStringLiteral("height"), g.height());
        } else {
            obj.insert(QStringLiteral("valid"), false);
            obj.insert(QStringLiteral("x"), 0);
            obj.insert(QStringLiteral("y"), 0);
            obj.insert(QStringLiteral("width"), 0);
            obj.insert(QStringLiteral("height"), 0);
        }
        return QJsonDocument(obj).toJson(QJsonDocument::Compact);
    })
}
