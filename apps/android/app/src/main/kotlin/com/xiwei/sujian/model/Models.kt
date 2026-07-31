package com.xiwei.sujian.model

//! # 核心数据模型（Android UI 层 - Model 层）
//!
//! 定义所有 Kotlin 侧的数据类，与 Rust Core 的数据结构一一对应。
//!
//! ## 架构定位
//!
//! 这些模型是 Rust Core UniFFI DTO 的 Kotlin 映射，**不是业务实体**。
//! 业务实体的定义和操作都在 Rust Core 中。
//! 所有数据通过 UniFFI typed bridge 传输，不经过 Gson JSON 反序列化。
//!
//! ## 设计原则
//!
//! - 字段名使用 Kotlin camelCase 命名，与 Core serde rename_all = "camelCase" 契约一致
//! - 这些类只做数据承载，不包含业务逻辑
//! - 修改 Rust Core 数据结构时，必须同步更新这里的模型

import com.google.gson.JsonElement

data class WorkspaceManifest(
    val version: Int
)

data class DeviceInfo(
    val deviceId: String = "",
    val deviceClass: String = "",
    val platform: String = ""
)

data class LocalSettings(
    val themeMode: String? = "system",
    val appearanceMode: String = "system",
    val colorSource: String = "built_in",
    val dynamicColorEnabled: Boolean = false,
    val selectedBuiltinThemeId: String = "",
    val selectedPaletteId: String = "",
    val locale: String? = null,
    val editorFontSize: Float = 16f,
    val editorLineSpacingMultiplier: Float = 1.5f,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L,
    val autoIndentEnabled: Boolean = true,
    val autoIndentWidth: Float = 2.0f,
    val windowWidth: Double = 800.0,
    val windowHeight: Double = 600.0,
    val editorTypingAnimationEnabled: Boolean = true,
    val editorSmoothCursorEnabled: Boolean = true,
    val editorTypingAnimationDurationMs: Int = 100,
    val editorSmoothCursorDurationMs: Int = 80,
    val aiEnabled: Boolean = false,
    val statsDeviceId: String? = null,
    val desktopSidebarWidth: Double = 240.0,
    val desktopEditorWidth: Double = 0.0,
    val editorCoordinatedTextCursorAnimationEnabled: Boolean = false,
    val diagnosticsEnabled: Boolean = true,
    val diagnosticsVerbose: Boolean = true,
    val useSelfRenderEditorOnAndroid: Boolean = true,
    val experimentalFullscreenMode: Boolean = false
)

data class SyncableSettings(
    val fontSize: Double = 0.0,
    val themeMode: String = "",
    @Deprecated("Use themePaletteJson instead") val monetColor: String = "",
    val themePaletteJson: String = ""
)

data class Project(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String
)

data class ProjectStats(
    val totalWordCount: Int,
    val volumeCount: Int,
    val chapterCount: Int
)

data class Volume(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
    val order: Int = 0
)

data class ChapterMeta(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
    val order: Int = 0,
    val wordCount: Int,
    val hash: String,
    val note: String? = null
)

data class ChapterOpenResult(
    val meta: ChapterMeta,
    val content: String
)

typealias ChapterContent = ChapterOpenResult

data class ChapterSaveReceipt(
    val chapterRelativePath: String,
    val contentLen: Long,
    val contentHash: String,
    val metaHash: String,
    val updatedAt: String,
    val wordCount: Int
)

data class RecentEdit(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val timestamp: String
)

// Sync Models

enum class SyncTransport {
    HttpsToken,
    SshKey
}

enum class BackendType {
    Git,
    GithubApi
}

data class SyncConfig(
    val enabled: Boolean? = false,
    val backendType: BackendType? = BackendType.GithubApi,
    val remoteUrl: String? = "",
    val transport: SyncTransport? = SyncTransport.HttpsToken,
    val branch: String? = "main",
    val autoSync: Boolean? = false,
    val syncIntervalSeconds: Int? = 300,
    val username: String? = "",
    val hasNetworkPermission: Boolean? = null,
    val hasNetworkStatePermission: Boolean? = null
) {
    fun normalize(): SyncConfig {
        return copy(
            enabled = enabled ?: false,
            backendType = backendType ?: BackendType.GithubApi,
            remoteUrl = remoteUrl ?: "",
            transport = transport ?: SyncTransport.HttpsToken,
            branch = if (branch.isNullOrEmpty()) "main" else branch,
            autoSync = autoSync ?: false,
            syncIntervalSeconds = if (syncIntervalSeconds == null || syncIntervalSeconds <= 0) 300 else syncIntervalSeconds,
            username = username ?: "",
            hasNetworkPermission = hasNetworkPermission ?: true,
            hasNetworkStatePermission = hasNetworkStatePermission ?: true
        )
    }
}

data class SyncSecrets(
    val token: String? = null,
    val sshPrivateKey: String? = null
)

data class Tombstone(
    val originalPath: String,
    val trashPath: String,
    val deletedAt: Long,
    val purgeAfter: Long,
    val deletedBy: String,
    val originalHash: String,
    val kind: String
)

