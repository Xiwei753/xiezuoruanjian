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
8.  session 的 state/store/epoch 写入只能从 SessionMutationGate/mutateSession
    进入（updateSessionState 已删除）；store.put/store.update/store.remove
    只在 SessionMutationScope 闭包内（锁保护）执行；
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
    # #641：WritingPaneRoute 创建 EditorTextFieldStateBridge，commitToCore lambda
    # 经 TextEditSessionBridge 调 Rust — bridge 接线需要 AppServiceBridge +
    # EditorTransactionCauseDto。后续可移入 EditorViewModel 进一步收窄。
    "feature/editor/ui/WritingPaneRoute.kt",
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
    # #641：WritingEditorSurface 消费 EditorTextFieldStateBridge，
    # WritingPaneRoute 创建 bridge — 允许这两个文件引用 input 层。
    _641_input_exemptions = {
        "feature/editor/ui/WritingEditorSurface.kt",
        "feature/editor/ui/WritingPaneRoute.kt",
    }
    findings = [f for f in findings if f.path not in _641_input_exemptions]
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
    # #641：EditorTextFieldStateBridge 持有 TextFieldState（androidx.compose.foundation.text.input）
    # 和 TextRange（androidx.compose.ui.text）— 这是系统实时输入状态，允许 Compose 依赖。
    findings = [
        f for f in findings
        if not (f.path == "feature/editor/input/EditorTextFieldStateBridge.kt")
    ]
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
        # #641：ComposeEditorVisualState / ComposeTextAnimationOverlay 是 Compose 显示层 —
        # 只消费 TextLayoutResult 做显示，不写正文持久状态。允许 Compose UI 依赖。
        _641_visual_exemptions = {
            "feature/editor/visual/ComposeEditorVisualState.kt",
            "feature/editor/visual/ComposeTextAnimationOverlay.kt",
        }
        findings = [f for f in findings if f.path not in _641_visual_exemptions]
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
    # #641：Window/View 层拆干净 — EditorWindowHost 不再持有 View/FrameClock/
    # presentationReady，只做 session 协调。检查旧成员没有复活。
    window_host = APP_SRC / "feature" / "editor" / "window" / "EditorWindowHost.kt"
    if window_host.exists():
        effective = "\n".join(
            effective_lines(window_host.read_text(encoding="utf-8"))
        )
        for forbidden_member in (
            "windowFrameClock",
            "sharedEditorView",
            "presentationReady",
            "createWindowView",
            "attachView",
            "detachView",
            "updateView",
            "PresentationReadinessGate",
        ):
            if forbidden_member in effective:
                findings.append(
                    Finding(
                        path="feature/editor/window/EditorWindowHost.kt",
                        line=0,
                        message=f"#641：EditorWindowHost 不得保留旧 View/FrameClock 成员 {forbidden_member}",
                    )
                )
    return findings


