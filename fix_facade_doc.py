import re

with open('core/writer_core/src/facade.rs', 'r') as f:
    content = f.read()

content = content.replace(
    "//!\n//! 这是所有客户端（Android、Linux）调用 Core 的**唯一入口**。",
    "//!\n//! 这是 Core 内部统一入口。\n//! **注意：**它不是平台稳定 API 边界。\n//! Android / Linux / 未来平台不得把 Facade 当主暴露层。\n//! 平台应走 `api::WriterCoreApi` 或其绑定适配层。"
)

content = content.replace(
    "//! Android: NativeCoreBridge → WriterCore::create_chapter() → chapter::create_chapter()\n",
    ""
)

content = content.replace(
    "//! Linux:   AppBackend → WriterCore::create_chapter() → chapter::create_chapter()",
    "//! Linux (Legacy): AppBackend/Linux adapter → facade::WriterCore::create_chapter() → chapter::create_chapter()\n//! Linux (New):    AppBackend/Linux adapter → api::WriterCoreApi::create_chapter() → facade::WriterCore::create_chapter() → chapter::create_chapter()"
)

with open('core/writer_core/src/facade.rs', 'w') as f:
    f.write(content)