data class SyncState(
    val status: SyncStatus = SyncStatus.Idle,
    val remoteUrl: String? = null,
    val backendType: String? = null,
    val transport: String? = null,
    val lastSyncedCommit: String? = null,
    val lastSyncTime: Long? = null,
    val lastError: String? = null,
    val knownFiles: Map<String, String>? = emptyMap(),
    val conflicts: List<SyncConflict>? = emptyList(),
    val tombstones: List<Tombstone>? = emptyList(),
    val deletedFiles: Set<String>? = emptySet()
)

data class SyncCapabilityData(
    val canRun: Boolean = false,
    val blockReasonCode: String? = null,
    val blockMessageKey: String? = null,
    val messageArgs: Map<String, String> = emptyMap()
)

enum class SyncStatus {
    Idle,
    Syncing,
    Success,
    ConfiguredNotTested,
    Conflict,
    PartialConflict,
    RecoverableError,
    FatalError,
    DirtyRepoBlocked,
    BranchMissingRecovered,
    Error,
    NoChanges,
    LatestWinsApplied
}

enum class FirstSyncMode {
    NotAttempted,
    CloneIntoEmptyWorkspace,
    InitExistingWorkspace,
    AlreadyGitRepo,
    BlockedNonEmptyRemote,
    UnrelatedHistories,
    None
}

data class SyncConflictSummary(
    val status: String,
    val localDirty: Boolean,
    val remoteChanged: Boolean,
    val conflictedFiles: List<String> = emptyList(),
    val blockedReason: String,
    val safeNextSteps: List<String> = emptyList()
)

data class SettingConflictDetail(
    val key: String,
    val localValue: String,
    val remoteValue: String
)

data class SyncConflict(
    val localPath: String,
    val remotePath: String,
    val localHash: String,
    val remoteHash: String,
    val baseHash: String,
    val createdAt: Long,
    val description: String
)

data class SyncDiagnosticsResult(
    val success: Boolean,
    val backendType: String,
    val hasNetworkPermission: Boolean,
    val hasNetworkStatePermission: Boolean,
    val networkState: String,
    val networkOk: Boolean,
    val authOk: Boolean,
    val repoOk: Boolean,
    val branchOk: Boolean,
    val networkStatus: String,
    val authStatus: String,
    val repoStatus: String,
    val branchStatus: String,
    val remoteUrlSanitized: String,
    val transport: String,
    val errorCategory: String,
    val rawError: String?
)

data class SyncResult(
    val status: SyncStatus,
    val uploadedFiles: List<String> = emptyList(),
    val downloadedFiles: List<String> = emptyList(),
    val localDeletes: List<String> = emptyList(),
    val remoteDeletes: List<String> = emptyList(),
    val overwrittenFiles: List<String> = emptyList(),
    val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
    val conflictSummary: SyncConflictSummary? = null,
    val settingsConflicts: List<SettingConflictDetail>? = null,
    val commitHash: String? = null,
    val error: String? = null,
    val errorCategory: String? = null,
    val firstSyncMode: FirstSyncMode = FirstSyncMode.None
)

data class SyncPlan(
    val filesToUpload: List<String> = emptyList(),
    val filesToDownload: List<String> = emptyList(),
    val filesToDeleteLocal: List<String> = emptyList(),
    val filesToDeleteRemote: List<String> = emptyList(),
    val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<String> = emptyList()
)

data class ActionDescriptor(
    val id: String,
    val title: String,
    val description: String,
    val category: String,
    val kind: String,
    val riskLevel: String,
    val confirmRequired: Boolean,
    val undoable: Boolean,
    val platforms: List<String>,
    val inputSchema: JsonElement?,
    val uiSchema: JsonElement?
)

data class ActionResult(
    val success: Boolean,
    val message: String?,
    val data: JsonElement?,
    val proposedUi: JsonElement?,
    val requiresConfirmation: Boolean?
)

data class WritingStatsSummary(
    val range: WritingStatsRange? = null,
    val totalWordCount: Long = 0,
    val totalTimeSeconds: Long = 0,
    val activeDays: Int = 0,
    val totalHumanTypedChars: Long? = null,
    val totalActiveSeconds: Long? = null,
    val totalSessions: Int? = null,
    val daysCount: Int? = null
)

typealias WritingWritingStatsSummary = WritingStatsSummary

data class WritingStatsRange(
    val startDate: String? = null,
    val endDate: String? = null
)

data class ProjectWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val projects: List<ProjectWritingStatsItem>? = emptyList()
)

data class ProjectWritingStatsItem(
    val projectId: String? = null,
    val projectTitle: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null
)

data class ChapterWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val chapters: List<ChapterWritingStatsItem>? = emptyList()
)

data class ChapterWritingStatsItem(
    val chapterId: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null
)

data class DeviceWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val devices: List<DeviceWritingStatsItem>? = emptyList()
)

