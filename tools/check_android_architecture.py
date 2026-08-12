#!/usr/bin/env python3
"""Android 分层架构源码扫描器（#597 六）。

业务分层规则直接扫描 `:app` 主源码（apps/android/app/src/main/kotlin），
不编译 Android App（不再通过 JUnit/Gradle 单元测试任务运行架构检查）。
`package-dir-consistent` 规则动态扫描 `:app`、`:core:designsystem`、`:core:platform`
三个 Gradle 模块的所有 `src/*/kotlin` 源集（glob 发现，不硬编码源集名单）。

保留的规则（对应原 `src/testArch` 静态规则，issue 正文第六节）：

1.  UI 层（app/feature）不能直接依赖具体 Bridge 类、UniFFI 绑定或 JNA；
2.  UI 层不能直接依赖 feature/editor/input 基础设施；
3.  数据/Bridge 层（core/interop、feature/*/data）不能依赖
    Compose/Activity/View/UI 与 feature/editor 显示/动画状态；
4.  feature/editor/input 只产生输入操作：不依赖 Repository/core/interop、Compose UI、
    Activity；UniFFI 只允许 EditorTransactionCauseDto 契约类型；
5.  feature/editor/visual 与 motion 只处理显示和动画状态：不写正文持久状态
    （feature/*/data）、不依赖 Activity/View/input、不依赖 Compose UI 框架、
    UniFFI 只允许 DTO 契约类型；
6.  editor session 层（EditorSessionCoordinator*）不能依赖 Compose 可变状态、
    View/Activity；派生 stateIn flow 与 reduceScope 已删除；唯一状态出口是
    sessionStateFlow + value getter；
7.  FrameClock 只能由窗口/显示层持有：EditorWindowHost 拥有唯一
    windowFrameClock 字段，session 层不得引用 WindowDisplayFrameClock；
8.  updateSessionState transform 是纯函数：transform 体内不得调用
    store.put/store.update/store.remove，store 写入只能在 transform 外
    通过 pendingRecord?.let { store.put(it) } 执行；
9.  core/designsystem 不能反向依赖 app 模块（源码与 build.gradle.kts 均不得）；
10. 已删除类型/入口不得复活：EditorAnimationSettings、派生 flow getter、
    SettingsRepository 旧的 1 参 setSyncSecretsOverride；
11. 结构契约：session/窗口层状态出口、FrameClock 生命周期、motion policy、
    同步事务提交、凭据类型化读取、预览状态纯净字段等存在性约束
    （原反射测试改为源码级检查）。

用法:
    python3 tools/check_android_architecture.py [--app-src DIR] [--designsystem-src DIR] [--designsystem-module DIR] [--platform-module DIR]

返回码:
    0 = 全部规则通过
    1 = 存在违规（报告按规则分类列出）
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent

# 默认扫描根（真实仓库路径）；configure() 只改同名运行时全局，
# 不覆盖这些 DEFAULT_* 常量，测试可随时回到真实仓库。
DEFAULT_APP_SRC = (
    PROJECT_ROOT / "apps" / "android" / "app" / "src" / "main" / "kotlin" / "com" / "xiwei" / "sujian"
)
DEFAULT_DS_SRC = (
    PROJECT_ROOT
    / "apps"
    / "android"
    / "core"
    / "designsystem"
    / "src"
    / "main"
    / "kotlin"
    / "com"
    / "xiwei"
    / "sujian"
    / "core"
    / "designsystem"
)
DEFAULT_DS_MODULE = PROJECT_ROOT / "apps" / "android" / "core" / "designsystem"
DEFAULT_PLATFORM_MODULE = PROJECT_ROOT / "apps" / "android" / "core" / "platform"
DEFAULT_APP_MODULE = PROJECT_ROOT / "apps" / "android" / "app"

# package-dir 一致性检查覆盖的源集（#602 评论#8 项8.2）。
# 不能只扫 src/main：debug/release/test/androidTest/testAi/androidTestAi 各源集
# 的 package 声明也必须与物理目录结构一致，否则重构会留下隐性错位。
# 源集根由 module 根动态发现，configure(--app-src) 时同步更新
# （#602 评论#9 项9.3、评论#10 项1）。


def package_source_roots_from_modules(module_roots: tuple[Path, ...]) -> tuple[Path, ...]:
    """从 Gradle module 根动态发现所有 Kotlin 源集根（#602 评论#10 项1）。

    对每个 module 遍历 src/*/kotlin（glob 动态发现，不硬编码 source set 名单），
    这样以后新增 testFoo、androidTestFoo 等源集也不用再改名单。
    """
    roots: list[Path] = []
    for module in module_roots:
        src_dir = module / "src"
        if not src_dir.is_dir():
            continue
        for kotlin_root in sorted(src_dir.glob("*/kotlin")):
            if kotlin_root.is_dir():
                roots.append(kotlin_root)
    return tuple(roots)


PACKAGE_SOURCE_ROOTS = package_source_roots_from_modules(
    (DEFAULT_APP_MODULE, DEFAULT_DS_MODULE, DEFAULT_PLATFORM_MODULE)
)

APP_SRC = DEFAULT_APP_SRC
DS_SRC = DEFAULT_DS_SRC
DS_MODULE = DEFAULT_DS_MODULE
PLATFORM_MODULE = DEFAULT_PLATFORM_MODULE


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    message: str


def strip_line_comment(line: str) -> str:
    """去掉 // 单行注释（与旧 Kotlin 检测器一致，避免注释误报）。"""
    idx = line.find("//")
    return line if idx < 0 else line[:idx]


