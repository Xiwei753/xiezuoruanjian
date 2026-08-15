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
                platform_module=root,
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

    def test_visual_rule_allows_core_rebase_mapping_dto(self):
        # #606 评论5: visual 层直接消费 Core 的 RebaseSliceMappingDto（只读、不重算）。
        findings = self.run_rule(
            "visual-motion-pure",
            {
                f"{APP_PREFIX}/feature/editor/visual/GoodRebase.kt": (
                    "package com.xiwei.sujian.feature.editor.visual\n\n"
                    "import uniffi.writer_core.RebaseSliceMappingDto\n"
                    "fun consume(m: RebaseSliceMappingDto) = m\n"
                )
            },
        )
        self.assertEqual([], findings, "visual 层消费 Core rebase mapping DTO 必须被允许")

    def test_visual_rule_flags_other_uniffi_dto(self):
        # visual 层仍不得直接引用白名单外的 uniffi DTO（只允许消费已收口的契约类型）。
        findings = self.run_rule(
            "visual-motion-pure",
            {
                f"{APP_PREFIX}/feature/editor/visual/BadUniffi.kt": (
                    "package com.xiwei.sujian.feature.editor.visual\n\n"
                    "import uniffi.writer_core.EditorEditResultDto\n"
                    "fun consume(d: EditorEditResultDto) = d\n"
                )
            },
        )
        self.assertTrue(findings, "visual 层引用白名单外 uniffi DTO 必须被报告")

    def test_session_rule_flags_mutable_state(self):
        findings = self.run_rule(
            "session-layer-no-platform-state",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
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
                f"{APP_PREFIX}/feature/editor/session/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
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
                f"{APP_PREFIX}/feature/editor/session/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
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

    def test_session_mutation_gate_only_flags_updateSessionState_presence(self):
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun bad() {\n"
                    "    updateSessionState { previous -> previous }\n"
                    "    mutateSession { }\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "updateSessionState 出现必须被报告（已删除）")

    def test_session_mutation_gate_only_flags_direct_state_flow_write(self):
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun bad() {\n"
                    "    _sessionStateFlow.value = newState\n"
                    "    mutateSession { }\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "_sessionStateFlow.value 直接赋值必须被报告")

    def test_session_mutation_gate_only_flags_direct_epoch_write(self):
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun bad() {\n"
                    "    inputLeaseEpoch = 42\n"
                    "    mutateSession { }\n"
                    "}\n"
                )
            },
        )
        self.assertTrue(findings, "inputLeaseEpoch 直接赋值必须被报告")

    def test_session_mutation_gate_only_passes_clean_mutateSession(self):
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "class EditorSessionCoordinator {\n"
                    "    val _sessionStateFlow = MutableStateFlow(0)\n"
                    "    var inputLeaseEpoch = 0L\n"
                    "    fun mutateSession(block: () -> Unit) {\n"
                    "        _sessionStateFlow.value = 1\n"
                    "        inputLeaseEpoch = 1\n"
                    "    }\n"
                    "    fun readSession(block: () -> Unit) {}\n"
                    "}\n"
                ),
                f"{APP_PREFIX}/feature/editor/session/SessionMutationScope.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "class SessionMutationScope {\n"
                    "    var sessionState = 0\n"
                    "    var leaseEpoch = 0L\n"
                    "}\n"
                ),
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.completeWindowAttach(\n"
                    "    windowId: String,\n"
                    "    targetId: String,\n"
                    "    sessionId: ULong,\n"
                    "): Boolean = true\n"
                ),
            },
        )
        self.assertEqual([], findings, "mutateSession 内写 state/epoch 不应误报")

    def test_session_mutation_gate_only_flags_missing_read_session(self):
        """#624 评论17 问题1/6：readSession 必须存在。"""
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionCoordinator.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "class EditorSessionCoordinator {\n"
                    "    fun mutateSession(block: () -> Unit) {}\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("readSession", messages, "缺少 readSession 必须被报告")

    def test_session_mutation_gate_only_flags_direct_store_access_outside_gateway(self):
        """#624 评论17 问题1/6：生产 session 代码不得直接调 store.record/allRecords/isRegistered。"""
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionEditOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun bad(coordinator: EditorSessionCoordinator) {\n"
                    "    val rec = coordinator.store.record(\"t1\")\n"
                    "    val all = coordinator.store.allRecords()\n"
                    "    val reg = coordinator.store.isRegistered(\"t1\")\n"
                    "    mutateSession { }\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("store.record(", messages, "直接调 store.record 必须被报告")
        self.assertIn("store.allRecords(", messages, "直接调 store.allRecords 必须被报告")
        self.assertIn("store.isRegistered(", messages, "直接调 store.isRegistered 必须被报告")

    def test_session_mutation_gate_only_flags_scope_holding_coordinator(self):
        """#624 评论17 问题5/6：SessionMutationScope 不得持有 EditorSessionCoordinator。"""
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/SessionMutationScope.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "class SessionMutationScope(\n"
                    "    internal val coordinator: EditorSessionCoordinator,\n"
                    ") {\n"
                    "    var sessionState = 0\n"
                    "    var leaseEpoch = 0L\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("EditorSessionCoordinator", messages, "scope 持有 coordinator 必须被报告")

    def test_session_mutation_gate_only_flags_mark_saved_revival(self):
        """#624 评论17 问题5/6：markSaved(targetId,...) 不得在生产 session 代码重新出现。"""
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionExternalOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.markSaved(targetId: String, v: Int) {}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("markSaved", messages, "markSaved 复活必须被报告")

    def test_session_mutation_gate_only_flags_complete_window_attach_non_boolean(self):
        """#624 评论17 问题2/6：completeWindowAttach 必须返回 Boolean。"""
        findings = self.run_rule(
            "session-mutation-gate-only",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.completeWindowAttach(\n"
                    "    windowId: String,\n"
                    "    targetId: String,\n"
                    "    sessionId: ULong,\n"
                    "): Unit {}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("completeWindowAttach", messages, "completeWindowAttach 非 Boolean 必须被报告")

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
                f"{APP_PREFIX}/feature/editor/session/EditorAnimationSettings.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "class EditorAnimationSettings\n"
                )
            },
        )
        self.assertTrue(findings, "EditorAnimationSettings 复活必须被报告")

    def test_deleted_types_rule_flags_labs_experimental_types_revival(self):
        """#617 评论四：labs 旧实验设置实现复活必须被报告（第二套真相）。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/app/labs/ExperimentalSettingsRepository.kt": (
                    "package com.xiwei.sujian.app.labs\n\n"
                    "class ExperimentalSettingsRepository\n"
                )
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("ExperimentalSettingsRepository", messages, "labs 类型复活必须被报告")

    def test_deleted_types_rule_flags_local_settings_state_revival(self):
        """#617 评论六：整份 localSettingsState 可观察状态复活必须被报告（根热路径回归）。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/settings/data/SettingsRepository.kt": (
                    "package com.xiwei.sujian.feature.settings.data\n\n"
                    "private val _localSettingsState = MutableStateFlow(LocalSettings())\n"
                    "val localSettingsState: StateFlow<LocalSettings> = _localSettingsState.asStateFlow()\n"
                )
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("localSettingsState", messages, "localSettingsState 复活必须被报告")

    def test_deleted_types_rule_ignores_local_settings_state_in_comments(self):
        """#617 评论六：注释里提及 localSettingsState（如本文档注释）不得误报。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/settings/data/SettingsRepository.kt": (
                    "package com.xiwei.sujian.feature.settings.data\n\n"
                    "// localSettingsState 已删除 — 窗口层只认 immersiveFullscreenEnabled\n"
                    "class SettingsRepository\n"
                )
            },
        )
        self.assertEqual([], findings, "注释中的 localSettingsState 不得误报")

    def test_deleted_types_rule_flags_ui_model_interaction_state_revival(self):
        """#617 评论八：UI 模型复活 isExpanded/isSelected 交互状态字段必须被报告（第二份真相）。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/project/ui/ProjectWorkspaceUiState.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "data class VolumeUiModel(\n"
                    "    val id: String,\n"
                    "    val title: String,\n"
                    "    val chapters: List<ChapterUiModel>,\n"
                    "    val isExpanded: Boolean = false,\n"
                    ")\n\n"
                    "data class ChapterUiModel(\n"
                    "    val id: String,\n"
                    "    val isSelected: Boolean = false,\n"
                    ")\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("UI 模型交互状态字段必须保持删除", messages, "isExpanded/isSelected 复活必须被报告")

    def test_deleted_types_rule_flags_tree_reading_ui_model_interaction_state(self):
        """#617 评论八：渲染方直接读 volume.isExpanded 必须被报告（应只从 expandedVolumeIds 派生）。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/project/ui/VolumeChapterTree.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "fun render(volume: VolumeUiModel) {\n"
                    "    if (volume.isExpanded) {}\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("渲染方不得读 UI 模型交互状态字段", messages, "渲染方读 isExpanded 必须被报告")

    def test_deleted_types_rule_ignores_ui_model_interaction_state_in_comments(self):
        """#617 评论八：注释里提及 isExpanded（如本文档注释）不得误报。"""
        findings = self.run_rule(
            "deleted-types-stay-deleted",
            {
                f"{APP_PREFIX}/feature/project/ui/ProjectWorkspaceUiState.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "// isExpanded 已删除 — 展开只存在 expandedVolumeIds 一份真相\n"
                    "data class VolumeUiModel(val id: String, val title: String, val chapters: List<ChapterUiModel>)\n"
                ),
                f"{APP_PREFIX}/feature/project/ui/VolumeChapterTree.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "// 渲染从 expandedVolumeIds 派生，不再读 volume.isExpanded\n"
                    "fun render(volume: VolumeUiModel, expanded: Set<String>) {}\n"
                ),
            },
        )
        self.assertEqual([], findings, "注释中的 isExpanded 不得误报")

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

    def test_source_contracts_rule_flags_project_view_model_android_view_model_revival(self):
        """#617 评论一：ProjectViewModel 复活 AndroidViewModel/Application 依赖必须被报告。"""
        findings = self.run_rule(
            "source-contracts",
            {
                f"{APP_PREFIX}/feature/project/ui/ProjectViewModel.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "import android.app.Application\n"
                    "class ProjectViewModel(application: Application) : AndroidViewModel(application)\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("AndroidViewModel/Application", messages, "崩溃根因依赖复活必须被报告")

    def test_source_contracts_rule_flags_app_root_local_settings_collection(self):
        """#617 评论六：应用根收集/读取整份本地设置必须被报告（根热路径回归）。"""
        findings = self.run_rule(
            "source-contracts",
            {
                f"{APP_PREFIX}/app/SujianApp.kt": (
                    "package com.xiwei.sujian.app\n\n"
                    "val localSettings by deps.settingsRepository.localSettingsState.collectAsState()\n"
                    "val localSettings2 = deps.settingsRepository.getLocalSettings()\n"
                )
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("整份本地设置", messages, "应用根收集整份本地设置必须被报告")

    def test_source_contracts_rule_allows_immersive_boolean_at_app_root(self):
        """#617 评论六：根部只收集沉浸式全屏专用布尔，不得误报。"""
        findings = self.run_rule(
            "source-contracts",
            {
                f"{APP_PREFIX}/app/SujianApp.kt": (
                    "package com.xiwei.sujian.app\n\n"
                    "val immersiveFullscreenEnabled by\n"
                    "    deps.settingsRepository.immersiveFullscreenEnabled.collectAsState()\n"
                    "ImmersiveSystemBarsEffect(activity = activityRef, enabled = immersiveFullscreenEnabled)\n"
                )
            },
        )
        # rule_source_contracts 的 require() 还会报告夹具缺失的其它文件；
        # 本用例只关心 SujianApp.kt 不得被 forbid 误报。
        app_findings = [f for f in findings if f.path == "app/SujianApp.kt"]
        self.assertEqual([], app_findings, "根部收集沉浸式全屏布尔不得误报")

    def test_package_dir_inconsistent_must_fail(self):
        """package 声明与物理目录不一致必须被报告（#602 评论#7 项13）。"""
        # 直接调 rule_package_dir_consistent 并注入临时 source_roots，
        # 显式 source_roots 优先于模块级 PACKAGE_SOURCE_ROOTS（#602 评论#9 项9.3）。
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/feature/sample/Inconsistent.kt": (
                        "package com.xiwei.sujian.feature.other\n\n"
                        "class Inconsistent\n"
                    )
                },
            )
            findings = arch.rule_package_dir_consistent(
                source_roots=(root / "src/main/kotlin",)
            )
            self.assertTrue(findings, "package 声明与目录不一致必须被报告")

    def test_package_dir_consistent_must_pass(self):
        """package 声明与物理目录一致不应被报告（#602 评论#7 项13）。"""
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/feature/sample/Consistent.kt": (
                        "package com.xiwei.sujian.feature.sample\n\n"
                        "class Consistent\n"
                    )
                },
            )
            findings = arch.rule_package_dir_consistent(
                source_roots=(root / "src/main/kotlin",)
            )
            self.assertEqual([], findings, "package 声明与目录一致不应被报告")

    def test_package_dir_debug_source_set_inconsistent(self):
        """debug 源集 package 与目录不一致必须被报告（#602 评论#8 项8.2）。

        rule_package_dir_consistent 必须覆盖 debug 源集，不能只扫 src/main。
        """
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    "src/debug/kotlin/com/xiwei/sujian/ui/Foo.kt": (
                        "package com.xiwei.sujian.app.debug\n\n"
                        "class Foo\n"
                    )
                },
            )
            findings = arch.rule_package_dir_consistent(
                source_roots=(root / "src/debug/kotlin",)
            )
            self.assertTrue(
                findings,
                "debug 源集 package com.xiwei.sujian.app.debug 位于 ui/ 目录必须被报告",
            )

    def test_input_rule_flags_feature_data_repository(self):
        """input 层直接依赖 feature data Repository 必须被报告（#602 评论#8 项8.3）。"""
        findings = self.run_rule(
            "input-layer-pure",
            {
                f"{APP_PREFIX}/feature/editor/input/BadInput.kt": (
                    "package com.xiwei.sujian.feature.editor.input\n\n"
                    "import com.xiwei.sujian.feature.project.data.ProjectRepository\n"
                    "class Bad(val repo: ProjectRepository)\n"
                )
            },
        )
        self.assertTrue(findings, "input 层依赖 feature.project.data 必须被报告")

    def test_data_no_editor_display_flags_settings_data(self):
        """feature/settings/data 反向依赖 editor.ui 必须被报告（#602 评论#8 项8.3）。"""
        findings = self.run_rule(
            "data-no-editor-display",
            {
                f"{APP_PREFIX}/feature/settings/data/BadRepo.kt": (
                    "package com.xiwei.sujian.feature.settings.data\n\n"
                    "import com.xiwei.sujian.feature.editor.ui.Bar\n"
                    "class Bad(val bar: Bar)\n"
                )
            },
        )
        self.assertTrue(findings, "settings/data 依赖 editor.ui 必须被报告")

    def test_editor_presentation_pure_flags_compose_ui(self):
        """feature/editor/presentation 依赖 Compose UI 必须被报告（#624 评论17）。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/BadVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import androidx.compose.ui.Modifier\n"
                    "class BadVm(val m: Modifier)\n"
                )
            },
        )
        self.assertTrue(findings, "presentation 依赖 androidx.compose.ui 必须被报告")

    def test_editor_presentation_pure_flags_android_view(self):
        """feature/editor/presentation 依赖 android.view 必须被报告（#624 评论17）。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/BadVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import android.view.View\n"
                    "class BadVm(val v: View)\n"
                )
            },
        )
        self.assertTrue(findings, "presentation 依赖 android.view 必须被报告")

    def test_editor_presentation_pure_flags_editor_platform(self):
        """feature/editor/presentation 依赖 editor.platform 必须被报告（#624 评论17）。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/BadVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import com.xiwei.sujian.feature.editor.platform.SujianEditorView\n"
                    "class BadVm(val v: SujianEditorView)\n"
                )
            },
        )
        self.assertTrue(findings, "presentation 依赖 editor.platform 必须被报告")

    def test_editor_presentation_pure_flags_editor_layout(self):
        """feature/editor/presentation 依赖 editor.layout 必须被报告（#624 评论17）。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/BadVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import com.xiwei.sujian.feature.editor.layout.LayoutToken\n"
                    "class BadVm(val t: LayoutToken)\n"
                )
            },
        )
        self.assertTrue(findings, "presentation 依赖 editor.layout 必须被报告")

    def test_editor_presentation_pure_flags_editor_ui(self):
        """feature/editor/presentation 依赖 editor.ui 必须被报告（#624 评论17）。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/BadVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import com.xiwei.sujian.feature.editor.ui.WritingPane\n"
                    "class BadVm(val p: WritingPane)\n"
                )
            },
        )
        self.assertTrue(findings, "presentation 依赖 editor.ui 必须被报告")

    def test_editor_presentation_pure_passes_clean_viewmodel(self):
        """干净的 presentation ViewModel（只依赖 lifecycle/coroutines/session）不报违规。"""
        findings = self.run_rule(
            "editor-presentation-pure",
            {
                f"{APP_PREFIX}/feature/editor/presentation/CleanVm.kt": (
                    "package com.xiwei.sujian.feature.editor.presentation\n\n"
                    "import androidx.lifecycle.ViewModel\n"
                    "import androidx.lifecycle.viewModelScope\n"
                    "import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator\n"
                    "import kotlinx.coroutines.launch\n"
                    "class CleanVm(val c: EditorSessionCoordinator) : ViewModel()\n"
                )
            },
        )
        self.assertEqual([], findings, "干净 presentation 不应被报告")

    # ----------------------------------------------------------------------
    # #628：架构扫描规则跟着新目录改
    # ----------------------------------------------------------------------

    def test_presentation_contract_layer_bridge_only_in_contract_dir(self):
        """Bridge 在 app/presentation/contract/PresentationContractBridge.kt 不报违规，
        在其他 presentation 文件报违规（#628：Bridge 路径改为相对路径判断）。"""
        findings = self.run_rule(
            "presentation-contract-layer",
            {
                f"{APP_PREFIX}/app/presentation/contract/PresentationContractBridge.kt": (
                    "package com.xiwei.sujian.app.presentation.contract\n\n"
                    "import com.xiwei.sujian.core.interop.app.AppServiceBridge\n"
                    "class PresentationContractBridge(val b: AppServiceBridge)\n"
                ),
                f"{APP_PREFIX}/app/presentation/OtherPolicy.kt": (
                    "package com.xiwei.sujian.app.presentation\n\n"
                    "import com.xiwei.sujian.core.interop.app.AppServiceBridge\n"
                    "class OtherPolicy(val b: AppServiceBridge)\n"
                ),
            },
        )
        paths = {f.path for f in findings}
        self.assertIn(
            "app/presentation/OtherPolicy.kt",
            paths,
            "Bridge 在非 contract/PresentationContractBridge.kt 的 presentation 文件必须被报告",
        )
        self.assertNotIn(
            "app/presentation/contract/PresentationContractBridge.kt",
            paths,
            "Bridge 在 app/presentation/contract/PresentationContractBridge.kt 不应被报告",
        )

    def test_presentation_contract_layer_allows_new_dto_whitelist(self):
        """新白名单 DTO（WindowViewportDto/PrimaryNavigationPlacementDto/LayoutMetricsDto/ShellModeDto）
        在 presentation 层不报违规（#628）。"""
        findings = self.run_rule(
            "presentation-contract-layer",
            {
                f"{APP_PREFIX}/app/presentation/contract/CleanContract.kt": (
                    "package com.xiwei.sujian.app.presentation.contract\n\n"
                    "import uniffi.writer_core.WindowViewportDto\n"
                    "import uniffi.writer_core.PrimaryNavigationPlacementDto\n"
                    "import uniffi.writer_core.LayoutMetricsDto\n"
                    "import uniffi.writer_core.ShellModeDto\n"
                    "import uniffi.writer_core.LayoutContractDto\n"
                    "class CleanContract(\n"
                    "    val viewport: WindowViewportDto,\n"
                    "    val placement: PrimaryNavigationPlacementDto,\n"
                    "    val metrics: LayoutMetricsDto,\n"
                    "    val shell: ShellModeDto,\n"
                    "    val contract: LayoutContractDto,\n"
                    ")\n"
                ),
            },
        )
        self.assertEqual([], findings, "新白名单 DTO 不应被报告")

    def test_presentation_contract_layer_flags_old_dto(self):
        """旧 DTO（WindowCapabilitiesDto/PointerClassDto）已从白名单移除，
        presentation 层引用必须被报告（#628）。"""
        findings = self.run_rule(
            "presentation-contract-layer",
            {
                f"{APP_PREFIX}/app/presentation/contract/BadContract.kt": (
                    "package com.xiwei.sujian.app.presentation.contract\n\n"
                    "import uniffi.writer_core.WindowCapabilitiesDto\n"
                    "import uniffi.writer_core.PointerClassDto\n"
                    "class BadContract(val w: WindowCapabilitiesDto, val p: PointerClassDto)\n"
                ),
            },
        )
        self.assertTrue(findings, "旧 DTO WindowCapabilitiesDto/PointerClassDto 必须被报告")

    def test_presentation_layout_no_breakpoints_flags_window_width_size_class(self):
        """app/presentation/layout/ 引用 WindowWidthSizeClass 必须被报告（#628）。"""
        findings = self.run_rule(
            "presentation-layout-no-breakpoints",
            {
                f"{APP_PREFIX}/app/presentation/layout/BadLayout.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass\n"
                    "class BadLayout(val w: WindowWidthSizeClass)\n"
                ),
            },
        )
        self.assertTrue(findings, "presentation/layout/ 引用 WindowWidthSizeClass 必须被报告")

    def test_presentation_layout_no_breakpoints_flags_hardcoded_600(self):
        """app/presentation/layout/ 硬编码 >= 600 必须被报告（#628）。"""
        findings = self.run_rule(
            "presentation-layout-no-breakpoints",
            {
                f"{APP_PREFIX}/app/presentation/layout/BadBreakpoint.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "fun isExpanded(width: Int): Boolean = width >= 600\n"
                ),
            },
        )
        self.assertTrue(findings, "presentation/layout/ 硬编码 >= 600 必须被报告")

    def test_presentation_layout_no_breakpoints_flags_hardcoded_1200(self):
        """app/presentation/layout/ 硬编码 < 1200 必须被报告（#628）。"""
        findings = self.run_rule(
            "presentation-layout-no-breakpoints",
            {
                f"{APP_PREFIX}/app/presentation/layout/BadBreakpoint2.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "fun isCompact(width: Int): Boolean = width < 1200\n"
                ),
            },
        )
        self.assertTrue(findings, "presentation/layout/ 硬编码 < 1200 必须被报告")

    def test_presentation_layout_no_breakpoints_passes_clean_adapter(self):
        """干净的 AndroidLayoutAdapter（只消费 LayoutContractDto）不报违规（#628）。"""
        findings = self.run_rule(
            "presentation-layout-no-breakpoints",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import uniffi.writer_core.LayoutContractDto\n"
                    "class AndroidLayoutAdapter(val contract: LayoutContractDto) {\n"
                    "    fun paneCount(): Int = contract.paneCount\n"
                    "}\n"
                ),
            },
        )
        self.assertEqual([], findings, "干净 AndroidLayoutAdapter 不应被报告")

    def test_ui_no_direct_layout_decision_flags_navigation(self):
        """app/navigation/ 引用 WindowWidthSizeClass 必须被报告（#628）。"""
        findings = self.run_rule(
            "ui-no-direct-layout-decision",
            {
                f"{APP_PREFIX}/app/navigation/BadNav.kt": (
                    "package com.xiwei.sujian.app.navigation\n\n"
                    "import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass\n"
                    "class BadNav(val w: WindowWidthSizeClass)\n"
                ),
            },
        )
        self.assertTrue(findings, "app/navigation/ 引用 WindowWidthSizeClass 必须被报告")

    def test_ui_no_direct_layout_decision_flags_feature_ui(self):
        """feature/*/ui 引用 availablePaneCount 必须被报告（#628）。"""
        findings = self.run_rule(
            "ui-no-direct-layout-decision",
            {
                f"{APP_PREFIX}/feature/project/ui/BadUi.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "fun pane(availablePaneCount: Int): Int = availablePaneCount\n"
                ),
            },
        )
        self.assertTrue(findings, "feature/project/ui 引用 availablePaneCount 必须被报告")

    def test_ui_no_direct_layout_decision_passes_clean_navigation(self):
        """干净的 navigation（消费 LayoutContractDto.primaryNavigationPlacement）不报违规（#628）。"""
        findings = self.run_rule(
            "ui-no-direct-layout-decision",
            {
                f"{APP_PREFIX}/app/navigation/CleanNav.kt": (
                    "package com.xiwei.sujian.app.navigation\n\n"
                    "import uniffi.writer_core.LayoutContractDto\n"
                    "class CleanNav(val contract: LayoutContractDto) {\n"
                    "    fun isSideNav(): Boolean = contract.primaryNavigationPlacement.isSide\n"
                    "}\n"
                ),
            },
        )
        self.assertEqual([], findings, "干净 navigation 消费 LayoutContractDto 不应被报告")

    # ------------------------------------------------------------------
    # #628 验收点 7：架构守卫收口（结构尺寸 / 旧 pane API / 旧模式禁回归）
    # ------------------------------------------------------------------

    def test_presentation_layout_no_structural_dimensions_flags_180dp(self):
        """feature/*/ui 硬编码 180.dp 必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-structural-dimensions",
            {
                f"{APP_PREFIX}/feature/project/ui/BadCard.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "import androidx.compose.ui.unit.dp\n"
                    "val cardWidth = 180.dp\n"
                ),
            },
        )
        self.assertTrue(findings, "feature/*/ui 硬编码 180.dp 必须被报告")

    def test_presentation_layout_no_structural_dimensions_flags_240dp_and_56dp(self):
        """feature/*/ui 硬编码 240.dp / 56.dp 必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-structural-dimensions",
            {
                f"{APP_PREFIX}/feature/project/ui/BadPane.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "import androidx.compose.ui.unit.dp\n"
                    "val paneWidth = 240.dp\n"
                    "val railHeight = 56.dp\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("240.dp", messages, "240.dp 必须被报告")
        self.assertIn("56.dp", messages, "56.dp 必须被报告")

    def test_presentation_layout_no_structural_dimensions_passes_layout_metrics(self):
        """feature/*/ui 消费 Rust LayoutMetrics 不报违规（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-structural-dimensions",
            {
                f"{APP_PREFIX}/feature/project/ui/GoodCard.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "import uniffi.writer_core.LayoutMetricsDto\n"
                    "class GoodCard(val metrics: LayoutMetricsDto) {\n"
                    "    val cardWidth = metrics.projectCardWidthDp\n"
                    "}\n"
                ),
            },
        )
        self.assertEqual([], findings, "消费 LayoutMetrics 不应被报告")

    def test_presentation_layout_no_pane_scaffold_directive_flags_revival(self):
        """AndroidLayoutAdapter.kt 重新出现 PaneScaffoldDirective 死链必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-pane-scaffold-directive",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import androidx.compose.material3.adaptive.PaneScaffoldDirective\n"
                    "class BadAdapter(val directive: PaneScaffoldDirective)\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("PaneScaffoldDirective", messages, "PaneScaffoldDirective 复活必须被报告")

    def test_presentation_layout_no_pane_scaffold_directive_flags_calculate_and_max_partitions(self):
        """calculatePaneScaffoldDirective / maxHorizontalPartitionsFor 复活必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-pane-scaffold-directive",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "fun bad() {\n"
                    "    val d = calculatePaneScaffoldDirective()\n"
                    "    val n = maxHorizontalPartitionsFor(d)\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("calculatePaneScaffoldDirective", messages)
        self.assertIn("maxHorizontalPartitionsFor", messages)

    def test_presentation_layout_no_pane_scaffold_directive_ignores_comment(self):
        """注释中提及 PaneScaffoldDirective（说明已删除）不得误报（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-pane-scaffold-directive",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "/**\n"
                    " * #628：PaneScaffoldDirective / calculatePaneScaffoldDirective /\n"
                    " * maxHorizontalPartitionsFor 整条死链已删除。\n"
                    " */\n"
                    "class CleanAdapter\n"
                ),
            },
        )
        self.assertEqual([], findings, "注释中的 PaneScaffoldDirective 不得误报")

    def test_presentation_layout_no_local_configuration_screen_size_flags_width(self):
        """AndroidLayoutAdapter.kt 用 screenWidthDp 必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-local-configuration-screen-size",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import androidx.compose.ui.platform.LocalConfiguration\n"
                    "fun badWidth(): Int = LocalConfiguration.current.screenWidthDp\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("screenWidthDp", messages, "screenWidthDp 必须被报告")

    def test_presentation_layout_no_local_configuration_screen_size_flags_height(self):
        """AndroidLayoutAdapter.kt 用 screenHeightDp 必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-local-configuration-screen-size",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import android.content.res.Configuration\n"
                    "fun badHeight(c: Configuration): Int = c.screenHeightDp\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("screenHeightDp", messages, "screenHeightDp 必须被报告")

    def test_presentation_layout_no_local_configuration_screen_size_passes_local_window_info(self):
        """AndroidLayoutAdapter.kt 用 LocalWindowInfo.containerDpSize 不报违规（#628 验收点 7）。"""
        findings = self.run_rule(
            "presentation-layout-no-local-configuration-screen-size",
            {
                f"{APP_PREFIX}/app/presentation/layout/AndroidLayoutAdapter.kt": (
                    "package com.xiwei.sujian.app.presentation.layout\n\n"
                    "import androidx.compose.ui.platform.LocalWindowInfo\n"
                    "fun width(): Float = LocalWindowInfo.current.containerDpSize.width\n"
                ),
            },
        )
        self.assertEqual([], findings, "用 LocalWindowInfo.containerDpSize 不应被报告")

    def test_no_legacy_workspace_pane_mode_flags_workspace_pane_mode(self):
        """WorkspacePaneMode 复活必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "no-legacy-workspace-pane-mode",
            {
                f"{APP_PREFIX}/feature/project/ui/BadMode.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "enum class WorkspacePaneMode { SinglePane, ListDetail, ThreePane }\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("WorkspacePaneMode", messages, "WorkspacePaneMode 复活必须被报告")

    def test_no_legacy_workspace_pane_mode_flags_list_detail_and_three_pane_variants(self):
        """Kotlin 枚举变体 LIST_DETAIL / THREE_PANE 复活必须被报告（#628 验收点 7）。"""
        findings = self.run_rule(
            "no-legacy-workspace-pane-mode",
            {
                f"{APP_PREFIX}/feature/project/ui/BadEnum.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "enum class WorkspaceLayoutMode {\n"
                    "    SINGLE_PANE,\n"
                    "    LIST_DETAIL,\n"
                    "    THREE_PANE,\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("LIST_DETAIL", messages, "LIST_DETAIL 变体必须被报告")
        self.assertIn("THREE_PANE", messages, "THREE_PANE 变体必须被报告")

    def test_no_legacy_workspace_pane_mode_passes_workspace_layout_mode(self):
        """新工作区模式 WorkspaceLayoutMode{SinglePane,Workbench} 不报违规（#628 验收点 7）。"""
        findings = self.run_rule(
            "no-legacy-workspace-pane-mode",
            {
                f"{APP_PREFIX}/feature/project/ui/GoodMode.kt": (
                    "package com.xiwei.sujian.feature.project.ui\n\n"
                    "enum class WorkspaceLayoutMode { SinglePane, Workbench }\n"
                ),
            },
        )
        self.assertEqual([], findings, "WorkspaceLayoutMode 不应被报告")

    def test_no_legacy_workspace_pane_mode_passes_shell_mode_three_pane(self):
        """ShellMode::ThreePane 是壳层模式，允许保留，不报违规（#628 验收点 7）。"""
        findings = self.run_rule(
            "no-legacy-workspace-pane-mode",
            {
                f"{APP_PREFIX}/app/navigation/ShellMode.kt": (
                    "package com.xiwei.sujian.app.navigation\n\n"
                    "enum class ShellMode { SinglePane, TwoPane, ThreePane }\n"
                    "val shell = ShellMode.ThreePane\n"
                ),
            },
        )
        self.assertEqual([], findings, "ShellMode::ThreePane 壳层模式不应被报告")


class PackageSourceRootsFollowAppSrcTest(unittest.TestCase):
    """configure(--app-src) 后 PACKAGE_SOURCE_ROOTS 必须跟随（#602 评论#9 项9.3）。"""

    def tearDown(self) -> None:
        arch.configure(
            app_src=arch.DEFAULT_APP_SRC,
            designsystem_src=arch.DEFAULT_DS_SRC,
            designsystem_module=arch.DEFAULT_DS_MODULE,
            platform_module=arch.DEFAULT_PLATFORM_MODULE,
        )

    def test_configure_updates_package_source_roots_inconsistent(self):
        """configure 后 rule_package_dir_consistent 不传 source_roots 时扫描新根。"""
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/feature/sample/Misplaced.kt": (
                        "package com.xiwei.sujian.feature.other\n\n"
                        "class Misplaced\n"
                    )
                },
            )
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
                platform_module=root,
            )
            findings = arch.rule_package_dir_consistent()
            self.assertTrue(
                findings,
                "configure 后 PACKAGE_SOURCE_ROOTS 必须跟随 app_src，"
                "不一致的 package 声明必须被报告",
            )

    def test_configure_updates_package_source_roots_consistent(self):
        """configure 后一致目录不报错。"""
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/feature/sample/Aligned.kt": (
                        "package com.xiwei.sujian.feature.sample\n\n"
                        "class Aligned\n"
                    )
                },
            )
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
                platform_module=root,
            )
            findings = arch.rule_package_dir_consistent()
            self.assertEqual(
                [], findings,
                "configure 后一致的 package 声明不应被报告",
            )

    def test_platform_module_package_dir_inconsistent(self):
        """:core:platform 文件 package 与目录不一致必须被报告（#602 评论#10 项1）。"""
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            platform_module = root / "core/platform"
            make_tree(
                platform_module,
                {
                    "src/main/kotlin/com/xiwei/sujian/core/platform/window/Bad.kt": (
                        "package com.xiwei.sujian.core.platform.api\n\n"
                        "class Bad\n"
                    )
                },
            )
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
                platform_module=platform_module,
            )
            findings = arch.rule_package_dir_consistent()
            self.assertTrue(
                findings,
                ":core:platform package 与目录不一致必须被报告",
            )

    def test_three_modules_consistent_no_findings(self):
        """三个 Gradle 模块 package 都与目录一致时不报错（#602 评论#10 项1）。"""
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            platform_module = root / "core/platform"
            make_tree(
                root,
                {
                    f"{APP_PREFIX}/feature/sample/Aligned.kt": (
                        "package com.xiwei.sujian.feature.sample\n\n"
                        "class Aligned\n"
                    ),
                },
            )
            make_tree(
                platform_module,
                {
                    "src/main/kotlin/com/xiwei/sujian/core/platform/api/Good.kt": (
                        "package com.xiwei.sujian.core.platform.api\n\n"
                        "class Good\n"
                    )
                },
            )
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
                platform_module=platform_module,
            )
            findings = arch.rule_package_dir_consistent()
            self.assertEqual(
                [], findings,
                "三模块 package 都与目录一致不应被报告",
            )