def rule_session_mutation_gate_only() -> list[Finding]:
    """session 的 state/store/epoch 写入只能从 SessionMutationGate/mutateSession 进入。

    #624 评论17 问题3：updateSessionState 入口已删除。session 的 state/store/epoch
    写入只走 mutateSession 单一临界区（SessionMutationScope 内）。
    """
    findings: list[Finding] = []
    session_dir = APP_SRC / "feature" / "editor" / "session"
    if not session_dir.exists():
        return findings
    sources = "\n".join(
        p.read_text(encoding="utf-8")
        for p in sorted(session_dir.glob("*.kt"))
    )
    # 1. updateSessionState 不得作为函数定义或调用出现在 session 生产代码中（已删除）。
    #    注释中提及 updateSessionState（说明已删除）是允许的。
    if re.search(r"\bfun\s+updateSessionState\b", sources) or re.search(r"\bupdateSessionState\s*[\{(]", sources):
        findings.append(
            Finding(
                path="feature/editor/session",
                line=0,
                message=(
                    "updateSessionState 必须删除 — session 的 state/store/epoch 写入"
                    "只走 mutateSession 单一临界区（#624 评论17 问题3）"
                ),
            )
        )
    # 2. mutateSession 必须存在。
    if "mutateSession" not in sources:
        findings.append(
            Finding(
                path="feature/editor/session",
                line=0,
                message="必须存在 mutateSession 单一临界区入口（SessionMutationGate 锁保护）",
            )
        )
    # 3. _sessionStateFlow.value = 只允许出现在 EditorSessionCoordinator.kt
    #    （mutateSession 内）和 SessionMutationScope.kt — 不得在其他 session .kt
    #    文件中直接写 _sessionStateFlow.value。
    for kt in sorted(session_dir.glob("*.kt")):
        if kt.name in ("EditorSessionCoordinator.kt", "SessionMutationScope.kt"):
            continue
        text = kt.read_text(encoding="utf-8")
        if "_sessionStateFlow.value =" in text or "_sessionStateFlow.value=" in text:
            findings.append(
                Finding(
                    path=f"feature/editor/session/{kt.name}",
                    line=0,
                    message=(
                        "_sessionStateFlow.value 直接赋值只允许在 "
                        "EditorSessionCoordinator.mutateSession 内 — "
                        "其他位置必须走 mutateSession 闭包（#624 评论17 问题1/3）"
                    ),
                )
            )
    # 4. inputLeaseEpoch 赋值只允许出现在 EditorSessionCoordinator.kt
    #    （mutateSession/invalidateInputLease 内）和 SessionMutationScope.kt。
    for kt in sorted(session_dir.glob("*.kt")):
        if kt.name in ("EditorSessionCoordinator.kt", "SessionMutationScope.kt"):
            continue
        text = kt.read_text(encoding="utf-8")
        if "inputLeaseEpoch =" in text or "inputLeaseEpoch=" in text:
            findings.append(
                Finding(
                    path=f"feature/editor/session/{kt.name}",
                    line=0,
                    message=(
                        "inputLeaseEpoch 赋值只允许在 EditorSessionCoordinator.mutateSession/"
                        "invalidateInputLease 内 — 其他位置必须走 mutateSession 闭包"
                        "（#624 评论17 问题1/3）"
                    ),
                )
            )
    # #624 评论17 问题1/6：readSession 必须存在（与 mutateSession 共用 mutationLock 的只读 gateway）。
    if "readSession" not in sources:
        findings.append(
            Finding(
                path="feature/editor/session",
                line=0,
                message="必须存在 readSession 只读临界区入口（与 mutateSession 共用 mutationLock）",
            )
        )
    # #624 评论17 问题1/6：生产 session 代码不得在 readSession/mutateSession 之外直接读写
    # EditorSessionStore。store.record/allRecords/isRegistered 只允许出现在
    # SessionMutationScope.kt（scope 方法实现）。EditorSessionCoordinator.kt 构造 scope
    # 时传 store 引用合法，但不得直接调 store.record。
    for kt in sorted(session_dir.glob("*.kt")):
        if kt.name == "SessionMutationScope.kt":
            continue
        text = kt.read_text(encoding="utf-8")
        for call in ("store.record(", "store.allRecords(", "store.isRegistered("):
            if call in text:
                findings.append(
                    Finding(
                        path=f"feature/editor/session/{kt.name}",
                        line=0,
                        message=(
                            f"生产 session 代码不得直接调用 {call} — 必须通过 "
                            "readSession/mutateSession 闭包内的 scope 访问 EditorSessionStore"
                            "（#624 评论17 问题1/6）"
                        ),
                    )
                )
    # #624 评论17 问题5/6：SessionMutationScope 不得持有 EditorSessionCoordinator / Core bridge。
    scope_path = session_dir / "SessionMutationScope.kt"
    if scope_path.exists():
        scope_text = scope_path.read_text(encoding="utf-8")
        if "coordinator: EditorSessionCoordinator" in scope_text or "val coordinator" in scope_text:
            findings.append(
                Finding(
                    path="feature/editor/session/SessionMutationScope.kt",
                    line=0,
                    message=(
                        "SessionMutationScope 不得持有 EditorSessionCoordinator / Core bridge — "
                        "Core 调用统一在 mutation/read snapshot 之外（#624 评论17 问题5/6）"
                    ),
                )
            )
    # #624 评论17 问题5/6：markSaved(targetId, ...) 不得在生产 session 代码重新出现。
    for kt in sorted(session_dir.glob("*.kt")):
        text = kt.read_text(encoding="utf-8")
        if re.search(r"\bfun\s+EditorSessionCoordinator\.markSaved\s*\(", text):
            findings.append(
                Finding(
                    path=f"feature/editor/session/{kt.name}",
                    line=0,
                    message=(
                        "markSaved(targetId, ...) 必须保持删除 — 正常保存和切章保存都走 "
                        "commitSavedLease（#624 评论17 问题5/6）"
                    ),
                )
            )
    # #624 评论17 问题2/6：completeWindowAttach 必须返回 Boolean（窗口绑定判断+提交同一次 mutation）。
    lifecycle_path = session_dir / "EditorSessionLifecycleOps.kt"
    if lifecycle_path.exists():
        lifecycle_text = lifecycle_path.read_text(encoding="utf-8")
        if not re.search(r"fun\s+EditorSessionCoordinator\.completeWindowAttach\s*\([^)]*\)\s*:\s*Boolean", lifecycle_text):
            findings.append(
                Finding(
                    path="feature/editor/session/EditorSessionLifecycleOps.kt",
                    line=0,
                    message=(
                        "completeWindowAttach 必须返回 Boolean — 单次 mutateSession 内校验 "
                        "isExactAttaching 才写 Attached，旧窗口晚到返回 false（#624 评论17 问题2/6）"
                    ),
                )
            )
    return findings