def effective_lines(content: str) -> list[str]:
    """去掉块注释（/* ... */）与单行注释后的逐行内容。

    文档注释里合法地提及被禁止类型（如 session 层注释说明自己不持有
    WindowDisplayFrameClock），不得被当成真实引用。
    """
    without_block = re.sub(r"/\*.*?\*/", "", content, flags=re.S)
    return [strip_line_comment(line) for line in without_block.splitlines()]


def references(line: str, forbidden: list[str]) -> list[str]:
    """返回该行命中的禁止引用（忽略行内注释 — 由调用方先过 [effective_lines]）。"""
    return [ref for ref in forbidden if ref in line]


def collect_kt_files(root: Path, path_filter: str | None = None) -> list[Path]:
    if not root.exists():
        return []
    return sorted(
        p
        for p in root.rglob("*.kt")
        if p.is_file()
        and "/build/" not in str(p)
        and "/generated/" not in str(p)
        and (path_filter is None or path_filter in str(p))
    )


def scan_forbidden(
    root: Path,
    path_filter: str | None,
    forbidden: list[str],
) -> list[Finding]:
    findings: list[Finding] = []
    for path in collect_kt_files(root, path_filter):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            for hit in references(raw, forbidden):
                findings.append(
                    Finding(
                        path=str(path.relative_to(root)),
                        line=lineno,
                        message=f"禁止引用 {hit}",
                    )
                )
    return findings


def scan_prefix_with_allowed(
    root: Path,
    path_filter: str,
    prefix: str,
    allowed_fqns: list[str],
) -> list[Finding]:
    """逐行检查：包含 prefix 但未包含任何 allowed FQN 的行构成违规。"""
    findings: list[Finding] = []
    for path in collect_kt_files(root, path_filter):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            if prefix in raw and not any(allowed in raw for allowed in allowed_fqns):
                findings.append(
                    Finding(
                        path=str(path.relative_to(root)),
                        line=lineno,
                        message=f"包含 {prefix} 但不在允许白名单 {allowed_fqns} 内",
                    )
                )
    return findings


# ---------------------------------------------------------------------------
# 规则实现（每个规则返回违规列表）
# ---------------------------------------------------------------------------

CONCRETE_BRIDGES = [
    # BridgeProvider / ActionBridge 已删除（#602 Phase 8），不再列入禁止引用清单。
    "com.xiwei.sujian.core.interop.common.BridgeMappers",
    "com.xiwei.sujian.core.interop.project.ChapterBridge",
    "com.xiwei.sujian.core.interop.project.ProjectBridge",
    "com.xiwei.sujian.core.interop.settings.SettingsBridge",
    "com.xiwei.sujian.feature.stats.data.interop.StatsBridge",
    "com.xiwei.sujian.feature.sync.data.interop.SyncBridge",
    "com.xiwei.sujian.core.interop.project.WritingBridge",
    "com.xiwei.sujian.core.interop.app.AppServiceBridge",
    "com.xiwei.sujian.feature.starmap.data.interop.StarMapBridge",
]

COMPOSE_UI_FRAMEWORK = [
    "androidx.compose.ui",
    "androidx.compose.material3",
    "androidx.compose.foundation",
    "androidx.compose.animation",
]

# 各 feature 模块的 data 子包（Repository/Bridge 所在层，#602 评论#8 项8.3）。
# input/visual/motion 纯净层不得直接依赖这些 data 层。
FEATURE_DATA_PACKAGES = [
    "com.xiwei.sujian.feature.project.data",
    "com.xiwei.sujian.feature.settings.data",
    "com.xiwei.sujian.feature.sync.data",
    "com.xiwei.sujian.feature.stats.data",
    "com.xiwei.sujian.feature.starmap.data",
]

# editor 显示/平台/动画层包（#602 评论#8 项8.3）。
# data 层不得反向依赖这些显示层。注意：不禁止整个 feature.editor，
# 因为 ChapterRepository 合法实现 feature.editor.session.ChapterContentSavePort。
EDITOR_DISPLAY_PACKAGES = [
    "com.xiwei.sujian.feature.editor.ui",
    "com.xiwei.sujian.feature.editor.visual",
    "com.xiwei.sujian.feature.editor.motion",
    "com.xiwei.sujian.feature.editor.render",
    "com.xiwei.sujian.feature.editor.platform",
    "com.xiwei.sujian.feature.editor.window",
    "com.xiwei.sujian.feature.editor.projection",
]

SESSION_LAYER_FILES = {
    "EditorSessionCoordinator.kt",
    "EditorSessionCoordinatorTypes.kt",
    "EditorSessionEditOps.kt",
    "EditorSessionExternalOps.kt",
    "EditorSessionLifecycleOps.kt",
    "EditorSessionStore.kt",
    "EditorSessionState.kt",
    "SessionCommandPort.kt",
    "SessionResetSource.kt",
    "DocumentVersion.kt",
    "EditorDocumentUpdate.kt",
    "EditorSessionViewModel.kt",
    "TextEditorProfile.kt",
}

# 窗口/显示层专属文件：允许持有 Compose 状态、View 与 FrameClock。
WINDOW_LAYER_FILES = {"EditorWindowHost.kt", "EditableTextTarget.kt", "WindowDisplayFrameClock.kt"}

DERIVED_FLOW_GETTERS = [
    "getActiveTargetIdFlow",
    "getEditingStateFlow",
    "getWindowBindingStateFlow",
]

SESSION_STATE_CONTRACT = {
    "sessionStateFlow": "唯一状态出口 sessionStateFlow",
    "activeTargetId": "value getter activeTargetId",
    "editingState": "value getter editingState",
    "windowBindingState": "value getter windowBindingState",
}


