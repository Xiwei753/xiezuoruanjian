import re

with open('apps/linux/src/main.rs', 'r') as f:
    content = f.read()

# 1. Update the architecture comment
content = content.replace(
    "QML UI → AppBackend (QObject) → WriterCore (Rust Core) → 文件系统",
    "QML UI → AppBackend/Linux adapter → WriterCoreApi → facade::WriterCore → Core domain"
)

# 2. Add use writer_core::api::WriterCoreApi;
if "use writer_core::api::WriterCoreApi;" not in content:
    content = content.replace(
        "use writer_core::facade::WriterCore;",
        "use writer_core::api::WriterCoreApi;\nuse writer_core::facade::WriterCore;"
    )

# 3. Add core_api() and core_facade() helpers to AppBackend impl
helper_code = """
    fn core_api(&self) -> Option<WriterCoreApi> {
        if self.current_has_workspace && !self.current_workspace.is_empty() {
            Some(WriterCoreApi::new(&self.current_workspace))
        } else {
            None
        }
    }

    // TODO(api): migrate when WriterCoreApi exposes this capability
    fn core_facade(&self) -> Option<WriterCore> {
        if self.current_has_workspace && !self.current_workspace.is_empty() {
            Some(WriterCore::new(&self.current_workspace))
        } else {
            None
        }
    }
"""
if "fn core_api(&self)" not in content:
    content = content.replace(
        "impl AppBackend {\n    fn debug_qml_enabled(&self) -> bool {",
        "impl AppBackend {" + helper_code + "\n    fn debug_qml_enabled(&self) -> bool {"
    )

with open('apps/linux/src/main.rs', 'w') as f:
    f.write(content)