def _extract_member_function_body(
    text: str,
    func_name: str,
) -> tuple[int, int, list[str]] | None:
    """提取 `fun EditorSessionCoordinator.<func_name>(...)` 的函数体行。

    返回 (start_lineno_1based, end_lineno_1based, body_lines)。找不到返回 None。
    大括号跟踪在去注释后的文本上进行（块注释 re.sub 去掉，行注释 strip_line_comment 去掉），
    不解析字符串字面量 — 对目标函数体内无字符串大括号干扰足够稳健。
    """
    cleaned = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    lines = cleaned.splitlines()
    pattern = re.compile(
        rf"\bfun\s+EditorSessionCoordinator\.{re.escape(func_name)}\s*\("
    )
    start_idx = None
    for i, line in enumerate(lines):
        if pattern.search(strip_line_comment(line)):
            start_idx = i
            break
    if start_idx is None:
        return None
    depth = 0
    in_body = False
    for j in range(start_idx, len(lines)):
        line = strip_line_comment(lines[j])
        for ch in line:
            if ch == "{":
                depth += 1
                in_body = True
            elif ch == "}":
                depth -= 1
        if in_body and depth == 0:
            return (start_idx + 1, j + 1, lines[start_idx : j + 1])
    return None


def rule_session_close_before_claim() -> list[Finding]:
    """#624 评论5294575627 要求5：close-before-claim 防回退。

    1. closeTarget / detachWindowBinding / releaseHost：closeSession 直接调用必须在
       mutateSession 闭包之后（先认领再 close）。
    2. releaseHost 不得出现 recordsToClose 旧模式（readSession{allRecords()} → 锁外 forEach
       closeSession → mutateSession clearRecords）。
    3. resetPersistentSession 不得出现 updateRecord(targetId) { it.copy(sessionId = 0UL) }
       旧路径 — 必须走 commitResetSnapshot CAS。
    4. commitPreparedBindingState 必须返回 Boolean（bind precondition CAS）。
    """
    findings: list[Finding] = []
    session_dir = APP_SRC / "feature" / "editor" / "session"
    lifecycle_path = session_dir / "EditorSessionLifecycleOps.kt"
    if not lifecycle_path.exists():
        return findings
    text = lifecycle_path.read_text(encoding="utf-8")
    rel = "feature/editor/session/EditorSessionLifecycleOps.kt"

    # 1. closeTarget / detachWindowBinding / releaseHost：closeSession 直接调用必须在
    #    mutateSession 闭包之后（先认领再 close）。
    for func_name in ("closeTarget", "detachWindowBinding", "releaseHost", "commitActiveSession", "cancelActiveSession"):
        body_info = _extract_member_function_body(text, func_name)
        if body_info is None:
            findings.append(
                Finding(
                    path=rel,
                    line=0,
                    message=(
                        f"缺少 EditorSessionCoordinator.{func_name} — "
                        "close-before-claim 检查无法进行（#624 评论5294575627 要求5）"
                    ),
                )
            )
            continue
        start_lineno, _, body_lines = body_info
        mutate_session_rel: int | None = None
        for r, line in enumerate(body_lines):
            if "mutateSession" in strip_line_comment(line):
                mutate_session_rel = r
                break
        if mutate_session_rel is None:
            continue
        for r, line in enumerate(body_lines):
            stripped = strip_line_comment(line)
            if "closeSession(" in stripped and r < mutate_session_rel:
                findings.append(
                    Finding(
                        path=rel,
                        line=start_lineno + r,
                        message=(
                            f"{func_name}: closeSession 必须在 mutateSession 认领之后调用"
                            "（close-before-claim，#624 评论5294575627 要求1/5）"
                        ),
                    )
                )

    # 2. releaseHost 不得出现 recordsToClose 旧模式。
    body_info = _extract_member_function_body(text, "releaseHost")
    if body_info is not None:
        start_lineno, _, body_lines = body_info
        for r, line in enumerate(body_lines):
            if "recordsToClose" in strip_line_comment(line):
                findings.append(
                    Finding(
                        path=rel,
                        line=start_lineno + r,
                        message=(
                            "releaseHost 不得使用 recordsToClose 旧模式 — 必须单次 "
                            "mutateSession 收集 sessionId 再锁外 close"
                            "（#624 评论5294575627 要求2/5）"
                        ),
                    )
                )

    # 3. resetPersistentSession 不得出现 updateRecord(targetId) { it.copy(sessionId = 0UL) } 旧路径。
    body_info = _extract_member_function_body(text, "resetPersistentSession")
    if body_info is not None:
        start_lineno, _, body_lines = body_info
        body_text = "\n".join(body_lines)
        if re.search(
            r"updateRecord\s*\(\s*targetId\s*\)\s*\{\s*it\.copy\s*\(\s*sessionId\s*=\s*0UL\s*\)\s*\}",
            body_text,
        ):
            findings.append(
                Finding(
                    path=rel,
                    line=start_lineno,
                    message=(
                        "resetPersistentSession 不得出现 updateRecord(targetId) { it.copy(sessionId = 0UL) } "
                        "旧路径 — 必须走 commitResetSnapshot CAS（#624 评论5294575627 要求4/5）"
                    ),
                )
            )

    # 4. commitPreparedBindingState 必须返回 Boolean（bind precondition CAS）。
    if not re.search(
        r"fun\s+EditorSessionCoordinator\.commitPreparedBindingState\s*\([^)]*\)\s*:\s*Boolean",
        text,
    ):
        findings.append(
            Finding(
                path=rel,
                line=0,
                message=(
                    "commitPreparedBindingState 必须返回 Boolean — bind precondition CAS"
                    "（#624 评论5294575627 要求3/5）"
                ),
            )
        )

    # 5. #624 评论5294575627 要求4/5：旧入口不得重新出现。
    for forbidden_func in ("prepareActiveSessionIfCurrent", "restampAttachingToWindow", "clearWindowAttach"):
        if re.search(rf"\bfun\s+EditorSessionCoordinator\.{forbidden_func}\s*\(", text):
            findings.append(
                Finding(
                    path=rel,
                    line=0,
                    message=(
                        f"{forbidden_func} 必须保持删除 — 统一走 bind precondition CAS"
                        "（#624 评论5294575627 要求2/4/5）"
                    ),
                )
            )

    # 6. #624 评论5294575627 要求3/5：prepareSessionForEdit 失败时不得 removeRecord(targetId)。
    body_info = _extract_member_function_body(text, "prepareSessionForEdit")
    if body_info is not None:
        start_lineno, _, body_lines = body_info
        for r, line in enumerate(body_lines):
            stripped = strip_line_comment(line)
            if "removeRecord(targetId)" in stripped:
                findings.append(
                    Finding(
                        path=rel,
                        line=start_lineno + r,
                        message=(
                            "prepareSessionForEdit 失败时不得 removeRecord(targetId) — "
                            "失败阶段尚未认领 Kotlin 所有权，直接 return null"
                            "（#624 评论5294575627 要求3/5）"
                        ),
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
    # #617 评论八：卷/章节 UI 模型不得携带交互状态 — 展开只存在
    # expandedVolumeIds 一份真相、选中只存在 selectedChapterId 一份真相；
    # 刷新链写回与用户切换因此互不覆盖。UI 模型上复活 isExpanded/isSelected
    # 字段、或渲染方直接读 volume.isExpanded 的接线方式不得复活。
    ui_state_file = APP_SRC / "feature" / "project" / "ui" / "ProjectWorkspaceUiState.kt"
    if ui_state_file.exists():
        effective = "\n".join(effective_lines(ui_state_file.read_text(encoding="utf-8")))
        for field in (r"val\s+isExpanded\s*:", r"val\s+isSelected\s*:"):
            if re.search(field, effective):
                findings.append(
                    Finding(
                        path="feature/project/ui/ProjectWorkspaceUiState.kt",
                        line=0,
                        message="UI 模型交互状态字段必须保持删除（#617 评论八：展开只认 expandedVolumeIds、选中只认 selectedChapterId）",
                    )
                )
    tree_file = APP_SRC / "feature" / "project" / "ui" / "VolumeChapterTree.kt"
    if tree_file.exists():
        effective = "\n".join(effective_lines(tree_file.read_text(encoding="utf-8")))
        for pattern in (
            r"\.isExpanded\b",
            r"\.isSelected\b",
        ):
            if re.search(pattern, effective):
                findings.append(
                    Finding(
                        path="feature/project/ui/VolumeChapterTree.kt",
                        line=0,
                        message="渲染方不得读 UI 模型交互状态字段（#617 评论八：展开/选中从 expandedVolumeIds/selectedChapterId 派生）",
                    )
                )
    # #641：旧 View 输入/排版/viewport/FrameClock/pipeline/visual/render/layout/projection
    # 死代码闭包已删除，不得复活。
    deleted_641_types = [
        "SujianEditorView", "EditorViewportController",
        "AndroidInputAdapter", "AndroidInputConnection", "InputCursorMapper",
        "DisplayTextMirror", "DisplayTextProjection", "TextOffsetIndex",
        "AndroidEditorPipeline", "EditPipeline", "EditorCommandPort", "FrameRenderInput",
        "AndroidLayoutEngine", "AndroidLineSnapshot", "AndroidLineSnapshotBuilder",
        "AffectedLayoutRevision", "AffectedLineCapture", "AndroidLayoutRevision",
        "FirstLineIndentSpan", "ParagraphStyleProjection",
        "AndroidTextAnimationRenderer", "AndroidTextRenderer", "EditorFrameComposer",
        "AndroidTextAnimationEngine", "AndroidVisualPlanner",
        "AnimationTimeline", "AnimationTimeSource", "CaptureMethod", "ColorDistance",
        "PreparedVisualTransaction", "RebaseMappingProvider",
        "TextRevealGeometry", "TextRevealSpec", "VisualProgressWindow", "VisualResourceStore",
        "VisualTrackState",
        "AffectedLayoutPlanner", "BlockShiftPlanner", "CaretRevealPlanner",
        "InsertDeletePlanner", "MoveCrossfadePlanner", "RebasePlanner", "SnapshotPlanner",
        "WindowDisplayFrameClock",
    ]
    for path in collect_kt_files(APP_SRC, "/feature/editor/"):
        effective = "\n".join(effective_lines(path.read_text(encoding="utf-8")))
        for deleted_type in deleted_641_types:
            if re.search(rf"\b(class|interface|data class|object|enum class)\s+{deleted_type}\b", effective):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=0,
                        message=f"{deleted_type} 必须保持删除（#641：旧 View 输入/排版/viewport/pipeline 死代码闭包）",
                    )
                )
    # EditorEditSource 不得在 platform 包复活（已迁移到 session 包）
    platform_edit_source = APP_SRC / "feature" / "editor" / "platform"
    if platform_edit_source.exists():
        for path in collect_kt_files(platform_edit_source, ""):
            effective = "\n".join(effective_lines(path.read_text(encoding="utf-8")))
            if re.search(r"\b(enum class|class|object)\s+EditorEditSource\b", effective):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=0,
                        message="EditorEditSource 不得在 platform 包复活（#641：已迁移到 session 包）",
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