# UI 层目录中承担 Repository 职责的文件豁免：这些文件虽位于 app/ 等目录，
# 但承担数据层 Repository 角色（#602 Phase 7 拆分），需要直接访问 Bridge/UniFFI。
# 豁免只针对 ui-no-uniffi-jna-bridge 规则，其他 UI 纯显示文件仍受约束。
UI_LAYER_REPOSITORY_EXEMPTIONS = {
    "app/theme/ThemeRepository.kt",
}

# #602 目录重构：app/<module>/interop/ 下的文件是 UniFFI/JNA 边界 Bridge，
# 合法引用 uniffi.writer_core 与具体 Bridge 类（与 core/interop/ 边界同理）。
# 仅精确匹配 app/<module>/interop/ 路径段，不弱化其他 UI 纯显示文件的约束。
_APP_INTEROP_BOUNDARY_RE = re.compile(r"^app/[^/]+/interop/")


def _is_app_interop_boundary(path: str) -> bool:
    """判断相对路径是否属于 app/<module>/interop/ UniFFI 边界目录。"""
    return bool(_APP_INTEROP_BOUNDARY_RE.match(path))


def _feature_ui_filters() -> list[str]:
    """动态发现所有 feature/*/ui 目录（#610 评论六第5点）。

    不再手写一个漏一个；新增 feature 模块时自动覆盖。
    """
    filters: list[str] = []
    feature_dir = APP_SRC / "feature"
    if feature_dir.is_dir():
        for sub in sorted(feature_dir.iterdir()):
            if sub.is_dir() and (sub / "ui").is_dir():
                filters.append(f"/feature/{sub.name}/ui/")
    return filters


def rule_ui_no_uniffi_jna_bridge() -> list[Finding]:
    ui_filters = [
        "/sujian/app/theme/",
        "/sujian/app/navigation/",
    ] + _feature_ui_filters()
    findings = []
    for f in ui_filters:
        findings += scan_forbidden(APP_SRC, f, ["uniffi.writer_core", "com.sun.jna"])
        findings += scan_forbidden(APP_SRC, f, CONCRETE_BRIDGES)
    # 豁免承担 Repository 职责的文件（数据层，非纯 UI）
    findings = [f for f in findings if f.path not in UI_LAYER_REPOSITORY_EXEMPTIONS]
    # 豁免 app/<module>/interop/ UniFFI 边界 Bridge（#602 重构后的合法边界）
    findings = [f for f in findings if not _is_app_interop_boundary(f.path)]
    return findings


def rule_ui_no_editor_input() -> list[Finding]:
    ui_filters = [
        "/sujian/app/theme/",
        "/sujian/app/navigation/",
    ] + _feature_ui_filters()
    findings = []
    for f in ui_filters:
        findings += scan_forbidden(APP_SRC, f, ["com.xiwei.sujian.feature.editor.input"])
    return findings


def rule_data_no_ui_framework() -> list[Finding]:
    # #602 评论#8 项8.3：data 边界不止 core/interop，还包括各 feature/*/data 与
    # 这些 Repository/Bridge 层都不得依赖 Compose/Activity/View。
    data_filters = [
        "/core/interop/",
        "/feature/project/data/",
        "/feature/settings/data/",
        "/feature/sync/data/",
        "/feature/stats/data/",
        "/feature/starmap/data/",
    ]
    forbidden = ["androidx.compose", "androidx.activity", "android.view"]
    findings: list[Finding] = []
    for f in data_filters:
        findings += scan_forbidden(APP_SRC, f, forbidden)
    return findings


def rule_data_no_editor_display() -> list[Finding]:
    # #602 评论#8 项8.3：data 层（core/interop + feature/*/data）
    # 不得反向依赖 editor 显示/平台/动画层。禁止 EDITOR_DISPLAY_PACKAGES（精确列出
    # ui/visual/motion/render/platform/window/projection），不一刀禁止整个 feature.editor，
    # 因为 ChapterRepository 合法实现 feature.editor.session.ChapterContentSavePort。
    data_filters = [
        "/core/interop/",
        "/feature/project/data/",
        "/feature/settings/data/",
        "/feature/sync/data/",
        "/feature/stats/data/",
        "/feature/starmap/data/",
    ]
    findings: list[Finding] = []
    for f in data_filters:
        findings += scan_forbidden(APP_SRC, f, EDITOR_DISPLAY_PACKAGES)
    return findings


def rule_input_layer_pure() -> list[Finding]:
    # #602 评论#8 项8.3：input 层不得直接依赖各 feature data 层与 ThemeRepository；
    # feature.home 已删除，从禁止清单移除。
    input_forbidden = (
        ["com.xiwei.sujian.core.interop"]
        + FEATURE_DATA_PACKAGES
        + ["com.xiwei.sujian.app.theme.ThemeRepository"]
    )
    findings = scan_forbidden(
        APP_SRC,
        "/feature/editor/input/",
        input_forbidden,
    )
    findings += scan_forbidden(
        APP_SRC,
        "/feature/editor/input/",
        ["androidx.compose.ui", "androidx.compose.material3", "androidx.compose.foundation", "androidx.activity"],
    )
    findings += scan_prefix_with_allowed(
        APP_SRC,
        "/feature/editor/input/",
        "uniffi.writer_core",
        ["uniffi.writer_core.EditorTransactionCauseDto"],
    )
    return findings


