package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets

// ! # 设置页分节只读状态（#618 六）
//
// 每个分类只读取自己真正消费的字段投影。SettingsViewModel 保留 _uiState 作为唯一
// 可写业务状态，这些 section state 全部由 _uiState 派生（map -> distinctUntilChanged
// -> stateIn），只负责读，不构成第二份设置真相。
//
// 目的：设置根节点与所有已组合分类不再订阅整份 SettingsUiState；切换实验室开关时
// 只有 laboratoryState 发新值，外观/编辑器/同步等分类不跟着重组。

/** 外观分类真正读取的字段。
 *
 * Issue #723 评论 5748592923：字号/行距移回写作区设置，外观分类不再持有
 * fontSize/lineSpacing。外观页只保留主题模式与颜色来源。 */
data class AppearanceSectionState(
    val appearanceMode: String,
    val colorSource: String,
    val dynamicColorEnabled: Boolean,
)

/** 编辑器分类真正读取的字段（含字号、行距、协同动画开关 —
 * #618 六 复审：改主题颜色时不再让编辑器分类订阅整份外观状态）。
 *
 * Issue #723 评论 5748592923：EditorSectionState 增加 lineSpacing 与
 * coordinatedAnimationEnabled，字号/行距入口回到写作区设置。 */
data class EditorSectionState(
    val fontSize: Float,
    val lineSpacing: Float,
    val autoIndentEnabled: Boolean,
    val autoIndentWidth: Float,
    val typingAnimationEnabled: Boolean,
    val typingAnimationDurationMs: Int,
    val smoothCursorEnabled: Boolean,
    val smoothCursorDurationMs: Int,
    val coordinatedAnimationEnabled: Boolean,
)

/** 保存分类真正读取的字段。 */
data class SaveSectionState(
    val autoSaveEnabled: Boolean,
    val autoSaveDelayMs: Long,
)

/** AI 分类真正读取的字段。 */
data class AiSectionState(
    val available: Boolean,
    val enabled: Boolean,
)

/** 诊断分类真正读取的字段。 */
data class DiagnosticsSectionState(
    val enabled: Boolean,
    val verbose: Boolean,
)

/** 实验室分类真正读取的字段。 */
data class LaboratorySectionState(
    val immersiveFullscreen: Boolean,
)

/** 关于分类真正读取的字段。 */
data class AboutSectionState(
    val dataRootPath: String,
    val versionInfo: String,
)

/**
 * 同步分类真正读取的字段 — 全量同步只有一份配置/凭据/命令状态
 * （#630 评论 #1+#2：合并旧 projectSync/appSync 双份）。
 */
data class SyncSectionState(
    val syncConfig: SyncConfig,
    val syncSecrets: SyncSecrets,
    val syncCapability: SyncCapabilityData,
    val syncProfileLoadState: SyncProfileLoadState,
    val dryRunState: SyncCommandState,
    val testConnectionState: SyncCommandState,
    val performSyncState: SyncCommandState,
    val syncResult: StructuredSyncResult?,
    val secureStorageWarning: String?,
)
