#!/usr/bin/env python3
"""一次性收口脚本（#753）：删除 ArkTS 端 Core 从不发出的 envelope 字段。

Core ffi/mod.rs 的 envelope 只有 success/data/errorCode/userMessage。
messageKey / messageArgs / warnings / changedPaths / changedEntities / rawError
都是 ArkTS 自己发明的字段，运行时永远是 undefined，UI 读它们等于读空。
"""
import re
import sys
from pathlib import Path

ROOT = Path("apps/harmony/entry/src")

# envelope 里要删掉的三件套（总是成组出现）
TRIO = re.compile(
    r",\s*warnings:\s*[^,{}]+,\s*changedPaths:\s*[^,{}]+,\s*changedEntities:\s*[^,{}]+"
)
TRIO_LINE = re.compile(r"^\s*(?:warnings|changedPaths|changedEntities):\s*[^,{}]+,?\s*$")
MSG_KEY = re.compile(r",?\s*messageKey:\s*(?:'[^']*'|[A-Za-z_][\w.]*(?:\s*\?\?[^,{}]+)?)\s*,?")
MSG_ARGS = re.compile(r",?\s*messageArgs:\s*\{[^{}]*\}\s*as\s*Record<string,\s*string>\s*,?")
MSG_ARGS2 = re.compile(r",?\s*messageArgs:\s*[A-Za-z_][\w.]*\s*,?")
CHANGED_IMPORT = re.compile(r"import\s*\{\s*ChangedEntity,\s*")
RAW_ERROR = re.compile(r"\brawError:\s*")

FILES = [
    "main/ets/corebridge/NativeWriterCoreBridge.ets",
    "main/ets/corebridge/native/NativeAppStateBridge.ets",
    "main/ets/corebridge/native/NativeChapterBridge.ets",
    "main/ets/corebridge/native/NativeCoreModule.ets",
    "main/ets/corebridge/native/NativeProjectBridge.ets",
    "main/ets/corebridge/native/NativeSettingsBridge.ets",
    "main/ets/corebridge/native/NativeStarMapBridge.ets",
    "main/ets/corebridge/native/NativeStatsBridge.ets",
    "main/ets/corebridge/native/NativeSyncBridge.ets",
    "main/ets/feature/editor/interop/NativeEditorSessionBridge.ets",
    "main/ets/feature/editor/input/EditorInputAdapter.ets",
    "main/ets/feature/editor/input/EditorSemanticDispatcher.ets",
    "main/ets/feature/editor/session/EditorSessionCoordinator.ets",
    "ohosTest/ets/mock/MockWriterCoreBridge.ets",
    "ohosTest/ets/test/CoreDtosTest.ets",
    "ohosTest/ets/test/EditorSessionPatchTest.ets",
]


def tidy(text: str) -> str:
    # 修复删除后出现的 `{ ,` / `, ,` / `,, ` / `, }`
    text = re.sub(r"\{\s*,", "{ ", text)
    text = re.sub(r",\s*,", ", ", text)
    text = re.sub(r",\s*\}", " }", text)
    text = re.sub(r"\{\s*\}", "{}", text)
    text = re.sub(r"\(\s*,", "(", text)
    return text


def main() -> int:
    for rel in FILES:
        path = ROOT / rel
        src = path.read_text(encoding="utf-8")
        orig = src
        lines = src.split("\n")
        kept = [ln for ln in lines if not TRIO_LINE.match(ln)]
        src = "\n".join(kept)
        src = TRIO.sub("", src)
        src = MSG_ARGS.sub("", src)
        src = MSG_ARGS2.sub("", src)
        src = MSG_KEY.sub("", src)
        src = CHANGED_IMPORT.sub("import { ", src)
        src = RAW_ERROR.sub("userMessage: ", src)
        src = tidy(src)
        src = re.sub(r"import\s*\{\s*\}\s*from", "", src)
        if src != orig:
            path.write_text(src, encoding="utf-8")
            print(f"rewrote {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