def rule_visual_motion_pure() -> list[Finding]:
    findings: list[Finding] = []
    for sub in ("/feature/editor/visual/", "/feature/editor/motion/"):
        # #602 评论#8 项8.3：visual/motion 不得直接依赖各 feature data 层与
        # ThemeRepository；feature.home 已删除，从禁止清单移除。
        visual_motion_forbidden = (
            ["com.xiwei.sujian.core.interop"]
            + FEATURE_DATA_PACKAGES
            + ["com.xiwei.sujian.app.theme.ThemeRepository"]
        )
        findings += scan_forbidden(
            APP_SRC,
            sub,
            visual_motion_forbidden,
        )
        findings += scan_forbidden(
            APP_SRC,
            sub,
            ["androidx.activity", "android.view", "com.xiwei.sujian.feature.editor.input"],
        )
        findings += scan_forbidden(APP_SRC, sub, COMPOSE_UI_FRAMEWORK)
        if sub == "/feature/editor/motion/":
            findings += scan_forbidden(APP_SRC, sub, ["uniffi.writer_core"])
        else:
            findings += scan_prefix_with_allowed(
                APP_SRC,
                sub,
                "uniffi.writer_core",
                [
                    "uniffi.writer_core.EditorOperationKindDto",
                    "uniffi.writer_core.AnimationModeDto",
                    # #606 评论5: visual 层直接消费 Core 计算的旧→新逻辑 slice 对应关系
                    # （RebasePlanner.applyRebaseToSlices / RebaseMappingProvider），
                    # 只读消费、不重新推导；其余 uniffi DTO 仍禁止进入 visual/motion。
                    "uniffi.writer_core.RebaseSliceMappingDto",
                ],
            )
    return findings


def _session_layer_files() -> list[Path]:
    # #602 目录重构：session 层文件从 feature/editor/coordinator 迁移到
    # feature/editor/session。
    session = APP_SRC / "feature" / "editor" / "session"
    if not session.exists():
        return []
    return [
        p
        for p in sorted(session.glob("*.kt"))
        if p.name in SESSION_LAYER_FILES
    ]


def rule_session_layer_no_platform_state() -> list[Finding]:
    findings: list[Finding] = []
    for path in _session_layer_files():
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            for hit in references(
                raw,
                ["mutableStateOf", "androidx.compose.runtime.MutableState", "android.view", "androidx.activity"],
            ):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message=f"session 层禁止依赖 Compose 可变状态/View/Activity: {hit}",
                    )
                )
    # #602 目录重构：EditorSessionCoordinator 在 session/，EditorWindowHost 在 window/。
    session_dir = APP_SRC / "feature" / "editor" / "session"
    window_dir = APP_SRC / "feature" / "editor" / "window"
    layer_dirs = {
        "EditorSessionCoordinator.kt": (session_dir, "feature/editor/session"),
        "EditorWindowHost.kt": (window_dir, "feature/editor/window"),
    }
    for name, (base_dir, rel_prefix) in layer_dirs.items():
        path = base_dir / name
        if not path.exists():
            continue
        content = path.read_text(encoding="utf-8")
        effective = "\n".join(effective_lines(content))
        for getter in DERIVED_FLOW_GETTERS:
            if getter in effective:
                findings.append(
                    Finding(
                        path=f"{rel_prefix}/{name}",
                        line=0,
                        message=f"派生 stateIn flow {getter} 必须保持删除（#595 三）",
                    )
                )
        if "reduceScope" in effective:
            findings.append(
                Finding(
                    path=f"{rel_prefix}/{name}",
                    line=0,
                    message="reduceScope 必须保持删除（#595 三）",
                )
            )
    coordinator_source_path = session_dir / "EditorSessionCoordinator.kt"
    if coordinator_source_path.exists():
        effective = "\n".join(
            effective_lines(coordinator_source_path.read_text(encoding="utf-8"))
        )
        for symbol, desc in SESSION_STATE_CONTRACT.items():
            if symbol not in effective:
                findings.append(
                    Finding(
                        path="feature/editor/session/EditorSessionCoordinator.kt",
                        line=0,
                        message=f"缺少 {desc}: {symbol}",
                    )
                )
    return findings


def rule_frame_clock_window_owned() -> list[Finding]:
    findings: list[Finding] = []
    # #602 目录重构：session 层文件在 feature/editor/session。
    session_dir = APP_SRC / "feature" / "editor" / "session"
    if not session_dir.exists():
        return findings
    for path in _session_layer_files():
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            if references(raw, ["WindowDisplayFrameClock"]):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="FrameClock 只能由窗口/显示层持有，session 层不得引用 WindowDisplayFrameClock",
                    )
                )
    # #602 目录重构：EditorWindowHost 迁移到 feature/editor/window。
    window_host = APP_SRC / "feature" / "editor" / "window" / "EditorWindowHost.kt"
    if window_host.exists():
        effective = "\n".join(
            effective_lines(window_host.read_text(encoding="utf-8"))
        )
        if "val windowFrameClock: WindowDisplayFrameClock" not in effective:
            findings.append(
                Finding(
                    path="feature/editor/window/EditorWindowHost.kt",
                    line=0,
                    message="窗口层必须持有唯一 windowFrameClock: WindowDisplayFrameClock 字段",
                )
            )
    return findings


