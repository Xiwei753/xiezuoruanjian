use cpp::cpp;

// ── Qt 文本布局模块：测试线程辅助 ──
//
// Issue #748 评论 5810761209: run_on_qt_thread 从 engine.rs 移到此处，
// 不再塞在正式排版引擎里。模块内测试通过 layout::run_on_qt_thread 使用，
// 集成测试通过 apps/Linux_qt/tests/common/qt_runtime.rs 使用。

/// Issue #707 评论 5724685300: 固定 Qt 测试线程。
///
/// Qt 官方线程规则把创建 QCoreApplication/QGuiApplication 的线程视为 GUI/main
/// thread，GUI 相关对象应在该线程使用。`ensure_qt_application()` 只保证
/// QGuiApplication 创建一次，不能保证后续每个 Rust `#[test]` 都在创建
/// QGuiApplication 的同一线程执行。Rust 测试默认并行跑，会违反 Qt 线程规则。
///
/// 本函数建一个专用线程，在该线程内创建 QGuiApplication 并运行通道循环。
/// 所有需要 Qt 的测试逻辑通过 `run_on_qt_thread(|| { ... })` 发到这同一个
/// 线程执行，保证 application 创建和所有 Qt 调用都发生在同一线程。
///
/// 闭包内构造所有 `!Send` 的对象（如 `AppRef`、`SujianEditorItem`），不跨线程
/// 传递。如果闭包 panic，通过 `catch_unwind` 捕获并在调用线程 re-panic，
/// 使测试失败正确传播。
///
/// SAFETY: QGuiApplication 在专用线程创建后不 delete，生命周期与线程相同。
/// offscreen platform 不需要真实显示。所有 Qt 调用都在该线程执行。
#[cfg(any(test, feature = "test-helpers"))]
pub fn run_on_qt_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    use std::sync::mpsc::channel;
    use std::sync::OnceLock;

    struct QtThreadHandle {
        sender: std::sync::mpsc::Sender<Box<dyn FnOnce() + Send + 'static>>,
    }

    static QT_THREAD: OnceLock<QtThreadHandle> = OnceLock::new();

    fn qt_thread() -> &'static QtThreadHandle {
        QT_THREAD.get_or_init(|| {
            let (sender, receiver) = channel::<Box<dyn FnOnce() + Send + 'static>>();
            std::thread::Builder::new()
                .name("qt-test-thread".to_string())
                .spawn(move || {
                    // 在这个专用线程内创建 QGuiApplication。
                    std::env::set_var("QT_QPA_PLATFORM", "offscreen");
                    // SAFETY: QGuiApplication 在这个专用线程创建，生命周期与线程相同。
                    // 所有 Qt 调用都通过 run_on_qt_thread 在这个线程执行。
                    cpp!(unsafe [] {
                        static int argc = 1;
                        static char argv0[] = "sujian-test";
                        static char* argv[] = {argv0, nullptr};
                        static QGuiApplication* app = nullptr;
                        if (QGuiApplication::instance() == nullptr && app == nullptr) {
                            app = new QGuiApplication(argc, argv);
                        }
                    });
                    // 消息循环：接收闭包并执行，panic 不终止线程。
                    while let Ok(f) = receiver.recv() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
                    }
                })
                .expect("failed to spawn qt test thread");
            QtThreadHandle { sender }
        })
    }

    let (result_tx, result_rx) = channel();
    qt_thread()
        .sender
        .send(Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = result_tx.send(result);
        }))
        .expect("qt test thread channel send failed");
    let result = result_rx
        .recv()
        .expect("qt test thread channel recv failed");
    if let Err(panic_payload) = result {
        std::panic::resume_unwind(panic_payload);
    }
}