class SessionCloseBeforeClaimTest(unittest.TestCase):
    """#624 评论5294575627 要求5：close-before-claim 守卫测试。"""

    def run_rule(self, rule_id: str, files: dict[str, str]) -> list[arch.Finding]:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            make_tree(root, files)
            arch.configure(
                app_src=root / APP_PREFIX,
                designsystem_src=root / DS_PREFIX,
                designsystem_module=root,
                platform_module=root,
            )
            _, by_rule = arch.run_checks()
            return by_rule[rule_id]

    def test_flags_close_target_close_before_claim(self):
        """closeTarget 中 closeSession 出现在 mutateSession 之前必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.closeTarget(\n"
                    "    targetId: String,\n"
                    "    reason: SessionCloseReason,\n"
                    ") {\n"
                    "    val sid = readSession { record(targetId)?.sessionId }\n"
                    "    closeSession(sid)\n"
                    "    mutateSession { removeRecord(targetId) }\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("close-before-claim", messages, "closeTarget close-before-claim 违规必须被报告")

    def test_flags_detach_window_binding_close_before_claim(self):
        """detachWindowBinding 中 closeSession 出现在 mutateSession 之前必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.detachWindowBinding(\n"
                    "    windowId: String,\n"
                    "    targetId: String,\n"
                    ") {\n"
                    "    val sid = readSession { record(targetId)?.sessionId }\n"
                    "    closeSession(sid)\n"
                    "    mutateSession { removeRecord(targetId) }\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("close-before-claim", messages, "detachWindowBinding close-before-claim 违规必须被报告")

    def test_flags_release_host_records_to_close_old_pattern(self):
        """releaseHost 使用 recordsToClose 旧模式必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.releaseHost() {\n"
                    "    val recordsToClose = readSession { allRecords() }\n"
                    "    recordsToClose.forEach { closeSession(it.sessionId) }\n"
                    "    mutateSession { clearRecords() }\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("recordsToClose", messages, "releaseHost recordsToClose 旧模式必须被报告")

    def test_flags_reset_persistent_session_zero_session_id_old_path(self):
        """resetPersistentSession 出现 updateRecord(targetId) { it.copy(sessionId = 0UL) } 必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.resetPersistentSession(\n"
                    "    targetId: String,\n"
                    "    text: String,\n"
                    "    cursorUtf8: Int,\n"
                    "): ExternalResetResult {\n"
                    "    if (!validateSession(sessionId)) {\n"
                    "        mutateSession { updateRecord(targetId) { it.copy(sessionId = 0UL) } }\n"
                    "        closeSession(sessionId)\n"
                    "    }\n"
                    "    return ExternalResetResult.Failed\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("0UL", messages, "resetPersistentSession 0UL 旧路径必须被报告")

    def test_flags_commit_prepared_binding_state_non_boolean(self):
        """commitPreparedBindingState 非 Boolean 返回必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.commitPreparedBindingState(\n"
                    "    targetId: String,\n"
                    "    sessionId: ULong,\n"
                    "    precondition: SessionBindPrecondition,\n"
                    "): Unit {}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("commitPreparedBindingState", messages, "commitPreparedBindingState 非 Boolean 必须被报告")

    def test_flags_commit_active_session_close_before_claim(self):
        """commitActiveSession 中 closeSession 出现在 mutateSession 之前必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {\n"
                    "    val sid = readSession { record(sessionState.activeTargetId ?: return@readSession 0UL)?.sessionId ?: 0UL }\n"
                    "    closeSession(sid)\n"
                    "    mutateSession { sessionState = EditorSessionState() }\n"
                    "    return true\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("close-before-claim", messages, "commitActiveSession close-before-claim 违规必须被报告")

    def test_flags_cancel_active_session_close_before_claim(self):
        """cancelActiveSession 中 closeSession 出现在 mutateSession 之前必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.cancelActiveSession(): Boolean {\n"
                    "    val sid = readSession { record(sessionState.activeTargetId ?: return@readSession 0UL)?.sessionId ?: 0UL }\n"
                    "    closeSession(sid)\n"
                    "    mutateSession { sessionState = EditorSessionState() }\n"
                    "    return true\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("close-before-claim", messages, "cancelActiveSession close-before-claim 违规必须被报告")

    def test_flags_deleted_old_bind_entries(self):
        """prepareActiveSessionIfCurrent/restampAttachingToWindow/clearWindowAttach 重新出现必须被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.prepareActiveSessionIfCurrent(targetId: String): SessionBindInfo? {\n"
                    "    return null\n"
                    "}\n"
                    "fun EditorSessionCoordinator.restampAttachingToWindow(windowId: String, targetId: String) {\n"
                    "}\n"
                    "fun EditorSessionCoordinator.clearWindowAttach(targetId: String) {\n"
                    "}\n"
                ),
            },
        )
        messages = " | ".join(f.message for f in findings)
        self.assertIn("prepareActiveSessionIfCurrent", messages)
        self.assertIn("restampAttachingToWindow", messages)
        self.assertIn("clearWindowAttach", messages)

    def test_passes_clean_claim_then_close(self):
        """干净实现（mutateSession 认领 → 锁外 closeSession(claim)）不应被报告。"""
        findings = self.run_rule(
            "session-close-before-claim",
            {
                f"{APP_PREFIX}/feature/editor/session/EditorSessionLifecycleOps.kt": (
                    "package com.xiwei.sujian.feature.editor.session\n\n"
                    "fun EditorSessionCoordinator.closeTarget(\n"
                    "    targetId: String,\n"
                    "    reason: SessionCloseReason,\n"
                    ") {\n"
                    "    val claim = mutateSession {\n"
                    "        val sid = record(targetId)?.sessionId ?: return@mutateSession null\n"
                    "        removeRecord(targetId)\n"
                    "        SessionCloseClaim(targetId, sid)\n"
                    "    }\n"
                    "    if (claim != null) closeSession(claim.sessionId)\n"
                    "}\n\n"
                    "fun EditorSessionCoordinator.detachWindowBinding(\n"
                    "    windowId: String,\n"
                    "    targetId: String,\n"
                    ") {\n"
                    "    val claim = mutateSession {\n"
                    "        val sid = record(targetId)?.sessionId ?: return@mutateSession null\n"
                    "        removeRecord(targetId)\n"
                    "        SessionCloseClaim(targetId, sid)\n"
                    "    }\n"
                    "    if (claim != null) closeSession(claim.sessionId)\n"
                    "}\n\n"
                    "fun EditorSessionCoordinator.releaseHost() {\n"
                    "    val ids = mutateSession {\n"
                    "        val l = allRecords().map { it.sessionId }\n"
                    "        clearRecords()\n"
                    "        l\n"
                    "    }\n"
                    "    ids.forEach { closeSession(it) }\n"
                    "}\n\n"
                    "fun EditorSessionCoordinator.resetPersistentSession(\n"
                    "    targetId: String,\n"
                    "    text: String,\n"
                    "    cursorUtf8: Int,\n"
                    "): ExternalResetResult {\n"
                    "    val candidate = createSession(targetId, text, cursorUtf8, true)\n"
                    "    return commitResetSnapshot(targetId, candidate, precondition, oldSessionIdToClose = null)\n"
                    "}\n\n"
                    "fun EditorSessionCoordinator.commitPreparedBindingState(\n"
                    "    targetId: String,\n"
                    "    sessionId: ULong,\n"
                    "    precondition: SessionBindPrecondition,\n"
                    "): Boolean = mutateSession { true }\n\n"
                    "fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {\n"
                    "    val sid = mutateSession {\n"
                    "        val targetId = sessionState.activeTargetId ?: return@mutateSession 0UL\n"
                    "        val rec = record(targetId) ?: return@mutateSession 0UL\n"
                    "        removeRecord(targetId)\n"
                    "        rec.sessionId\n"
                    "    }\n"
                    "    if (sid != 0UL) closeSession(sid)\n"
                    "    return true\n"
                    "}\n\n"
                    "fun EditorSessionCoordinator.cancelActiveSession(): Boolean {\n"
                    "    val sid = mutateSession {\n"
                    "        val targetId = sessionState.activeTargetId ?: return@mutateSession 0UL\n"
                    "        val rec = record(targetId) ?: return@mutateSession 0UL\n"
                    "        removeRecord(targetId)\n"
                    "        rec.sessionId\n"
                    "    }\n"
                    "    if (sid != 0UL) closeSession(sid)\n"
                    "    return true\n"
                    "}\n"
                ),
            },
        )
        self.assertEqual([], findings, "干净 claim-then-close 实现不应被报告")


class RealRepoTest(unittest.TestCase):
    def test_real_repo_scan_passes(self):
        """真实仓库全量扫描必须零违规（架构门禁回归测试）。"""
        arch.configure(
            app_src=arch.DEFAULT_APP_SRC,
            designsystem_src=arch.DEFAULT_DS_SRC,
            designsystem_module=arch.DEFAULT_DS_MODULE,
            platform_module=arch.DEFAULT_PLATFORM_MODULE,
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