def rule_transform_purity() -> list[Finding]:
    """updateSessionState { ... } transform 体内不得调用 store.put/update/remove。"""
    findings: list[Finding] = []
    # #602 目录重构：transform 纯函数检查针对 feature/editor/session。
    session_dir = APP_SRC / "feature" / "editor" / "session"
    if not session_dir.exists():
        return findings
    sources = "\n".join(
        p.read_text(encoding="utf-8")
        for p in sorted(session_dir.glob("*.kt"))
    )
    pattern = re.compile(r"updateSessionState\s*\{")
    bodies: list[str] = []
    for match in pattern.finditer(sources):
        start = match.end()
        depth = 1
        i = start
        while i < len(sources) and depth > 0:
            if sources[i] == "{":
                depth += 1
            elif sources[i] == "}":
                depth -= 1
            i += 1
        bodies.append(sources[start : i - 1])
    if not bodies:
        findings.append(
            Finding(
                path="feature/editor/session",
                line=0,
                message="必须存在 updateSessionState transform（找不到任何调用）",
            )
        )
    for idx, body in enumerate(bodies, 1):
        for store_call in ("store.put(", "store.update(", "store.remove("):
            if store_call in body:
                findings.append(
                    Finding(
                        path="feature/editor/session",
                        line=0,
                        message=(
                            f"第 {idx} 个 updateSessionState transform 体内调用 {store_call} — "
                            "transform 是纯函数，store 写入只能在 transform 外通过 "
                            "pendingRecord?.let { store.put(it) } 执行（#595 五）"
                        ),
                    )
                )
    if "pendingRecord?.let { store.put(it) }" not in sources:
        findings.append(
            Finding(
                path="feature/editor/session",
                line=0,
                message="store 写入必须使用 pendingRecord?.let { store.put(it) } 模式（transform 外）",
            )
        )
    return findings


def rule_designsystem_independent() -> list[Finding]:
    forbidden_app_packages = [
        "com.xiwei.sujian.app",
        "com.xiwei.sujian.feature",
        "com.xiwei.sujian.core.interop",
        "com.xiwei.sujian.core.model",
        "com.xiwei.sujian.core.platform",
    ]
    findings = scan_forbidden(DS_SRC, None, forbidden_app_packages)
    findings += scan_forbidden(DS_SRC, None, ["uniffi.writer_core", "com.sun.jna", "com.xiwei.sujian.core.interop"])
    build_script = DS_MODULE / "build.gradle.kts"
    if build_script.exists():
        effective = "\n".join(
            strip_line_comment(line)
            for line in build_script.read_text(encoding="utf-8").splitlines()
        )
        if ":app" in effective or 'project("app")' in effective:
            findings.append(
                Finding(
                    path="core/designsystem/build.gradle.kts",
                    line=0,
                    message="core/designsystem 的 build.gradle.kts 不得声明对 :app 项目的依赖",
                )
            )
    return findings


def rule_deleted_types_stay_deleted() -> list[Finding]:
    findings: list[Finding] = []
    for path in collect_kt_files(APP_SRC, "/feature/editor/"):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            if re.search(r"\b(class|interface|data class)\s+EditorAnimationSettings\b", raw):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="EditorAnimationSettings 必须保持删除（#595 十：EditorMotionPolicy 是唯一可写动画状态源）",
                    )
                )
    # #602 目录重构：SettingsRepository 迁移到 feature/settings/data。
    settings_repo = APP_SRC / "feature" / "settings" / "data" / "SettingsRepository.kt"
    if settings_repo.exists():
        effective = "\n".join(effective_lines(settings_repo.read_text(encoding="utf-8")))
        if re.search(r"fun\s+setSyncSecretsOverride\s*\(", effective):
            findings.append(
                Finding(
                    path="feature/settings/data/SettingsRepository.kt",
                    line=0,
                    message="旧 swallow-failure setSyncSecretsOverride 必须保持删除（#595 十）",
                )
            )
    # #617 评论四：labs 旧实验设置实现已删除（第二套真相），不得复活。
    labs_dir = APP_SRC / "app" / "labs"
    if labs_dir.exists():
        for path in collect_kt_files(labs_dir, ""):
            findings.append(
                Finding(
                    path=str(path.relative_to(APP_SRC)),
                    line=0,
                    message="app/labs 旧实验设置实现必须保持删除（#617 评论四：第二套真相）",
                )
            )
    for labs_type in ("ExperimentalSettingsRepository", "ExperimentalFeatureRegistry"):
        for path in collect_kt_files(APP_SRC, ""):
            for lineno, raw in enumerate(effective_lines(path.read_text(encoding="utf-8")), 1):
                if re.search(rf"\b(class|interface|data class|object)\s+{labs_type}\b", raw):
                    findings.append(
                        Finding(
                            path=str(path.relative_to(APP_SRC)),
                            line=lineno,
                            message=f"{labs_type} 必须保持删除（#617 评论四：第二套真相）",
                        )
                    )
    # #617 评论六：整份 localSettingsState 可观察状态已删除 — 窗口执行层只认
    # immersiveFullscreenEnabled 这一位（构造时从 prefs 初始化、保存成功后同步）。
    # 把整份 LocalSettings 重新暴露为仓库级可观察状态的接线方式不得复活：
    # 其它本地设置字段的保存/读取会因此无谓触发应用根重组（评论六批评的热路径）。
    for path in collect_kt_files(APP_SRC, ""):
        for lineno, raw in enumerate(effective_lines(path.read_text(encoding="utf-8")), 1):
            if re.search(r"\b(var|val)\s+localSettingsState\b", raw):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="localSettingsState 必须保持删除（#617 评论六：窗口层只认 immersiveFullscreenEnabled 这一位）",
                    )
                )
    return findings


