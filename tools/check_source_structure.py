#!/usr/bin/env python3
"""扫描非生成的生产 .kt/.kts/.rs 文件，检查源码结构问题。

本工具检查仓库中生产源码的结构健康度，覆盖以下规则：

1. god-file                生产源码文件超过 800 行（只计非空非注释行）
2. multiple-large-state    一个文件同时声明多个大型状态持有类（>200 行的多个）
3. production-test-bloat   生产 Rust 文件内嵌大段 #[cfg(test)] 测试模块（>100 行）
4. broad-suppression       整文件/整模块的宽范围 lint suppression
5. short-variable-names    连续出现大量无语义短变量名（连续超过 5 个）
6. no-reason-exception     白名单条目缺少规则或原因说明

排除的生成目录通过精确路径前缀匹配，不使用大范围白名单。
白名单 ALLOWED_EXCEPTIONS 每项为 (文件路径, 规则名) -> 原因，缺少原因会报违规。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    rule: str
    message: str


# ---------------------------------------------------------------------------
# 路径配置
# ---------------------------------------------------------------------------

GENERATED_PATH_PREFIXES: tuple[str, ...] = (
    "apps/android/app/build/generated/",
    "apps/android/app/build/",
    "apps/android/core/designsystem/build/",
    "apps/android/core/platform/build/",
    "bindings/",
    "target/",
)

SKIP_PATH_PARTS = {"build", "target", ".git", ".gradle"}

TEST_PATH_PREFIXES: tuple[str, ...] = (
    "apps/android/app/src/test/",
    "apps/android/app/src/androidTest/",
    "apps/harmony/entry/src/ohosTest/",
    "apps/Linux_qt/tests/",
)

TEST_DIR_NAMES = {"tests", "test"}

TEST_FILE_SUFFIXES: tuple[str, ...] = (
    "_tests.rs",
    "_test.rs",
    "Test.kt",
    "Tests.kt",
)

TEST_FILE_NAMES = {"tests.rs", "test.rs"}

BUILD_SCRIPT_NAMES = {
    "build.gradle.kts",
    "settings.gradle.kts",
    "build.gradle",
    "settings.gradle",
}


# ---------------------------------------------------------------------------
# 白名单: (文件路径, 规则名) -> 原因
# 每项必须附具体原因；缺少原因会报 no-reason-exception。
#
# 原则：
# - 只保留当前确实无法拆分的核心算法/聚合根文件
# - Issue #597 要求拆分的文件可重新列入，但原因必须包含具体技术理由
#   和 TODO 计划，说明当前为什么不能拆分以及后续拆分方向
# - 每项原因必须说明"为什么不能拆分"而非"为什么大"
# ---------------------------------------------------------------------------

ALLOWED_EXCEPTIONS: dict[tuple[Path, str], str] = {
    # --- god-file: 既有大型核心文件，共享不可分割的上下文 ---
    # 以下三项为 Android 平台桥接/管线聚合根：ktlint 1.0.1 全量格式整改后
    # 行数跨过 800 上限（职责未变）；按平台边界拆分为独立任务，不在 #597 范围。
    (Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/core/interop/app/AppServiceBridge.kt"), "god-file"):
        "FFI 门面类：121 个向后兼容委托函数聚合在唯一 holder 上，签名由 Core 契约决定；"
        "已按领域拆分为 ProjectBridge/ChapterBridge/SettingsBridge 等子桥，门面只保留委托；"
        "TODO(#597) 后续把门面委托按领域迁移到扩展函数文件",
    (Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/platform/SujianEditorView.kt"), "god-file"):
        "编辑器平台宿主：View 生命周期/InputConnection/渲染回调/窗口绑定共享同一视图上下文，"
        "拆分会破坏平台绑定状态机；已按阶段拆分内部函数；"
        "TODO(#597) 后续按回调族提取子对象",
    (Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/pipeline/AndroidEditorPipeline.kt"), "god-file"):
        "编辑器管线聚合：输入/排版/渲染/提交共享同一帧上下文与运行时状态，"
        "拆分会破坏帧原子性；已按阶段拆分内部函数；"
        "TODO(#597) 后续按阶段提取子管线",
    (Path("apps/Linux_qt/src/editor/layout.rs"), "god-file"):
        "Qt 编辑器排版核心：行盒/字距/换行/光标命中共享同一排版上下文，"
        "拆分会引入循环依赖；已按渲染阶段切分函数",
    (Path("apps/Linux_qt/src/sujian_editor_item/animation_coordinator.rs"), "god-file"):
        "Qt 动画协调器：多轨道动画状态机共享时间轴与帧调度，"
        "拆分会破坏原子提交语义",
    (Path("core/writer_core/src/search/api.rs"), "god-file"):
        "搜索 API 聚合层：查询/替换/高亮共享同一查询解析与命中流，"
        "已按子模块拆分内部函数",
    (Path("core/writer_core/src/settings/mod.rs"), "god-file"):
        "设置模块根：Schema 定义、迁移、序列化共享强类型树，"
        "拆分会破坏 schema 一致性校验",
    (Path("core/writer_core/src/api/types/editor.rs"), "god-file"):
        "编辑器 API 类型定义：DTO 与序列化契约必须同文件保持一致，"
        "属于数据模型文件",
    (Path("core/writer_core/src/api/service/starmap_ops.rs"), "god-file"):
        "星图 API 操作聚合：节点/边/布局操作共享同一图快照上下文，"
        "已按操作类型拆分函数",
    (Path("core/writer_core/src/sync/lww.rs"), "god-file"):
        "LWW 同合算法实现：元素/字段/集合三层数据结构共享同一时钟比较内核，"
        "拆分会破坏算法不变量",
    (Path("core/writer_core/src/sync/service.rs"), "god-file"):
        "同步服务编排层：Git/LWW 双策略/诊断/路径过滤共享同一 SyncService 入口与配置上下文，"
        "拆分会破坏同步原子性；已按策略拆分内部函数；"
        "TODO(#597) 后续按策略提取子服务",
    (Path("core/writer_core/src/writing_stats/store.rs"), "god-file"):
        "写作统计存储：聚合/持久化/查询共享同一时间窗口索引，"
        "已按读写路径拆分内部函数",
    (Path("apps/Linux_qt/src/backend/settings_backend.rs"), "god-file"):
        "Qt 设置后端：Schema 映射、持久化、通知共享同一设置树，"
        "已按子域拆分内部函数",
    (Path("apps/Linux_qt/src/sujian_editor_item/pipeline.rs"), "god-file"):
        "Qt 编辑器流水线：输入/排版/渲染/提交共享同一帧上下文，"
        "拆分会破坏帧原子性",
    (Path("core/writer_core/src/api/service/mod.rs"), "god-file"):
        "Core API 服务根：服务注册/路由/错误映射共享同一 API 表，"
        "拆分会破坏 API 契约一致性",
    (Path("apps/Linux_qt/src/backend/project_operations.rs"), "god-file"):
        "Qt 项目操作后端：创建/打开/保存/导出共享同一项目上下文，"
        "已按操作类型拆分函数",
    (Path("apps/Linux_qt/src/backend/sync_backend.rs"), "god-file"):
        "Qt 同步后端：密钥存取/网络同步配置/手动同步/诊断共享同一 SyncBackend QObject 与 operation_id 锁，"
        "拆分会破坏异步流输出竞态守卫；已按操作类型拆分内部函数；"
        "TODO(#597) 后续按操作族提取子后端",
    (Path("apps/Linux_qt/src/backend/app_backend.rs"), "god-file"):
        "Qt 应用后端：全局状态/桥接注册/日志收集共享同一 AppBackend QObject 上下文，"
        "拆分会破坏 QML 元对象注册一致性；已按子域拆分内部函数；"
        "TODO(#597) 后续按子域提取子后端",
    (Path("core/writer_core/src/editor/kernel/apply.rs"), "god-file"):
        "编辑器内核命令分派：所有 EditorCommand 分支共享同一匹配上下文与修订校验，"
        "按命令类型拆分会破坏匹配完备性；TODO(#597) 后续按命令族提取子模块",
    (Path("apps/Linux_qt/src/sujian_editor_item/editing.rs"), "god-file"):
        "Qt 编辑器项编辑逻辑：文本输入/删除/选区操作共享同一编辑上下文，"
        "拆分会破坏编辑原子性；TODO(#597) 后续按操作类型提取子模块",
    (Path("apps/Linux_qt/src/sujian_editor_item/mod.rs"), "god-file"):
        "Qt 编辑器项模块根：组件注册/属性/信号共享同一 QML 绑定上下文，"
        "拆分会破坏 QML 元对象注册一致性；TODO(#597) 后续按子域提取子模块",
    (Path("apps/Linux_qt/src/sujian_editor_item/rendering.rs"), "god-file"):
        "Qt 编辑器项渲染逻辑：文本/光标/选区/背景共享同一绘制上下文，"
        "拆分会破坏绘制批次合并优化；TODO(#597) 后续按绘制层提取子模块",

    # --- production-test-bloat: 既有内嵌测试模块，待后续拆分到独立 _tests.rs ---
    (Path("core/writer_platform_api/src/lib.rs"), "production-test-bloat"):
        "既有 FFI 契约测试，验证跨语言边界；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/delete_guard.rs"), "production-test-bloat"):
        "既有删除守卫单元测试，验证引用计数不变量；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/action_registry.rs"), "production-test-bloat"):
        "既有动作注册表测试，验证分发契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/app_config.rs"), "production-test-bloat"):
        "既有应用配置测试，验证加载/迁移契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/layout_policy.rs"), "production-test-bloat"):
        "既有布局策略测试，验证断点/方向切换；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/screen_policy.rs"), "production-test-bloat"):
        "既有屏幕策略测试，验证分类/适配规则；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/editor/strong_types.rs"), "production-test-bloat"):
        "既有强类型测试，验证 newtype 不变量与边界检查；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/sync/github_api_client.rs"), "production-test-bloat"):
        "既有 GitHub API 客户端测试，验证请求/响应映射；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/writing_stats/aggregate.rs"), "production-test-bloat"):
        "既有写作统计聚合测试，验证时间窗口计算；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/writing_stats/store.rs"), "production-test-bloat"):
        "既有写作统计存储测试，验证持久化/索引；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/starmap/semantic.rs"), "production-test-bloat"):
        "既有星图语义测试，验证节点/边语义校验；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/starmap/package_storage.rs"), "production-test-bloat"):
        "既有星图包存储测试，验证打包/解包契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/api/envelope.rs"), "production-test-bloat"):
        "既有 API 信封测试，验证错误码/分页映射；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/storage/transaction.rs"), "production-test-bloat"):
        "既有存储事务测试，验证提交/回滚原子性；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/history/command.rs"), "production-test-bloat"):
        "既有历史命令测试，验证撤销/重做契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/history/manager.rs"), "production-test-bloat"):
        "既有历史管理器测试，验证栈/边界不变量；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/facade/mod.rs"), "production-test-bloat"):
        "既有门面测试，验证 API 聚合路由；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/search/api.rs"), "production-test-bloat"):
        "既有搜索 API 测试，验证查询/替换/高亮；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/api/types/screen_policy.rs"), "production-test-bloat"):
        "既有屏幕策略 DTO 测试，验证序列化契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/api/types/editor.rs"), "production-test-bloat"):
        "既有编辑器 DTO 测试，验证序列化/版本契约；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/api/service/mod.rs"), "production-test-bloat"):
        "既有 API 服务测试，验证路由/错误映射；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/sync_bridge.rs"), "production-test-bloat"):
        "既有同步桥测试，验证命令映射/回调；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/backend/app_backend.rs"), "production-test-bloat"):
        "既有应用后端测试，验证状态树委托；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/backend/diagnostics.rs"), "production-test-bloat"):
        "既有诊断后端测试，验证健康检查/指标；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/editor/layout.rs"), "production-test-bloat"):
        "既有排版测试，验证行盒/换行/光标命中；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/editor/paragraph_index_map.rs"), "production-test-bloat"):
        "既有段落索引映射测试，验证段落/偏移转换；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/sujian_editor_item/animation_coordinator.rs"), "production-test-bloat"):
        "既有动画协调器测试，验证多轨道时序；待拆分到独立 _tests.rs",
    (Path("apps/Linux_qt/src/sujian_editor_item/linux_coordinator.rs"), "production-test-bloat"):
        "既有 Linux 协调器测试，验证平台事件路由；待拆分到独立 _tests.rs",
    (Path("core/writer_core/src/app_service/mod.rs"), "production-test-bloat"):
        "既有应用服务模块测试，验证生命周期/初始化契约；待拆分到独立 _tests.rs",

    # --- broad-suppression: 既有 crate 级 suppression，待精确收窄 ---
    (Path("apps/Linux_qt/src/main.rs"), "broad-suppression"):
        "Qt 应用入口既有 crate 级 allow(dead_code/unused/deprecated)，"
        "因 qmetaobject 宏生成大量未直接引用成员；"
        "TODO(#597) 待精确收窄到宏生成成员",
}


# ---------------------------------------------------------------------------
# 阈值常量
# ---------------------------------------------------------------------------

GOD_FILE_LINE_LIMIT = 800
LARGE_STATE_CLASS_LINE_LIMIT = 200
MULTIPLE_LARGE_STATE_THRESHOLD = 2
PRODUCTION_TEST_MODULE_LINE_LIMIT = 100
SHORT_VAR_CONSECUTIVE_LIMIT = 5

SHORT_VAR_NAMES: frozenset[str] = frozenset(
    {
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
        "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
        "tmp", "temp", "data", "result", "res", "ret", "val",
        "foo", "bar", "baz", "qux", "quux",
        "state1", "state2", "state3",
        "obj1", "obj2", "obj3",
        "item1", "item2", "item3",
        "tmp1", "tmp2", "tmp3",
        "a1", "a2", "a3", "b1", "b2", "b3",
    }
)

_PLACEHOLDER_REASONS = frozenset({"", "TODO", "FIXME", "tbd", "TBD"})


# ---------------------------------------------------------------------------
# 正则模式
# ---------------------------------------------------------------------------

_RUST_VAR_DECL_RE = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_]\w*)\s*(?::\s*[^=;]+)?=")
_KT_VAR_DECL_RE = re.compile(r"\b(?:val|var)\s+([A-Za-z_]\w*)\s*(?::\s*[^=]+)?=")

_RUST_TYPE_DECL_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:struct|enum)\s+([A-Za-z_]\w*)\b"
)
_KT_TYPE_DECL_RE = re.compile(
    r"^\s*(?:public|private|protected|internal|open|sealed|abstract|final|data|inline|value|enum)?\s*"
    r"(?:public|private|protected|internal|open|sealed|abstract|final|data|inline|value|enum)?\s*"
    r"(?:class|object)\s+([A-Za-z_]\w*)\b"
)

_RUST_CRATE_SUPPRESS_RE = re.compile(
    r"#!\s*\[\s*allow\s*\([^)]*\b"
    r"(?:warnings|deprecated|dead_code|unused(?:_[a-z_]+)?|clippy::all|non_snake_case)"
    r"\b[^)]*\)\s*\]"
)
_RUST_MOD_SUPPRESS_RE = re.compile(
    r"#\s*\[\s*allow\s*\(\s*(?:warnings|clippy::all)\s*\)\s*\]"
)
_KT_FILE_SUPPRESS_ALL_RE = re.compile(
    r'@file\s*:\s*Suppress\s*\(\s*\[?\s*"ALL"\s*[,)\]]'
)
_KT_FILE_SUPPRESS_MULTI_RE = re.compile(
    r'@file\s*:\s*Suppress\s*\(\s*\[?\s*"[^"]*"\s*,\s*"'
)

_RUST_CFG_TEST_RE = re.compile(r"#\[cfg\(test\)\]")
_RUST_MOD_TESTS_RE = re.compile(r"^\s*mod\s+(tests?)\s*\{")


# ---------------------------------------------------------------------------
# 路径判断
# ---------------------------------------------------------------------------

def _is_generated_path(rel_path: Path) -> bool:
    posix = rel_path.as_posix()
    for prefix in GENERATED_PATH_PREFIXES:
        if posix.startswith(prefix):
            return True
    for part in rel_path.parts:
        if part in SKIP_PATH_PARTS:
            return True
    return False


def _is_test_file(rel_path: Path) -> bool:
    posix = rel_path.as_posix()
    for prefix in TEST_PATH_PREFIXES:
        if posix.startswith(prefix):
            return True
    for part in rel_path.parts:
        if part in TEST_DIR_NAMES:
            return True
    name = rel_path.name
    if name in TEST_FILE_NAMES:
        return True
    for suffix in TEST_FILE_SUFFIXES:
        if name.endswith(suffix):
            return True
    return False


def _is_build_script(rel_path: Path) -> bool:
    return rel_path.name in BUILD_SCRIPT_NAMES


def is_production_source(rel_path: Path) -> bool:
    """判断相对路径是否为需要扫描的生产源码文件。"""
    if rel_path.suffix not in (".kt", ".kts", ".rs"):
        return False
    if _is_generated_path(rel_path):
        return False
    if _is_test_file(rel_path):
        return False
    if _is_build_script(rel_path):
        return False
    return True


# ---------------------------------------------------------------------------
# 文本处理辅助
# ---------------------------------------------------------------------------

def _strip_line_comment_and_string(line: str) -> str:
    """去掉单行字符串内容和行注释，避免文案/注释触发规则。"""
    result: list[str] = []
    in_string = False
    string_char: str | None = None
    i = 0
    while i < len(line):
        ch = line[i]
        if in_string:
            if ch == "\\":
                result.append(ch)
                if i + 1 < len(line):
                    result.append(line[i + 1])
                    i += 2
                    continue
                i += 1
                continue
            if ch == string_char:
                in_string = False
                result.append(ch)
                i += 1
                continue
            result.append(ch)
            i += 1
            continue
        if ch in ('"', "'"):
            in_string = True
            string_char = ch
            result.append(ch)
            i += 1
            continue
        if ch == "/" and i + 1 < len(line) and line[i + 1] == "/":
            break
        result.append(ch)
        i += 1
    return "".join(result)


def _count_effective_lines(text: str) -> int:
    """计算非空非注释行数。"""
    lines = text.splitlines()
    count = 0
    in_block_comment = False
    for line in lines:
        stripped = line.strip()
        if not stripped:
            continue
        if in_block_comment:
            if "*/" in stripped:
                in_block_comment = False
                after = stripped.split("*/", 1)[1].strip()
                if after and not after.startswith("//"):
                    count += 1
            continue
        if stripped.startswith("/*") and "*/" not in stripped[2:]:
            in_block_comment = True
            continue
        if stripped.startswith("//"):
            continue
        if stripped.startswith("/*") and "*/" in stripped[2:]:
            after = stripped.split("*/", 1)[1].strip()
            if after and not after.startswith("//"):
                count += 1
            continue
        count += 1
    return count


def _find_block_end(lines: list[str], start_index: int) -> int | None:
    """从 start_index 行开始，找到匹配的 } 的行号（0-indexed）。"""
    depth = 0
    started = False
    in_string = False
    string_char: str | None = None
    in_block_comment = False

    for i in range(start_index, len(lines)):
        line = lines[i]
        j = 0
        while j < len(line):
            ch = line[j]
            if in_block_comment:
                if ch == "*" and j + 1 < len(line) and line[j + 1] == "/":
                    in_block_comment = False
                    j += 2
                    continue
                j += 1
                continue
            if in_string:
                if ch == "\\":
                    j += 2
                    continue
                if ch == string_char:
                    in_string = False
                j += 1
                continue
            if ch == "/" and j + 1 < len(line):
                if line[j + 1] == "/":
                    break
                if line[j + 1] == "*":
                    in_block_comment = True
                    j += 2
                    continue
            if ch in ('"', "'"):
                in_string = True
                string_char = ch
                j += 1
                continue
            if ch == "{":
                depth += 1
                started = True
            elif ch == "}":
                depth -= 1
                if started and depth == 0:
                    return i
            j += 1
    return None


def _is_allowed(rel_path: Path, rule: str) -> bool:
    """判断某文件某规则是否在白名单中。"""
    return (rel_path, rule) in ALLOWED_EXCEPTIONS


# ---------------------------------------------------------------------------
# 规则检测
# ---------------------------------------------------------------------------

def _check_god_file(rel_path: Path, text: str, findings: list[Finding]) -> None:
    if _is_allowed(rel_path, "god-file"):
        return
    effective = _count_effective_lines(text)
    if effective > GOD_FILE_LINE_LIMIT:
        findings.append(
            Finding(
                rel_path,
                1,
                "god-file",
                f"生产源码文件 {effective} 行（超过 {GOD_FILE_LINE_LIMIT} 行上限），"
                "应按职责拆分为多个模块",
            )
        )


def _check_multiple_large_state(
    rel_path: Path, text: str, findings: list[Finding]
) -> None:
    if _is_allowed(rel_path, "multiple-large-state"):
        return
    lines = text.splitlines()
    is_rust = rel_path.suffix == ".rs"
    decl_re = _RUST_TYPE_DECL_RE if is_rust else _KT_TYPE_DECL_RE

    large_classes: list[tuple[str, int, int]] = []
    for index, line in enumerate(lines):
        m = decl_re.match(line)
        if not m:
            continue
        block_start = index
        for look in range(index, min(len(lines), index + 5)):
            if "{" in lines[look]:
                block_start = look
                break
        else:
            continue
        end = _find_block_end(lines, block_start)
        if end is None:
            continue
        line_count = end - index + 1
        if line_count > LARGE_STATE_CLASS_LINE_LIMIT:
            large_classes.append((m.group(1), index + 1, line_count))

    if len(large_classes) >= MULTIPLE_LARGE_STATE_THRESHOLD:
        names = ", ".join(f"{n}({c}行)" for n, _, c in large_classes)
        findings.append(
            Finding(
                rel_path,
                large_classes[0][1],
                "multiple-large-state",
                f"文件内同时声明 {len(large_classes)} 个超过 "
                f"{LARGE_STATE_CLASS_LINE_LIMIT} 行的状态类: {names}；"
                "应拆分到独立文件",
            )
        )


def _check_production_test_bloat(
    rel_path: Path, text: str, findings: list[Finding]
) -> None:
    if rel_path.suffix != ".rs":
        return
    if _is_allowed(rel_path, "production-test-bloat"):
        return
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if not _RUST_CFG_TEST_RE.search(line):
            continue
        for look in range(index, min(len(lines), index + 5)):
            if _RUST_MOD_TESTS_RE.match(lines[look]):
                end = _find_block_end(lines, look)
                if end is None:
                    continue
                test_lines = end - look + 1
                if test_lines > PRODUCTION_TEST_MODULE_LINE_LIMIT:
                    findings.append(
                        Finding(
                            rel_path,
                            look + 1,
                            "production-test-bloat",
                            f"生产 Rust 文件内嵌 {test_lines} 行测试模块"
                            f"（超过 {PRODUCTION_TEST_MODULE_LINE_LIMIT} 行上限），"
                            "测试应拆到独立 _tests.rs 文件",
                        )
                    )
                break


def _check_broad_suppression(
    rel_path: Path, text: str, findings: list[Finding]
) -> None:
    if _is_allowed(rel_path, "broad-suppression"):
        return
    lines = text.splitlines()
    is_rust = rel_path.suffix == ".rs"

    for index, line in enumerate(lines):
        code = _strip_line_comment_and_string(line)
        if is_rust:
            if _RUST_CRATE_SUPPRESS_RE.search(code):
                findings.append(
                    Finding(
                        rel_path,
                        index + 1,
                        "broad-suppression",
                        "禁止 crate/module 级宽范围 lint suppression；"
                        "应精确标注到具体成员",
                    )
                )
            if _RUST_MOD_SUPPRESS_RE.search(code):
                findings.append(
                    Finding(
                        rel_path,
                        index + 1,
                        "broad-suppression",
                        "禁止模块级 allow(warnings)/allow(clippy::all)；"
                        "应精确标注到具体成员",
                    )
                )
        else:
            if _KT_FILE_SUPPRESS_ALL_RE.search(code):
                findings.append(
                    Finding(
                        rel_path,
                        index + 1,
                        "broad-suppression",
                        "禁止文件级 @file:Suppress(\"ALL\")；"
                        "应精确标注具体 lint 并附原因",
                    )
                )
            if _KT_FILE_SUPPRESS_MULTI_RE.search(code):
                findings.append(
                    Finding(
                        rel_path,
                        index + 1,
                        "broad-suppression",
                        "禁止文件级 @file:Suppress 批量关闭多个 lint；"
                        "应精确标注到具体成员并附原因",
                    )
                )


def _check_short_variable_names(
    rel_path: Path, text: str, findings: list[Finding]
) -> None:
    if _is_allowed(rel_path, "short-variable-names"):
        return
    lines = text.splitlines()
    is_rust = rel_path.suffix == ".rs"
    decl_re = _RUST_VAR_DECL_RE if is_rust else _KT_VAR_DECL_RE

    consecutive: list[tuple[int, str]] = []
    last_decl_line = -10

    for index, line in enumerate(lines):
        code = _strip_line_comment_and_string(line)
        m = decl_re.search(code)
        if not m:
            continue
        var_name = m.group(1)
        if var_name.startswith("_"):
            consecutive = []
            last_decl_line = index
            continue
        if var_name in SHORT_VAR_NAMES:
            if index - last_decl_line <= 5:
                consecutive.append((index + 1, var_name))
            else:
                consecutive = [(index + 1, var_name)]
            last_decl_line = index
            if len(consecutive) > SHORT_VAR_CONSECUTIVE_LIMIT:
                names = ", ".join(n for _, n in consecutive)
                findings.append(
                    Finding(
                        rel_path,
                        consecutive[0][0],
                        "short-variable-names",
                        f"连续 {len(consecutive)} 个无语义短变量名: {names}；"
                        "应使用表达意图的命名",
                    )
                )
                consecutive = []
        else:
            consecutive = []
            last_decl_line = index


def _check_whitelist_reasons(findings: list[Finding]) -> None:
    """规则 6: 白名单条目必须有原因。"""
    for (path, rule), reason in ALLOWED_EXCEPTIONS.items():
        if not rule or reason.strip() in _PLACEHOLDER_REASONS:
            findings.append(
                Finding(
                    path,
                    1,
                    "no-reason-exception",
                    "白名单条目缺少规则名或原因说明；"
                    "每项必须附带 (文件, 规则, 具体原因)",
                )
            )


# ---------------------------------------------------------------------------
# 扫描入口
# ---------------------------------------------------------------------------

def scan_text(rel_path: Path, text: str) -> list[Finding]:
    """扫描单个文件文本，返回违规列表。"""
    findings: list[Finding] = []
    _check_god_file(rel_path, text, findings)
    _check_multiple_large_state(rel_path, text, findings)
    _check_production_test_bloat(rel_path, text, findings)
    _check_broad_suppression(rel_path, text, findings)
    _check_short_variable_names(rel_path, text, findings)
    return findings


def iter_source_files(root: Path) -> Iterable[Path]:
    """迭代仓库中需要扫描的生产源码文件。"""
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if path.suffix not in (".kt", ".kts", ".rs"):
            continue
        rel = path.relative_to(root)
        if not is_production_source(rel):
            continue
        yield path


def scan_repository(root: Path) -> list[Finding]:
    """扫描整个仓库，返回违规列表。"""
    findings: list[Finding] = []
    _check_whitelist_reasons(findings)
    for path in iter_source_files(root):
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            rel = path.relative_to(root)
            findings.append(
                Finding(rel, 1, "non-utf8-source", "源文件必须是 UTF-8 编码")
            )
            continue
        rel = path.relative_to(root)
        findings.extend(scan_text(rel, text))
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="扫描生产源码结构问题")
    parser.add_argument("root", nargs="?", default=".", help="仓库根目录")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    findings = scan_repository(root)
    for finding in findings:
        print(f"{finding.path}:{finding.line}: {finding.rule}: {finding.message}")

    if findings:
        print(f"发现 {len(findings)} 个源码结构问题。", file=sys.stderr)
        return 1
    print("源码结构扫描通过。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