# #624 评论17 第1/3部分：feature/editor/presentation 是从 ui 层抽出的页面事务层
# （EditorViewModel + 拆分 ops + ChapterSwitchGate + TargetDocumentUpdateBus）。
# 它只承载 ViewModel/状态机/协程事务，不得依赖 Compose 布局/Android View/SujianEditorView
# 与 editor 显示/输入/平台/排版/渲染层。
EDITOR_PRESENTATION_FORBIDDEN = (
    COMPOSE_UI_FRAMEWORK
    + [
        "androidx.compose.runtime",  # @Composable/@Immutable 等 Compose 标记也不得进入
        "android.view",
        "com.xiwei.sujian.feature.editor.platform",
        "com.xiwei.sujian.feature.editor.render",
        "com.xiwei.sujian.feature.editor.input",
        "com.xiwei.sujian.feature.editor.layout",
        "com.xiwei.sujian.feature.editor.ui",
        "com.xiwei.sujian.feature.editor.visual",
        "com.xiwei.sujian.feature.editor.motion",
        "com.xiwei.sujian.feature.editor.window",
    ]
)


def rule_editor_presentation_pure() -> list[Finding]:
    """#624 评论17 第1/3部分：feature/editor/presentation 不得依赖
    Compose/View/editor platform/render/input/layout/ui/visual/motion/window。

    presentation 只承载 ViewModel/状态机/协程事务，Compose 表面留在 ui 层
    （WritingPane/WritingEditorSurface 等）。这条规则保护从 ui 包迁出的
    EditorViewModel/EditorViewModelTypes/Editor*Ops/ChapterSwitchGate/
    TargetDocumentUpdateBus 不会把 Compose/View/平台依赖带进新包。
    """
    return scan_forbidden(
        APP_SRC,
        "/feature/editor/presentation/",
        EDITOR_PRESENTATION_FORBIDDEN,
    )