def rule_source_contracts() -> list[Finding]:
    """存在性契约（原反射结构测试改为源码级检查）。"""
    findings: list[Finding] = []

    def require(
        path: Path,
        pattern: str,
        desc: str,
    ) -> None:
        if not path.exists():
            findings.append(Finding(str(path.relative_to(APP_SRC)), 0, f"文件缺失: {desc}"))
            return
        effective = "\n".join(effective_lines(path.read_text(encoding="utf-8")))
        if not re.search(pattern, effective):
            findings.append(
                Finding(str(path.relative_to(APP_SRC)), 0, f"缺少 {desc}（模式 {pattern}）")
            )

    def forbid(
        path: Path,
        pattern: str,
        desc: str,
    ) -> None:
        if not path.exists():
            return
        effective = "\n".join(effective_lines(path.read_text(encoding="utf-8")))
        if re.search(pattern, effective):
            findings.append(
                Finding(str(path.relative_to(APP_SRC)), 0, f"禁止出现 {desc}（模式 {pattern}）")
            )

    # #602 目录重构：EditorWindowHost 在 feature/editor/window。
    window_dir = APP_SRC / "feature" / "editor" / "window"
    host = window_dir / "EditorWindowHost.kt"
    require(host, r"fun\s+beginEdit\s*\(", "EditorWindowHost.beginEdit（活动编辑生命周期入口）")
    require(host, r"fun\s+releaseWindow\s*\(", "EditorWindowHost.releaseWindow（释放 FrameClock 连接）")
    require(host, r"fun\s+getChapterPreviewState\s*\(", "EditorWindowHost.getChapterPreviewState(String)")
    require(host, r"fun\s+applyMotionPolicy\s*\(", "EditorWindowHost.applyMotionPolicy(EditorMotionPolicy)")
    require(host, r"\bmotionPolicyFlow\b", "EditorWindowHost.motionPolicyFlow 委托")

    motion = APP_SRC / "feature" / "editor" / "motion" / "EditorMotionPolicy.kt"
    require(motion, r"val\s+reduceMotion\b", "EditorMotionPolicy.reduceMotion 字段")

    # 同步函数已从 SettingsRepository 迁移到 SyncRepository（#602 Phase 7 拆分）。
    # SyncRepository 位于 feature/sync/data/，承担同步数据访问职责。
    sync_repo = APP_SRC / "feature" / "sync" / "data" / "SyncRepository.kt"
    require(sync_repo, r"fun\s+commitSyncProfile\s*\(", "SyncRepository.commitSyncProfile(SyncConfig, SyncSecrets)")
    require(sync_repo, r"fun\s+loadCommittedSyncProfile\s*\(", "SyncRepository.loadCommittedSyncProfile")
    require(sync_repo, r"fun\s+loadSyncSecretsForGeneration\s*\(", "SyncRepository.loadSyncSecretsForGeneration")
    require(sync_repo, r"fun\s+snapshotSyncProfile\s*\(", "SyncRepository.snapshotSyncProfile")
    require(sync_repo, r"fun\s+loadSyncConfigStrict\s*\(", "SyncRepository.loadSyncConfigStrict")
    require(sync_repo, r"fun\s+loadLegacySyncSecretsTyped\s*\(", "SyncRepository.loadLegacySyncSecretsTyped")
    require(sync_repo, r"fun\s+deleteSyncSecretsForGeneration\s*\(", "SyncRepository.deleteSyncSecretsForGeneration")
    require(sync_repo, r"fun\s+setSyncSecretsOverrideStrict\s*\(", "SyncRepository.setSyncSecretsOverrideStrict")
    require(sync_repo, r"fun\s+clearSyncSecretsOverride\s*\(", "SyncRepository.clearSyncSecretsOverride")

    gate = APP_SRC / "feature" / "sync" / "data" / "SyncProfileGate.kt"
    require(gate, r"\bcommitExclusive\s*\(", "SyncProfileGate.commitExclusive")
    require(gate, r"\bsnapshotExclusive\s*\(", "SyncProfileGate.snapshotExclusive")

    sync_bridge = APP_SRC / "feature" / "sync" / "data" / "interop" / "SyncBridge.kt"
    require(sync_bridge, r"fun\s+clearSyncSecretsOverride\s*\(", "SyncBridge.clearSyncSecretsOverride")

    engine = APP_SRC / "feature" / "editor" / "visual" / "AndroidTextAnimationEngine.kt"
    require(engine, r"fun\s+submitCursorOnlyTransaction\s*\(", "AndroidTextAnimationEngine.submitCursorOnlyTransaction")

    # #602 目录重构：SujianEditorView 在 feature/editor/platform。
    view = APP_SRC / "feature" / "editor" / "platform" / "SujianEditorView.kt"
    require(view, r"fun\s+setKernelAnimationEnabled\s*\(", "SujianEditorView.setKernelAnimationEnabled(Boolean)")

    frame_input = APP_SRC / "feature" / "editor" / "pipeline" / "FrameRenderInput.kt"
    require(frame_input, r"cursorTransition\b", "FrameRenderInput.cursorTransition 字段（与文字事务解耦）")

    preview = APP_SRC / "feature" / "editor" / "projection" / "ChapterPreviewState.kt"
    if preview.exists():
        for lineno, raw in enumerate(effective_lines(preview.read_text(encoding="utf-8")), 1):
            lowered = raw.lower()
            if any(
                token in lowered
                for token in ("animation", "visualruntime", "bitmap")
            ) and re.search(r"\bval\s+\w+", effective):
                findings.append(
                    Finding(
                        str(preview.relative_to(APP_SRC)),
                        lineno,
                        "ChapterPreviewState 不得持有动画引擎/Bitmap/VisualRuntime 相关字段",
                    )
                )
    # #617 评论一：ProjectViewModel 不得复活 AndroidViewModel/Application 依赖 —
    # Navigation 3 NavEntry 级 ViewModelStoreOwner 的 CreationExtras 不保证
    # APPLICATION_KEY，该依赖曾在进入章节树时直接崩溃。
    project_vm = APP_SRC / "feature" / "project" / "ui" / "ProjectViewModel.kt"
    forbid(
        project_vm,
        r"AndroidViewModel|android\.app\.Application",
        "ProjectViewModel 依赖 AndroidViewModel/Application（#617 评论一，Navigation 3 崩溃根因）",
    )
    # #617 评论六：应用根不得读取/收集整份本地设置 — 窗口层只认
    # immersiveFullscreenEnabled 这一位；根部 collectAsState 整份 LocalSettings
    # （或为此在根调 getLocalSettings）会让其它本地设置变化无谓触发根部重组。
    sujian_app = APP_SRC / "app" / "SujianApp.kt"
    forbid(
        sujian_app,
        r"localSettingsState|getLocalSettings\s*\(",
        "SujianApp 根读取/收集整份本地设置（#617 评论六：根部只认 immersiveFullscreenEnabled 这一位）",
    )

    return findings