data class DeviceWritingStatsItem(
    val deviceId: String? = null,
    val platform: String? = null,
    val deviceClass: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null,
    val sessionsCount: Int? = null
)

data class WritingSpeedCurve(
    val range: WritingStatsRange? = null,
    val bucketMinutes: Int = 0,
    val buckets: List<WritingSpeedBucket>? = emptyList()
)

data class WritingSpeedBucket(
    val startMs: Long = 0,
    val endMs: Long = 0,
    val charsTyped: Long = 0,
    val charsPerMinute: Double = 0.0
)

data class ProjectStatsSummary(
    val projectId: String,
    val wordCount: Long
)

// ── Layout Policy Models ──

enum class FoldState {
    None, Flat, HalfOpened
}

enum class FoldOrientation {
    Horizontal, Vertical
}

enum class FoldOcclusion {
    None, Full
}

data class FoldFeatureInfo(
    val state: FoldState = FoldState.None,
    val orientation: FoldOrientation = FoldOrientation.Vertical,
    val isSeparating: Boolean = false,
    val occlusion: FoldOcclusion = FoldOcclusion.None,
    val boundsLeftVp: Float = 0f,
    val boundsTopVp: Float = 0f,
    val boundsRightVp: Float = 0f,
    val boundsBottomVp: Float = 0f
)

enum class Orientation {
    Unknown, Portrait, Landscape
}

enum class PointerKind {
    Unknown, Touch, Stylus, Mouse, Trackpad
}

enum class WidthClass {
    Compact, Medium, Expanded, Large, ExtraLarge
}

enum class HeightClass {
    Compact, Medium, Expanded
}

enum class ShellMode {
    SinglePane, SupportingPane, TwoPane, ThreePane
}

enum class EditorMode {
    FullWidth, CenteredPaper
}

enum class NavigationMode {
    Stack, ListDetail
}

enum class NavigationPresentation {
    BottomBar, NavigationRail, PermanentDrawer
}

enum class WorkspacePaneMode {
    SinglePane, ListDetail, ThreePane
}

data class VisiblePaneRoles(
    val showProjectList: Boolean = true,
    val showChapterTree: Boolean = true,
    val showEditor: Boolean = true,
    val showSupporting: Boolean = false
)

data class PaneWidthConstraint(
    val minDp: Float = 0f,
    val preferredDp: Float = 0f,
    val maxDp: Float = 0f
)

enum class AvoidRegionKind {
    WindowInset, VerticalHinge, HorizontalHinge
}

data class AvoidRegion(
    val leftDp: Float = 0f,
    val topDp: Float = 0f,
    val rightDp: Float = 0f,
    val bottomDp: Float = 0f,
    val kind: AvoidRegionKind = AvoidRegionKind.WindowInset
)

data class WindowMetrics(
    val widthDp: Float,
    val heightDp: Float,
    val safeTopDp: Float = 0f,
    val safeBottomDp: Float = 0f,
    val keyboardVisible: Boolean = false,
    val foldFeature: FoldFeatureInfo = FoldFeatureInfo(),
    val orientation: Orientation = Orientation.Portrait,
    val pointer: PointerKind = PointerKind.Touch
)

data class LayoutPlan(
    val widthClass: WidthClass,
    val heightClass: HeightClass,
    val shellMode: ShellMode,
    val editorMode: EditorMode,
    val navigationMode: NavigationMode,
    val navigationPresentation: NavigationPresentation,
    val workspacePaneMode: WorkspacePaneMode,
    val visiblePaneRoles: VisiblePaneRoles,
    val contentMaxWidthDp: Float,
    val pagePaddingDp: Float,
    val gridColumns: Int,
    val showBottomBar: Boolean,
    val listPaneWidth: PaneWidthConstraint,
    val editorContentMaxWidthDp: Float,
    val primaryPaneMinDp: Float,
    val primaryPanePreferredDp: Float,
    val primaryPaneMaxDp: Float,
    val supportingPaneMode: WorkspacePaneMode? = null,
    val avoidRegions: List<AvoidRegion> = emptyList()
)

// ── Screen Policy 类型（Core screen_policy） ──

enum class ScreenRole {
    Home, ProjectList, ProjectWorkspace, Writing, StarMap, Stats, Settings, Sync
}

enum class ActionRole {
    Back, Save, CreateProject, CreateVolume, CreateChapter, Delete, Rename, Settings, Sync, Search, Sort
}

enum class ActionPlacement {
    TopLeading, TopTrailing, Floating, BottomBar, ContextMenu, SidePanel, Navigation, ListHeader, ItemTrailing, EmptyState
}

enum class PaneRole {
    PrimaryList, Detail, Editor, Inspector, Drawer, Supporting
}

data class ActionSlot(
    val actionId: String,
    val role: ActionRole,
    val placement: ActionPlacement,
    val visibleIn: List<ShellMode>,
    val requiresConfirmation: Boolean
)

data class ScreenPolicy(
    val screenRole: ScreenRole,
    val actionSlots: List<ActionSlot>
)