PRESENTATION_CONTRACT_DTO_WHITELIST = [
    "uniffi.writer_core.WindowViewportDto",
    "uniffi.writer_core.WindowOcclusionDto",
    "uniffi.writer_core.LayoutContractDto",
    "uniffi.writer_core.ScreenPolicyDto",
    "uniffi.writer_core.ScreenRoleDto",
    "uniffi.writer_core.ActionSlotDto",
    "uniffi.writer_core.ActionRoleDto",
    "uniffi.writer_core.ActionTargetDto",
    "uniffi.writer_core.ActionRegionDto",
    "uniffi.writer_core.PrimaryNavigationPlacementDto",
    "uniffi.writer_core.LayoutMetricsDto",
    # #628 任务 1-3：WorkspacePaneMode 改名为 WorkspaceLayoutMode，
    # 对应 uniffi DTO 由 WorkspacePaneModeDto 改为 WorkspaceLayoutModeDto。
    "uniffi.writer_core.WorkspaceLayoutModeDto",
    "uniffi.writer_core.ShellModeDto",
    # #628 评论 5301021120 第 1-3 步：Workbench Layout Plan DTOs
    # （删除单数 workbench_occlusion，改为 WorkbenchLayoutPlan 含七角色 bounds）。
    "uniffi.writer_core.WorkbenchLayoutPlanDto",
    "uniffi.writer_core.WorkbenchPlacementDto",
    "uniffi.writer_core.WorkbenchVisibilityDto",
    "uniffi.writer_core.WorkbenchRoleDto",
    "uniffi.writer_core.LayoutRectDto",
    # #628 评论 5301021120 02:59:39Z 版：WorkbenchLayoutPlan 的 valid: bool 改由
    # ResolvedWorkspaceModeDto（Workbench/SinglePane）表达 Rust 决定的最终产品模式。
    "uniffi.writer_core.ResolvedWorkspaceModeDto",
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
            # Bridge 依赖只允许出现在唯一入口
            # app/presentation/contract/PresentationContractBridge.kt（#628：文件
            # 已迁入 contract/ 子目录，按相对路径精确匹配，不再按文件名匹配）。
            if any(
                b in raw
                for b in [
                    "com.xiwei.sujian.app.di.AppServiceProvider",
                    "com.xiwei.sujian.core.interop.app.AppServiceBridge",
                ]
            ) and not str(path.relative_to(APP_SRC)) == "app/presentation/contract/PresentationContractBridge.kt":
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="presentation 层只有 app/presentation/contract/PresentationContractBridge.kt 可以依赖 Bridge",
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


def rule_presentation_layout_no_breakpoints() -> list[Finding]:
    """#628：app/presentation/layout/ 不得出现 WindowWidthSizeClass /
    availablePaneCount / windowWidthSizeClass 这类布局断点判断，也不得硬编码
    600/840/1200/1600 窗口断点比较。

    布局断点已收回 Rust presentation/layout/breakpoints.rs，Android layout 层
    只消费 LayoutContractDto。
    """
    forbidden = [
        "WindowWidthSizeClass",
        "availablePaneCount",
        "windowWidthSizeClass",
    ]
    findings = scan_forbidden(APP_SRC, "/sujian/app/presentation/layout/", forbidden)
    # 数字断点 600/840/1200/1600 容易误报（注释、其他数值），用更精确的匹配：
    # 仅匹配形如 `>= 600` / `== 840` / `1200 >` 这类断点比较。
    for path in collect_kt_files(APP_SRC, "/sujian/app/presentation/layout/"):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            if re.search(r"[<>=!]=?\s*(600|840|1200|1600)\b", raw) or re.search(
                r"\b(600|840|1200|1600)\s*[<>=]", raw
            ):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message="presentation/layout/ 不得硬编码窗口断点 600/840/1200/1600（已收回 Rust breakpoints.rs）",
                    )
                )
    return findings