PACKAGE_RE = re.compile(r"^\s*package\s+(com\.xiwei\.sujian(?:\.[A-Za-z0-9_]+)*)\s*$", re.M)


def is_kotlin_file(path: Path) -> bool:
    return path.suffix == ".kt"


def rule_package_dir_consistent(
    source_roots: tuple[Path, ...] | None = None,
) -> list[Finding]:
    """package 声明必须与物理目录结构一致（#602 评论#7 项13、评论#8 项8.2）。

    遍历所有 source root（main/debug/release/test/androidTest/testAi/androidTestAi）
    下 com/xiwei/sujian 基目录中的 .kt 文件（排除 build/generated），解析首条
    package 声明，将包名 com.xiwei.sujian.xxx.yyy 转为目录 xxx/yyy，与文件相对
    该基目录的父目录比较，不一致则报错。

    不同 source root 的基包前缀都是 com/xiwei/sujian，因此相对目录比较统一算到
    这一层。不存在的 source root 跳过。可选参数 source_roots 供测试注入临时根，
    默认使用模块级 PACKAGE_SOURCE_ROOTS（不破坏现有 configure 机制）。
    """
    findings: list[Finding] = []
    roots = source_roots if source_roots is not None else PACKAGE_SOURCE_ROOTS
    prefix = "com.xiwei.sujian"
    base_subpath = Path("com") / "xiwei" / "sujian"
    for source_root in roots:
        if not source_root.exists():
            continue
        base_dir = source_root / base_subpath
        if not base_dir.exists():
            continue
        for path in sorted(base_dir.rglob("*.kt")):
            if not is_kotlin_file(path) or not path.is_file():
                continue
            str_path = str(path)
            if "/build/" in str_path or "/generated/" in str_path:
                continue
            content = path.read_text(encoding="utf-8")
            match = PACKAGE_RE.search(content)
            if not match:
                continue
            pkg = match.group(1)
            if pkg == prefix:
                pkg_subpath = ""
            else:
                pkg_subpath = pkg[len(prefix) + 1 :].replace(".", "/")
            rel_dir = path.parent.relative_to(base_dir)
            rel_dir_str = str(rel_dir).replace("\\", "/")
            if rel_dir_str == ".":
                rel_dir_str = ""
            if pkg_subpath != rel_dir_str:
                findings.append(
                    Finding(
                        path=str(path.relative_to(source_root)),
                        line=0,
                        message=f"package '{pkg}' 与目录 '{rel_dir_str}' 不一致",
                    )
                )
    return findings



PRESENTATION_CONTRACT_DTO_WHITELIST = [
    "uniffi.writer_core.WindowCapabilitiesDto",
    "uniffi.writer_core.LayoutContractDto",
    "uniffi.writer_core.ScreenPolicyDto",
    "uniffi.writer_core.ScreenRoleDto",
    "uniffi.writer_core.ActionSlotDto",
    "uniffi.writer_core.ActionRoleDto",
    "uniffi.writer_core.ActionTargetDto",
    "uniffi.writer_core.ActionRegionDto",
    "uniffi.writer_core.PointerClassDto",
    "uniffi.writer_core.WorkspacePaneModeDto",
]


def rule_presentation_contract_layer() -> list[Finding]:
    """#610：app/presentation 是 Core presentation contract 的 Android 映射层。

    - 只允许引用白名单内的 uniffi contract DTO（平台控件/编辑器 DTO 不得进入）；
    - AppServiceBridge/AppServiceProvider 只允许出现在 PresentationContractBridge.kt
      （唯一入口）；AndroidChromePolicy / AndroidAdaptiveLayoutPolicy 不得直接依赖 Bridge；
    - 旧的 ScreenPolicyModels/LayoutPolicyModels/双份 UI policy 已删除，不得复活。
    """
    findings: list[Finding] = []
    for path in collect_kt_files(APP_SRC, "/sujian/app/presentation/"):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            # uniffi.writer_core 只允许白名单 DTO
            if "uniffi.writer_core" in raw and not any(
                allowed in raw for allowed in PRESENTATION_CONTRACT_DTO_WHITELIST
            ):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message=f"presentation 层引用非契约 DTO: {raw.strip()}",
                    )
                )
            # Bridge 依赖只允许出现在唯一入口 PresentationContractBridge.kt
            if any(
                b in raw
                for b in [
                    "com.xiwei.sujian.app.di.AppServiceProvider",
                    "com.xiwei.sujian.core.interop.app.AppServiceBridge",
                ]
            ) and not path.name == "PresentationContractBridge.kt":
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="presentation 层只有 PresentationContractBridge.kt 可以依赖 Bridge",
                    )
                )
    # 旧的双份 UI policy 不得复活
    for path in collect_kt_files(APP_SRC, None):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            if re.search(r"\bSujianChromePolicy\b", raw):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="SujianChromePolicy 必须保持删除（#610：AndroidChromePolicy 消费 Core ActionSlot）",
                    )
                )
    return findings

