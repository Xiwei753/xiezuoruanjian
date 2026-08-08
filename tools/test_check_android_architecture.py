#!/usr/bin/env python3
"""Android 架构扫描器自测（#597 六）。

静态规则的测试只测试扫描器本身：通过临时夹具源码树验证检测逻辑确实能
抓到真实违规样例，不编译 Android App、不检查类名反射。

必须覆盖：
- 检测器识别 import 语句与全限定名引用；
- 检测器忽略注释（// 与 /* */）中的引用；
- 干净夹具不误报；
- 每条分层规则对真实违规样例都产生报告；
- 真实仓库全量扫描零违规（回归门禁）。
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import check_android_architecture as arch


def make_tree(root: Path, files: dict[str, str]) -> None:
    for rel, content in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


APP_PREFIX = "src/main/kotlin/com/xiwei/sujian"
DS_PREFIX = "src/main/kotlin/com/xiwei/sujian/core/designsystem"


class DetectorPrimitivesTest(unittest.TestCase):
    def test_flags_import_statement(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/ui/Violation.kt": (
                        "package com.xiwei.sujian.ui\n\n"
                        "import com.xiwei.sujian.data.SyncBridge\n"
                        "class V { fun use() {} }\n"
                    )
                },
            )
            findings = arch.scan_forbidden(root, "/ui/", ["com.xiwei.sujian.data.SyncBridge"])
            self.assertTrue(findings, "import 语句中的禁止引用必须被识别")

    def test_flags_fully_qualified_usage(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/ui/Violation.kt": (
                        "package com.xiwei.sujian.ui\n\n"
                        "class V { fun use(): com.xiwei.sujian.data.SyncBridge? = null }\n"
                    )
                },
            )
            findings = arch.scan_forbidden(root, "/ui/", ["com.xiwei.sujian.data.SyncBridge"])
            self.assertTrue(findings, "全限定名引用必须被识别")

    def test_ignores_line_comment(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/ui/Clean.kt": (
                        "package com.xiwei.sujian.ui\n\n"
                        "// import com.xiwei.sujian.data.SyncBridge -- 注释不应误报\n"
                        "class Clean\n"
                    )
                },
            )
            findings = arch.scan_forbidden(root, "/ui/", ["com.xiwei.sujian.data.SyncBridge"])
            self.assertEqual([], findings, "单行注释中的引用不应误报")

    def test_ignores_block_comment(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/ui/Clean.kt": (
                        "package com.xiwei.sujian.ui\n\n"
                        "/**\n"
                        " * 文档说明：本层不持有 WindowDisplayFrameClock（显示层职责）。\n"
                        " */\n"
                        "class Clean\n"
                    )
                },
            )
            findings = arch.scan_forbidden(root, "/ui/", ["WindowDisplayFrameClock"])
            self.assertEqual([], findings, "块注释中的引用不应误报")

    def test_clean_file_not_flagged(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/ui/Clean.kt": (
                        "package com.xiwei.sujian.ui\n\n"
                        "import androidx.compose.runtime.Composable\n"
                        "import com.xiwei.sujian.core.interop.settings.SettingsRepository\n"
                        "class CleanViewModel(val repo: SettingsRepository)\n"
                    )
                },
            )
            findings = arch.scan_forbidden(
                root, "/ui/", ["uniffi.writer_core", "com.sun.jna", "com.xiwei.sujian.data.SyncBridge"]
            )
            self.assertEqual([], findings, "干净文件不应误报")


class LayerRuleTests(unittest.TestCase):
    def run_rule(self, rule_id: str, files: dict[str, str]) -> list[arch.Finding]:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(root, files)
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
            )
            _, by_rule = arch.run_checks()
            return by_rule[rule_id]

    def test_ui_bridge_rule_flags_concrete_bridge(self):
        findings = self.run_rule(
            "ui-no-uniffi-jna-bridge",
            {
                f"{APP_PREFIX}/app/theme/StatsScreen.kt": (
                    "package com.xiwei.sujian.app.theme\n\n"
                    "import com.xiwei.sujian.core.interop.app.AppServiceBridge\n"
                    "class Bad { val b: AppServiceBridge? = null }\n"
                )
            },
        )
        self.assertTrue(findings, "UI 层直接引用 AppServiceBridge 必须被报告")

    def test_ui_bridge_rule_exempts_theme_repository(self):
        """ThemeRepository 承担 Repository 职责，豁免引用 Bridge/UniFFI（#602 Phase 7）。"""
        findings = self.run_rule(
            "ui-no-uniffi-jna-bridge",
            {
                f"{APP_PREFIX}/app/theme/ThemeRepository.kt": (
                    "package com.xiwei.sujian.app.theme\n\n"
                    "import com.xiwei.sujian.core.interop.app.AppServiceBridge\n"
                    "import uniffi.writer_core.WriterCoreFfi\n"
                    "class ThemeRepository(val bridge: AppServiceBridge)\n"
                )
            },
        )
        self.assertEqual([], findings, "ThemeRepository 引用 Bridge/UniFFI 应被豁免")

    def test_ui_bridge_rule_passes_repository_usage(self):
        findings = self.run_rule(
            "ui-no-uniffi-jna-bridge",
            {
                f"{APP_PREFIX}/app/theme/StatsScreen.kt": (
                    "package com.xiwei.sujian.app.theme\n\n"
                    "import com.xiwei.sujian.core.interop.settings.SettingsRepository\n"
                    "class Good(val repo: SettingsRepository)\n"
                )
            },
        )
        self.assertEqual([], findings, "UI 层引用 Repository 是允许的")

    def test_data_rule_flags_compose_dependency(self):
        findings = self.run_rule(
            "data-no-ui-framework",
            {
                f"{APP_PREFIX}/core/interop/common/BadBridge.kt": (
                    "package com.xiwei.sujian.core.interop.common\n\n"
                    "import androidx.compose.runtime.Composable\n"
                    "class Bad { @Composable fun render() {} }\n"
                )
            },
        )
        self.assertTrue(findings, "data 层依赖 androidx.compose 必须被报告")

    def test_input_rule_flags_repository_dependency(self):
        findings = self.run_rule(
            "input-layer-pure",
            {
                f"{APP_PREFIX}/feature/editor/input/BadInput.kt": (
                    "package com.xiwei.sujian.feature.editor.input\n\n"
                    "import com.xiwei.sujian.core.interop.project.WorkspaceRepository\n"
                    "class Bad(val repo: WorkspaceRepository)\n"
                )
            },
        )
        self.assertTrue(findings, "input 层依赖 data 层必须被报告")

    def test_input_rule_allows_dto_contract(self):
        findings = self.run_rule(
            "input-layer-pure",
            {
                f"{APP_PREFIX}/feature/editor/input/GoodInput.kt": (
                    "package com.xiwei.sujian.feature.editor.input\n\n"
                    "import uniffi.writer_core.EditorTransactionCauseDto\n"
                    "class Good(val cause: EditorTransactionCauseDto)\n"
                )
            },
        )
        self.assertEqual([], findings, "input 层引用 EditorTransactionCauseDto 契约类型是允许的")

    def test_motion_rule_flags_persistence_dependency(self):
        findings = self.run_rule(
            "visual-motion-pure",
            {
                f"{APP_PREFIX}/feature/editor/motion/BadMotion.kt": (
                    "package com.xiwei.sujian.feature.editor.motion\n\n"
                    "import com.xiwei.sujian.core.interop.settings.SettingsRepository\n"
                    "class Bad(val repo: SettingsRepository)\n"
                )
            },
        )
        self.assertTrue(findings, "motion 层写正文持久状态必须被报告")

    def test_session_rule_flags_mutable_state(self):
        findings = self.run_rule(
            "session-layer-no-platform-state",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "import androidx.compose.runtime.mutableStateOf\n"
                    "class EditorSessionCoordinator {\n"
                    "    val bad by mutableStateOf(0)\n"
                    "    val sessionStateFlow = 1\n"
                    "    val activeTargetId = 2\n"
                    "    val editingState = 3\n"
                    "    val windowBindingState = 4\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "session 层持有 Compose mutableStateOf 必须被报告")

    def test_session_rule_flags_derived_flow_revival(self):
        findings = self.run_rule(
            "session-layer-no-platform-state",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "class EditorSessionCoordinator {\n"
                    "    fun getActiveTargetIdFlow() {}\n"
                    "    val sessionStateFlow = 1\n"
                    "    val activeTargetId = 2\n"
                    "    val editingState = 3\n"
                    "    val windowBindingState = 4\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "派生 stateIn flow getter 复活必须被报告")

    def test_frame_clock_rule_flags_session_reference(self):
        findings = self.run_rule(
            "frame-clock-window-owned",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "class EditorSessionCoordinator {\n"
                    "    val clock: WindowDisplayFrameClock? = null\n"
                    "    val sessionStateFlow = 1\n"
                    "    val activeTargetId = 2\n"
                    "    val editingState = 3\n"
                    "    val windowBindingState = 4\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "session 层持有 FrameClock 必须被报告")

    def test_transform_purity_rule_flags_store_write_inside_transform(self):
        findings = self.run_rule(
            "update-session-state-transform-purity",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "fun bad() {\n"
                    "    updateSessionState { previous ->\n"
                    "        store.put(previous)\n"
                    "        previous\n"
                    "    }\n"
                    "    pendingRecord?.let { store.put(it) }\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "transform 体内调用 store.put 必须被报告")

    def test_transform_purity_rule_passes_pure_transform(self):
        findings = self.run_rule(
            "update-session-state-transform-purity",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "fun good() {\n"
                    "    var pendingRecord: Any? = null\n"
                    "    updateSessionState { previous -> previous.copy() }\n"
                    "    pendingRecord?.let { store.put(it) }\n"
                    "}\n"
                )
            },
        )
        self.assertEqual([], findings, "纯 transform + pendingRecord 模式不应误报")

    def test_designsystem_rule_flags_app_package_reference(self):
        findings = self.run_rule(
            "designsystem-independence",
            {
                f"{DS_PREFIX}/component/Bad.kt": (
                    "package com.xiwei.sujian.core.designsystem.component\n\n"
                    "import com.xiwei.sujian.app.SujianAppState\n"
                    "class Bad(val state: SujianAppState)\n"
                )
            },
        )
        self.assertTrue(findings, "designsystem 反向依赖 app 包必须被报告")

    def test_designsystem_rule_passes_clean_component(self):
        findings = self.run_rule(
            "designsystem-independence",
            {
                f"{DS_PREFIX}/component/Good.kt": (
                    "package com.xiwei.sujian.core.designsystem.component\n\n"
                    "import androidx.compose.material3.Text\n"
                    "class Good { fun render() = Text }\n"
                )
            },
        )
        self.assertEqual([], findings, "干净 designsystem 组件不应误报")

    def test_deleted_types_rule_flags_animation_settings_revival(self):
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/editor/coordinator/EditorAnimationSettings.kt": (
                    "package com.xiwei.sujian.feature.editor.coordinator\n\n"
                    "class EditorAnimationSettings\n"
                )
            },
        )
        self.assertTrue(findings, "EditorAnimationSettings 复活必须被报告")

    def test_source_contracts_rule_flags_missing_sync_commit(self):
        findings = self.run_rule(
            "source-contracts",
            {
                # 同步函数契约检查 SyncRepository（#602 Phase 7 从 SettingsRepository 拆分）
                f"{APP_PREFIX}/feature/sync/data/SyncRepository.kt": (
                    "package com.xiwei.sujian.feature.sync.data\n\nclass SyncRepository\n"
                ),
                f"{APP_PREFIX}/core/interop/sync/SyncProfileGate.kt": (
                    "package com.xiwei.sujian.core.interop.sync\n\nclass SyncProfileGate\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("commitSyncProfile", messages, "缺少 commitSyncProfile 必须被报告")


class RealRepoTest(unittest.TestCase):
    def test_real_repo_scan_passes(self):
        """真实仓库全量扫描必须零违规（架构门禁回归测试）。"""
        arch.configure(
            app_src=arch.DEFAULT_APP_SRC,
            designsystem_src=arch.DEFAULT_DS_SRC,
            designsystem_module=arch.DEFAULT_DS_MODULE,
        )
        all_findings, _ = arch.run_checks()
        self.assertEqual(
            [],
            all_findings,
            "真实仓库架构扫描必须通过:\n"
            + "\n".join(f"{f.path}:{f.line} {f.message}" for f in all_findings),
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
