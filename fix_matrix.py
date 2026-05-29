import re

with open('docs/CAPABILITY_MATRIX.md', 'r') as f:
    content = f.read()

# Replace column headers
content = content.replace("| Android JNI 入口 | Android Kotlin 入口 |", "| Android JNI 入口 (Legacy) | Android Kotlin 入口 (AppServiceBridge) |")

# For Workspace
content = content.replace("`createWorkspace`, `validateWorkspace`", "`AppServiceBridge.createWorkspace`")
content = content.replace("`NativeCoreBridge.createWorkspace`, `validateWorkspace`", "`AppServiceBridge.createWorkspace`")
content = content.replace("| `facade::WriterCore::new`, `create_workspace`, `validate_workspace` |", "| `WriterCoreApi::create_workspace`, `validate_workspace` |")

# Replace mentions of NativeCoreBridge as main to Legacy
content = content.replace(
    "Android JNI 单独导出",
    "Android 通过 AppServiceBridge 调用 UniFFI，部分旧接口保留在 NativeCoreBridge"
)

# For Linux backend, replace "Linux backend 入口" with "Linux backend (WriterCoreApi)"
content = content.replace("| Linux backend 入口 |", "| Linux backend (WriterCoreApi) |")

with open('docs/CAPABILITY_MATRIX.md', 'w') as f:
    f.write(content)