RULES: list[tuple[str, str, object]] = [
    (
        "ui-no-uniffi-jna-bridge",
        "UI 层不能直接依赖具体 Bridge / UniFFI / JNA",
        rule_ui_no_uniffi_jna_bridge,
    ),
    (
        "ui-no-editor-input",
        "UI 层不能直接依赖 feature/editor/input 基础设施",
        rule_ui_no_editor_input,
    ),
    (
        "data-no-ui-framework",
        "data 层（Bridge/Repository）不能依赖 Compose/Activity/View/UI",
        rule_data_no_ui_framework,
    ),
    (
        "data-no-editor-display",
        "data 层不能依赖 editor 显示/动画状态",
        rule_data_no_editor_display,
    ),
    (
        "input-layer-pure",
        "feature/editor/input 只产生输入操作，不依赖 Repository/UI/UniFFI（DTO 契约除外）",
        rule_input_layer_pure,
    ),
    (
        "visual-motion-pure",
        "visual/motion 只处理显示和动画状态，不写正文持久状态",
        rule_visual_motion_pure,
    ),
    (
        "session-layer-no-platform-state",
        "editor session 层不能依赖 Compose 可变状态/View/Activity",
        rule_session_layer_no_platform_state,
    ),
    (
        "frame-clock-window-owned",
        "FrameClock 只能由窗口/显示层持有",
        rule_frame_clock_window_owned,
    ),
    (
        "update-session-state-transform-purity",
        "updateSessionState transform 是纯函数（transform 内不写 store）",
        rule_transform_purity,
    ),
    (
        "designsystem-independence",
        "core/designsystem 不能反向依赖 app 模块",
        rule_designsystem_independent,
    ),
    (
        "deleted-types-stay-deleted",
        "已删除类型/入口不得复活",
        rule_deleted_types_stay_deleted,
    ),
    (
        "source-contracts",
        "关键结构契约（session 状态出口/FrameClock/motion policy/同步事务/凭据/预览纯净）",
        rule_source_contracts,
    ),
    (
        "package-dir-consistent",
        "package 声明必须与物理目录结构一致",
        rule_package_dir_consistent,
    ),
    (
        "presentation-contract-layer",
        "app/presentation 只消费 Core presentation contract DTO（#610）",
        rule_presentation_contract_layer,
    ),
]


def configure(
    app_src: Path,
    designsystem_src: Path,
    designsystem_module: Path | None = None,
    platform_module: Path | None = None,
) -> None:
    """设置扫描根目录（默认指向真实仓库，测试可指向临时夹具树）。"""
    global APP_SRC, DS_SRC, DS_MODULE, PLATFORM_MODULE, PACKAGE_SOURCE_ROOTS
    APP_SRC = app_src
    DS_SRC = designsystem_src
    if designsystem_module is not None:
        DS_MODULE = designsystem_module
    if platform_module is not None:
        PLATFORM_MODULE = platform_module
    app_module = app_src.parents[5]
    PACKAGE_SOURCE_ROOTS = package_source_roots_from_modules(
        (app_module, DS_MODULE, PLATFORM_MODULE)
    )


def run_checks() -> tuple[list[Finding], dict[str, list[Finding]]]:
    """运行全部规则，返回 (全部违规, 按规则分类的违规)。"""
    all_findings: list[Finding] = []
    by_rule: dict[str, list[Finding]] = {}
    for rule_id, title, checker in RULES:
        findings = sorted(
            checker(),  # type: ignore[operator]
            key=lambda f: (f.path, f.line),
        )
        by_rule[rule_id] = findings
        all_findings.extend(findings)
        print(f"[{rule_id}] {title}: {'通过' if not findings else f'{len(findings)} 处违规'}")
    return all_findings, by_rule


def main() -> int:
    parser = argparse.ArgumentParser(description="Android 分层架构源码扫描器（#597 六）")
    parser.add_argument(
        "--app-src",
        type=Path,
        default=DEFAULT_APP_SRC,
        help="app 模块主源码根目录（默认 %(default)s）",
    )
    parser.add_argument(
        "--designsystem-src",
        type=Path,
        default=DEFAULT_DS_SRC,
        help="core/designsystem 主源码根目录（默认 %(default)s）",
    )
    parser.add_argument(
        "--designsystem-module",
        type=Path,
        default=DEFAULT_DS_MODULE,
        help="core/designsystem 模块根目录（默认 %(default)s）",
    )
    parser.add_argument(
        "--platform-module",
        type=Path,
        default=DEFAULT_PLATFORM_MODULE,
        help="core/platform 模块根目录（默认 %(default)s）",
    )
    args = parser.parse_args()

    if not args.app_src.exists():
        print(f"错误: app 主源码根目录不存在: {args.app_src}", file=sys.stderr)
        return 1
    if not args.designsystem_src.exists():
        print(f"错误: core/designsystem 主源码根目录不存在: {args.designsystem_src}", file=sys.stderr)
        return 1

    configure(
        args.app_src,
        args.designsystem_src,
        args.designsystem_module,
        args.platform_module,
    )
    all_findings, by_rule = run_checks()
    if not all_findings:
        print("\n全部架构规则通过。")
        return 0

    print("\n===== 违规报告 =====")
    for rule_id, findings in by_rule.items():
        if not findings:
            continue
        print(f"\n--- {rule_id} ---")
        for f in findings:
            location = f"{f.path}:{f.line}" if f.line else f.path
            print(f"  {location}: {f.message}")
    print(f"\n共 {len(all_findings)} 处违规。", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