def rule_ui_no_direct_layout_decision() -> list[Finding]:
    """#628：feature/*/ui 和 app/navigation 下不得直接根据窗口尺寸决定 pane 数
    或 Bottom/Side；必须消费 LayoutContractDto。

    navigation 层可以读 LayoutContractDto.primaryNavigationPlacement（通过 DTO
    字段），但不应自己判断窗口尺寸来决定 Bottom/Side，因此只禁
    WindowWidthSizeClass / availablePaneCount / windowWidthSizeClass。
    """
    ui_filters = ["/sujian/app/navigation/"] + _feature_ui_filters()
    forbidden = [
        "WindowWidthSizeClass",
        "availablePaneCount",
        "windowWidthSizeClass",
    ]
    findings: list[Finding] = []
    for f in ui_filters:
        findings += scan_forbidden(APP_SRC, f, forbidden)
    return findings


# #628 验收点 7：跨端产品结构宽度（project_card / tool_pane / tool_rail）已收回
# Rust presentation/layout/metrics（LayoutMetrics），feature/*/ui 只做 .dp 映射，
# 不得重新硬编码 180.dp / 240.dp / 56.dp。只扫 feature/*/ui/，不扫 designsystem
# （Material 控件皮肤允许有自己的尺寸 token）。
STRUCTURAL_DIMENSION_LITERALS = ["180.dp", "240.dp", "56.dp"]


def rule_presentation_layout_no_structural_dimensions() -> list[Finding]:
    """#628 验收点 7：feature/*/ui 不得重新硬编码跨端产品结构宽度
    180.dp / 240.dp / 56.dp（project_card / tool_pane / tool_rail）。

    这些结构尺寸必须来自 Rust LayoutMetrics（Core presentation/layout/metrics
    决定），Android 只做 .dp 映射。只扫 feature/*/ui/，不扫 designsystem
    （Material 控件皮肤允许有尺寸）。
    """
    findings: list[Finding] = []
    for f in _feature_ui_filters():
        for path in collect_kt_files(APP_SRC, f):
            for lineno, raw in enumerate(
                effective_lines(path.read_text(encoding="utf-8")), 1
            ):
                for hit in references(raw, STRUCTURAL_DIMENSION_LITERALS):
                    findings.append(
                        Finding(
                            path=str(path.relative_to(APP_SRC)),
                            line=lineno,
                            message=(
                                f"feature/*/ui 不得硬编码跨端结构宽度 {hit}"
                                "（#628 验收点 7：必须来自 Rust LayoutMetrics）"
                            ),
                        )
                    )
    return findings


def _android_layout_adapter_path() -> Path:
    """AndroidLayoutAdapter.kt 的位置（#628：app/presentation/layout/）。"""
    return APP_SRC / "app" / "presentation" / "layout" / "AndroidLayoutAdapter.kt"


# #628 验收点 7：Material3 PaneScaffoldDirective / calculatePaneScaffoldDirective /
# maxHorizontalPartitionsFor 整条死链已删除（无消费者），不得复活。
PANE_SCAFFOLD_DIRECTIVE_DEAD_CHAIN = [
    "PaneScaffoldDirective",
    "calculatePaneScaffoldDirective",
    "maxHorizontalPartitionsFor",
]


def rule_presentation_layout_no_pane_scaffold_directive() -> list[Finding]:
    """#628 验收点 7：AndroidLayoutAdapter.kt 不得重新出现 Material3
    PaneScaffoldDirective 整条死链。

    PaneScaffoldDirective / calculatePaneScaffoldDirective /
    maxHorizontalPartitionsFor 已删除（无消费者），不得复活。注释中提及
    （说明已删除）允许，effective_lines 已去注释。
    """
    findings: list[Finding] = []
    path = _android_layout_adapter_path()
    if not path.exists():
        return findings
    for lineno, raw in enumerate(
        effective_lines(path.read_text(encoding="utf-8")), 1
    ):
        for hit in references(raw, PANE_SCAFFOLD_DIRECTIVE_DEAD_CHAIN):
            findings.append(
                Finding(
                    path=str(path.relative_to(APP_SRC)),
                    line=lineno,
                    message=(
                        f"PaneScaffoldDirective 死链必须保持删除: {hit}"
                        "（#628 验收点 7：Material3 scaffold directive 已无消费者）"
                    ),
                )
            )
    return findings


# #628 验收点 7：Android layout adapter 不得用 LocalConfiguration.screenWidthDp/
# screenHeightDp 作为共享布局输入 — 窗口尺寸改用 LocalWindowInfo.current.
# containerDpSize（Compose 1.7+ 标准方式）。
LOCAL_CONFIGURATION_SCREEN_SIZE = ["screenWidthDp", "screenHeightDp"]


def rule_presentation_layout_no_local_configuration_screen_size() -> list[Finding]:
    """#628 验收点 7：AndroidLayoutAdapter.kt 不得用
    LocalConfiguration.screenWidthDp/screenHeightDp 作为共享布局输入。

    窗口尺寸改用 LocalWindowInfo.current.containerDpSize（Compose 1.7+ 标准方式）。
    注释中提及（说明已改用）允许，effective_lines 已去注释。
    """
    findings: list[Finding] = []
    path = _android_layout_adapter_path()
    if not path.exists():
        return findings
    for lineno, raw in enumerate(
        effective_lines(path.read_text(encoding="utf-8")), 1
    ):
        for hit in references(raw, LOCAL_CONFIGURATION_SCREEN_SIZE):
            findings.append(
                Finding(
                    path=str(path.relative_to(APP_SRC)),
                    line=lineno,
                    message=(
                        f"AndroidLayoutAdapter 不得用 LocalConfiguration.{hit}"
                        " 作为共享布局输入"
                        "（#628 验收点 7：改用 LocalWindowInfo.current.containerDpSize）"
                    ),
                )
            )
    return findings


# #628 验收点 7：旧工作区产品模式 WorkspacePaneMode{SinglePane,ListDetail,ThreePane}
# 已删除（改为 WorkspaceLayoutMode{SinglePane,Workbench}），不得复活。
# ShellMode::ThreePane / ShellModeDto::ThreePane 是壳层模式，允许保留 —
# 只禁工作区产品模式 token：WorkspacePaneMode / LIST_DETAIL / THREE_PANE
# （Kotlin 枚举变体大写蛇形，与 Rust 风格 ThreePane 区分）。
LEGACY_WORKSPACE_PANE_MODE_TOKENS = ["WorkspacePaneMode", "LIST_DETAIL", "THREE_PANE"]


def rule_no_legacy_workspace_pane_mode() -> list[Finding]:
    """#628 验收点 7：旧工作区产品模式 WorkspacePaneMode{ListDetail,ThreePane}
    已删除（改为 WorkspaceLayoutMode{SinglePane,Workbench}），不得复活。

    Android main 源码不得重新引入 WorkspacePaneMode / LIST_DETAIL / THREE_PANE
    （Kotlin 枚举变体）。ShellMode::ThreePane / ShellModeDto::ThreePane 是壳层
    模式，允许保留 — 只禁工作区产品模式。
    """
    findings: list[Finding] = []
    for path in collect_kt_files(APP_SRC, None):
        for lineno, raw in enumerate(
            effective_lines(path.read_text(encoding="utf-8")), 1
        ):
            for hit in references(raw, LEGACY_WORKSPACE_PANE_MODE_TOKENS):
                findings.append(
                    Finding(
                        path=str(path.relative_to(APP_SRC)),
                        line=lineno,
                        message=(
                            f"旧工作区产品模式必须保持删除: {hit}"
                            "（#628 验收点 7：改用 WorkspaceLayoutMode{SinglePane,Workbench}）"
                        ),
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
        "session-mutation-gate-only",
        "session 的 state/store/epoch 写入只能从 mutateSession 进入（updateSessionState 已删除）",
        rule_session_mutation_gate_only,
    ),
    (
        "session-close-before-claim",
        "closeTarget/detach/releaseHost 先 mutateSession 认领再锁外 close；commitPreparedBindingState 返回 Boolean；reset 不得走 0UL 旧路径（#624 评论5294575627）",
        rule_session_close_before_claim,
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
        "editor-presentation-pure",
        "feature/editor/presentation 不得依赖 Compose/View/editor platform/render/input/layout（#624 评论17）",
        rule_editor_presentation_pure,
    ),
    (
        "presentation-contract-layer",
        "app/presentation 只消费 Core presentation contract DTO（#610）",
        rule_presentation_contract_layer,
    ),
    (
        "presentation-layout-no-breakpoints",
        "app/presentation/layout/ 不得硬编码窗口断点（#628：已收回 Rust breakpoints.rs）",
        rule_presentation_layout_no_breakpoints,
    ),
    (
        "ui-no-direct-layout-decision",
        "feature/*/ui 和 app/navigation 不得直接判断窗口尺寸决定布局（#628：必须消费 LayoutContractDto）",
        rule_ui_no_direct_layout_decision,
    ),
    (
        "presentation-layout-no-structural-dimensions",
        "feature/*/ui 不得硬编码跨端结构宽度 180/240/56.dp（#628 验收点 7：必须来自 Rust LayoutMetrics）",
        rule_presentation_layout_no_structural_dimensions,
    ),
    (
        "presentation-layout-no-pane-scaffold-directive",
        "AndroidLayoutAdapter.kt 不得重新出现 PaneScaffoldDirective 死链（#628 验收点 7）",
        rule_presentation_layout_no_pane_scaffold_directive,
    ),
    (
        "presentation-layout-no-local-configuration-screen-size",
        "AndroidLayoutAdapter.kt 不得用 LocalConfiguration.screenWidthDp/screenHeightDp（#628 验收点 7）",
        rule_presentation_layout_no_local_configuration_screen_size,
    ),
    (
        "no-legacy-workspace-pane-mode",
        "旧工作区产品模式 WorkspacePaneMode/ListDetail/ThreePane 不得重新引入（#628 验收点 7）",
        rule_no_legacy_workspace_pane_mode,
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
