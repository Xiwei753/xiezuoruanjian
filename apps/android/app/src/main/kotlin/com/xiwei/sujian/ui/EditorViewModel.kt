package com.xiwei.sujian.ui

//! # 编辑器 ViewModel（Android UI 层 - ViewModel）
//!
//! 管理编辑器的 UI 状态、自动保存、设置同步、写作统计。
//!
//! ## 架构定位
//!
//! ```text
//! EditorActivity → EditorViewModel → WorkspaceRepository → WritingBridge/WorkspaceBridge → Rust Core
//! ```
//!
//! ## 职责边界
//!
//! - **做**：UI 状态管理、自动保存调度、设置加载/应用、写作统计上报、
//!   章节打开事务（latest-wins + 提交前 session 预准备）
//! - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 SujianEditorView 负责）
//! - **不直接调用 legacy JNI adapter**：只通过 Repository 和领域 Bridge 间接调用
//!
//! ## 关键流程
//!
//! 1. **章节加载**：`requestOpenChapter()` → 保存旧章节 → 加载新章节 →
//!   预准备 Rust session → 一次性提交（数据 + 会话 + 导航）
//! 2. **自动保存**：`onContentChanged()` → `scheduleAutoSave()` → `performSave()`
//! 3. **设置同步**：`onSettingsChanged()` → `reloadSettings()` → 更新 `EditorSettingsState`
//! 4. **写作统计**：`onContentChanged()` → `reportWritingEvent()` → `WorkspaceRepository.processWritingEvent()`
//!
//! ## 线程模型
//!
//! - UI 操作在 `Dispatchers.Main`
//! - 文件 I/O 在 `Dispatchers.IO`
//! - 保存互斥锁 `saveMutex` 防止并发保存冲突
//!
//! ## #595 一：章节打开事务
//!
//! `requestOpenChapter` 是数据、Rust session、窗口和 Navigator 的同一次提交：
//! 串行门 → requestId 校验 → 冻结旧章节输入 → 保存/flush A → 加载 B →
//! 为 B 预准备 Rust session（提交前取得有效 snapshot/bind plan）→
//! 再次确认 requestId 仍为最新 → 一次性提交 active=B/EditorUiState/EditorSessionState →
//! 调用方提交业务选择并导航。过期请求在每个可见提交边界回滚临时状态并返回
//! [ChapterSwitchResult.Stale]；保存/加载/session 预准备失败时 A 的状态、
//! 输入回调和 Navigator 全部保持不变。
//!
//! 输入窗口防护：提交成功返回后 `inputFrozen` 保持 true，直到新章节的
//! WritingPane 真实附着编辑器后调用 [confirmEditorAttached] 才解除 —
//! 旧章节 A 的 View 在"提交 → 导航"窗口期内无法把输入写入已切到 B 的 ViewModel。
//!
//! ## #595 二：文档版本
//!
//! 正文更新事实携带 [com.xiwei.sujian.editor.v2.coordinator.DocumentVersion]
//! （Repository contentHash + 同步 manifest 锚点），不再使用进程内 contentVersion
//! 计数器；新旧判断由会话层 reducer 按版本锚点 + localDirty 完成。

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceDocumentGate
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin
import com.xiwei.sujian.editor.v2.coordinator.DocumentVersion
import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.TargetDocumentFact
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.runtime.SujianAppDependencies
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

enum class SaveStatus {
    Idle,
    Unsaved,
    Saving,
    Saved,
    SaveFailed
}

data class EditorSession(
    val sessionId: String,
    val projectId: String,
    val volumeId: String,
    val chapterId: String
)

/**
 * #595 一：章节切换事务结果 — 导航目标与 ViewModel 当前章节只能从同一个
 * committed 状态派生，保存/加载失败必须回滚，不能只把 loading 改回 false。
 *
 * - [Success]：旧章节保存成功、新章节内容已加载提交且 Rust session 已预准备
 *   （currentSession/content/loading 一致）。
 * - [SaveFailed]：旧章节保存失败，currentSession 仍指向旧章节，调用方必须回滚导航。
 * - [LoadFailed]：新章节加载或 session 预准备失败，currentSession 已回退到旧章节，
 *   调用方必须回滚导航。
 */
sealed interface ChapterSwitchResult {
    data class Success(val session: EditorSession, val content: String) : ChapterSwitchResult
    data class SaveFailed(val current: EditorSession) : ChapterSwitchResult
    data class LoadFailed(val requested: ChapterKey) : ChapterSwitchResult
    /**
     * #595 一：请求已过期 — 更新的请求已排队或正在执行。调用方不得导航、
     * 不得回滚、不得修改任何状态；实际切换由最新请求完成。
     */
    data object Stale : ChapterSwitchResult
}

/** 章节三元组 — 切换失败时携带请求目标，供回滚/重试识别。 */
data class ChapterKey(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)

sealed class SaveCommand {
    data class Save(val content: String, val session: EditorSession) : SaveCommand()
    data class Clear(val session: EditorSession) : SaveCommand()
    data class Flush(val reply: kotlinx.coroutines.CompletableDeferred<Boolean>) : SaveCommand()
}

data class EditorSettingsState(
    val fontSize: Float = 16f,
    val lineSpacingMultiplier: Float = 1.5f,
    val autoIndentEnabled: Boolean = true,
    val autoIndentWidth: Float = 2.0f,
    val typingAnimationEnabled: Boolean = true,
    val typingAnimationDurationMs: Long = 100L,
    val smoothCursorEnabled: Boolean = true,
    val smoothCursorDurationMs: Long = 80L,
    val coordinatedTextCursorAnimationEnabled: Boolean = true,
    val reduceMotion: Boolean = false,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L
)

data class EditorUiState(
    val loading: Boolean = false,
    val content: String = "",
    /** #595 一/二：当前已加载章节正文的真实文件 hash（ChapterMeta.hash）—
     *  外部替换协议据此判断 Repository 版本新旧，不再由 UI 猜测。 */
    val chapterHash: String = "",
    val chapterTitle: String = "",
    val chapterNote: String? = null,
    val saveStatus: SaveStatus = SaveStatus.Idle,
    val wordCount: Int = 0,
    val sessionAdded: Int = 0,
    val speed: Int = 0,
    val errorMessage: String? = null,
    val editorEnabled: Boolean = true,
    val settings: EditorSettingsState = EditorSettingsState()
)

sealed class EditorEvent {
    data class ToastMessage(val message: String) : EditorEvent()
    data class ShowSaveFailedDialog(val message: String) : EditorEvent()
}

class EditorViewModel(
    application: Application
) : AndroidViewModel(application) {

    // #595 一：依赖注入 — 必须由 SujianApp 进程级容器提供同一组 Repository。
    // 删除 fallback WorkspaceRepository(getApplication())/SettingsRepository(getApplication())
    // （旧实现会为首次打开章节创建独立 Repository，绕过进程级依赖容器）。
    private var _workspaceRepository: WorkspaceRepository? = null
    private var _settingsRepository: SettingsRepository? = null
    private var _syncStatusRepository: com.xiwei.sujian.data.SyncStatusRepository? = null
    private var _sessionCoordinator: EditorSessionCoordinator? = null

    private val workspaceRepository: WorkspaceRepository
        get() = _workspaceRepository
            ?: error("EditorViewModel 未注入 WorkspaceRepository — 必须通过 Factory(SujianAppDependencies) 创建")
    private val settingsRepository: SettingsRepository
        get() = _settingsRepository
            ?: error("EditorViewModel 未注入 SettingsRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    // #595 五：同步观察使用 viewModelScope（随 ViewModel 自动取消），
    // 不再创建独立未管理 CoroutineScope。
    private var syncObserverJob: kotlinx.coroutines.Job? = null

    /**
     * #595 一/二/五：初始化 — 幂等；同步观察与正文 flush 回调只注册一次。
     *
     * [sessionCoordinator] 用于章节打开事务的 session 预准备（提交前取得
     * 有效 snapshot/bind plan，导航后编辑器立即可用）。
     */
    fun initialize(
        workspaceRepo: WorkspaceRepository,
        settingsRepo: SettingsRepository,
        syncStatusRepo: com.xiwei.sujian.data.SyncStatusRepository? = null,
        sessionCoordinator: EditorSessionCoordinator? = null,
    ) {
        if (_workspaceRepository == null) _workspaceRepository = workspaceRepo
        if (_settingsRepository == null) _settingsRepository = settingsRepo
        if (syncStatusRepo != null && _syncStatusRepository !== syncStatusRepo) {
            _syncStatusRepository = syncStatusRepo
            restartSyncObserver()
        }
        if (sessionCoordinator != null && _sessionCoordinator !== sessionCoordinator) {
            _sessionCoordinator = sessionCoordinator
        }
        // #595 三：同步前统一 flush 活动正文（WorkspaceDocumentGate）。
        // 同一进程同一时刻只有一个活动正文 VM，直接注册。
        WorkspaceDocumentGate.registerFlusher { requestSave().await() }
    }

    private fun restartSyncObserver() {
        syncObserverJob?.cancel()
        syncObserverJob = viewModelScope.launch(Dispatchers.IO) {
            val repo = _syncStatusRepository ?: return@launch
            var lastSynced = false
            repo.state.collect { state ->
                val isSynced = state == com.xiwei.sujian.model.SyncIndicatorState.Synced
                if (isSynced && !lastSynced) {
                    checkSyncMergedChapter()
                }
                lastSynced = isSynced
            }
        }
    }

    /**
     * #595 二/三：同步完成后检查当前章节磁盘内容是否变更 —
     * 变更时发布版本化文档事实（[TargetDocumentFact]），WritingPane 据此
     * 执行一次 Core reset 以反映同步合并后的新正文。
     */
    private suspend fun checkSyncMergedChapter() {
        val session = currentSession ?: return
        if (inputFrozen || _uiState.value.loading) return
        try {
            val (content, meta) = withContext(Dispatchers.IO) {
                workspaceRepository.getChapterContentWithMeta(
                    session.projectId, session.volumeId, session.chapterId
                )
            }
            val currentHash = _uiState.value.chapterHash
            if (meta.hash.isNotEmpty() &&
                meta.hash != currentHash &&
                content != _uiState.value.content &&
                syncMergeEmitDedup.shouldEmit(meta.hash)
            ) {
                val syncState = try { settingsRepository.loadSyncState() } catch (_: Exception) { null }
                val baseVersion = _sessionCoordinator?.sessionState?.committedVersion ?: DocumentVersion()
                emitDocumentFact(
                    TargetDocumentFact(
                        targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                        text = content,
                        sourceVersion = DocumentVersion(
                            contentHash = meta.hash,
                            syncManifestRevision = syncState?.lastSyncTime,
                        ),
                        baseVersion = baseVersion,
                        origin = DocumentFactOrigin.SYNC_MERGED,
                    )
                )
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Exception) {
            // 同步合并检查失败不阻塞用户操作
        }
    }

    private val _uiState = MutableStateFlow(EditorUiState())
    val uiState: StateFlow<EditorUiState> = _uiState.asStateFlow()

    private val _events = Channel<EditorEvent>(Channel.BUFFERED)
    val events = _events.receiveAsFlow()

    // #595 二：Repository 真实来源的正文文档事实流 — 按 target 分区的最新事实
    // 总线（带 replay 语义：新 collector 立即拿到该 target 的当前文档事实）。
    private val documentUpdateBus = TargetDocumentUpdateBus()

    private fun emitDocumentFact(fact: TargetDocumentFact) {
        documentUpdateBus.emit(fact)
    }

    /** 指定 target 的正文文档事实流 — 只收该 target 的事实，新 collector 先收到当前事实。 */
    fun documentUpdates(targetId: String): kotlinx.coroutines.flow.Flow<TargetDocumentFact> =
        documentUpdateBus.updates(targetId)

    private var currentSession: EditorSession? = null

    private var initialWordCount = 0
    private var sessionStartTime = System.currentTimeMillis()

    private val saveMutex = Mutex()
    private var pendingSaveContent: String? = null
    private var autoSaveJob: kotlinx.coroutines.Job? = null
    private var saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
    private var saveActorJob: kotlinx.coroutines.Job? = null
    private var contentExplicitlyCleared = false

    private val statsDeviceId: String by lazy {
        val prefs = application.getSharedPreferences("writer_stats", android.content.Context.MODE_PRIVATE)
        var id = prefs.getString("device_id", null)
        if (id == null) {
            id = "android-${java.util.UUID.randomUUID()}"
            prefs.edit().putString("device_id", id).apply()
        }
        id
    }

    private var statsSessionId: String = java.util.UUID.randomUUID().toString()
    private var statsLastEventMs: Long = 0
    private var previousText: String = ""
    private var isLoadingChapter = false

    /**
     * #595 一：输入冻结 — 章节切换事务期间与提交后（导航落地前）为 true；
     * 新章节 WritingPane 真实附着编辑器后经 [confirmEditorAttached] 解除。
     * 防止旧章节 A 的 View 在"提交 → 导航"窗口内把输入写入已切到 B 的 ViewModel。
     */
    @Volatile
    private var inputFrozen = false

    // #595 二：同步合并发射去重 — 每个章节只发射一次同一 fileHash 的 SyncMerged。
    // 章节提交时 reset（见 switchChapterLocked/initChapter）。
    private val syncMergeEmitDedup = SyncMergeEmitDedup()

    // #595 一：章节切换串行门 — 同一时间只允许一个切换事务执行；
    // 请求序号在每个可见提交边界校验，过期请求回滚临时状态并返回 Stale。
    private val chapterSwitchGate = ChapterSwitchGate()

    /**
     * #595 一：章节切换统一事务入口（手机竖屏、大屏工作台共用）。
     *
     * 固定顺序：串行门 → requestId 校验 → 冻结旧章节输入 → 保存旧章节 →
     * 加载新章节 → 预准备 Rust session → 再次校验 requestId → 一次性提交
     * active/正文/hash/标题/session → 返回 Success。调用方只在 Success 之后
     * 提交业务选择和 Navigator；失败时旧 EditorUiState 完整恢复、Navigator
     * 完全不变化。
     *
     * - 过期请求（事务执行期间有更新请求排队）在每个可见提交边界回滚临时
     *   状态并返回 [ChapterSwitchResult.Stale] — 不保存可见状态、不导航；
     * - [kotlinx.coroutines.CancellationException] 重新抛出并恢复旧状态，
     *   不得当作普通加载失败处理；
     * - Success 返回后 [inputFrozen] 保持 true，直到新 pane 附着编辑器。
     */
    suspend fun requestOpenChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String): ChapterSwitchResult {
        return when (val gate = chapterSwitchGate.runLatest { isLatest ->
            switchChapterLocked(isLatest, projectId, volumeId, chapterId, chapterTitle)
        }) {
            is ChapterSwitchGate.Result.Completed -> gate.value
            ChapterSwitchGate.Result.Stale -> ChapterSwitchResult.Stale
        }
    }

    /**
     * 兼容入口 — 由 WritingPane 在直接进入正文（深链/恢复）时调用；
     * 与 [requestOpenChapter] 共用同一串行锁和 requestId 语义。
     */
    suspend fun switchChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String): ChapterSwitchResult =
        requestOpenChapter(projectId, volumeId, chapterId, chapterTitle)

    /**
     * #595 一：判断给定章节是否为 ViewModel 当前已提交章节。
     * 防止切换事务提交后（业务选择/导航尚未落地的一帧内）旧 WritingPane 用
     * 新章节正文对旧 target 执行 beginEdit/外部替换。
     */
    fun isCurrentChapter(projectId: String, volumeId: String, chapterId: String): Boolean {
        val s = currentSession ?: return false
        return s.projectId == projectId && s.volumeId == volumeId && s.chapterId == chapterId
    }

    /**
     * #595 一：新章节 pane 真实附着编辑器后解除输入冻结。
     * 只允许当前已提交章节的 target 解除；过期 pane 调用是 no-op。
     */
    fun confirmEditorAttached(targetId: String) {
        val s = currentSession ?: return
        if (targetId == chapterTargetId(s.projectId, s.volumeId, s.chapterId)) {
            inputFrozen = false
        }
    }

    private suspend fun switchChapterLocked(
        isLatest: () -> Boolean,
        projectId: String,
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
    ): ChapterSwitchResult {
        val oldSession = currentSession
        // #595 一：事务回滚需要完整的旧 EditorUiState — 保存/加载失败时整体恢复。
        val oldUiState = _uiState.value
        val oldContentExplicitlyCleared = contentExplicitlyCleared

        if (oldSession != null && oldSession.projectId == projectId && oldSession.volumeId == volumeId && oldSession.chapterId == chapterId) {
            return ChapterSwitchResult.Success(oldSession, _uiState.value.content)
        }

        inputFrozen = true
        var preparedTargetId: String? = null
        try {
            // #595 一/转场：切换章节时同步置 loading — 在旧章节保存完成前就隐藏编辑器。
            _uiState.value = _uiState.value.copy(loading = true)

            if (oldSession != null) {
                autoSaveJob?.cancel()
                saveActorJob?.cancel()
                saveCommandChannel.close()

                val content = _uiState.value.content
                val saveOk = if (content.trim().isNotEmpty()) {
                    saveMutex.withLock {
                        try {
                            when (val result = workspaceRepository.saveChapterContent(oldSession.projectId, oldSession.volumeId, oldSession.chapterId, content)) {
                                is com.xiwei.sujian.data.BridgeResult.Success -> true
                                is com.xiwei.sujian.data.BridgeResult.Error -> {
                                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                                    emitErrorEvent(getApplication<Application>().getString(R.string.error_save_failed, result.message))
                                    false
                                }
                                com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                                    emitErrorEvent(getApplication<Application>().getString(R.string.error_save_native_not_loaded))
                                    false
                                }
                            }
                        } catch (e: kotlinx.coroutines.CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                            emitErrorEvent(getApplication<Application>().getString(R.string.error_save_exception, e.message ?: ""))
                            false
                        }
                    }
                } else if (contentExplicitlyCleared) {
                    saveMutex.withLock {
                        try {
                            when (val result = workspaceRepository.clearChapterContent(oldSession.projectId, oldSession.volumeId, oldSession.chapterId)) {
                                is com.xiwei.sujian.data.BridgeResult.Success -> true
                                is com.xiwei.sujian.data.BridgeResult.Error -> {
                                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                                    emitErrorEvent(getApplication<Application>().getString(R.string.error_save_failed, result.message))
                                    false
                                }
                                com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                                    false
                                }
                            }
                        } catch (e: kotlinx.coroutines.CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                            false
                        }
                    }
                } else {
                    true
                }

                if (!saveOk) {
                    // #595 一：保存失败返回明确失败结果，且完整恢复旧 EditorUiState。
                    _uiState.value = oldUiState.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
                    contentExplicitlyCleared = oldContentExplicitlyCleared
                    saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
                    startSaveActor()
                    inputFrozen = false
                    return ChapterSwitchResult.SaveFailed(oldSession)
                }
                // #595 一：可见提交边界 1 — 保存完成后若已有更新请求，回滚并退出。
                if (!isLatest()) {
                    restoreAfterSwitch(oldSession, oldUiState, oldContentExplicitlyCleared)
                    return ChapterSwitchResult.Stale
                }
            } else {
                saveActorJob?.cancel()
                saveCommandChannel.close()
            }

            saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)

            val newSession = EditorSession(
                sessionId = java.util.UUID.randomUUID().toString(),
                projectId = projectId,
                volumeId = volumeId,
                chapterId = chapterId
            )
            currentSession = newSession
            // #595 二：章节提交后重置同步合并发射去重。
            syncMergeEmitDedup.reset()

            _uiState.value = _uiState.value.copy(
                loading = true,
                chapterTitle = chapterTitle,
                saveStatus = SaveStatus.Idle
            )
            contentExplicitlyCleared = false
            startSaveActor()
            reloadSettings()
            // #595 一：加载在事务内完成 — 只有内容就绪后才提交 Success。
            val loaded = loadChapter(newSession)
            if (!loaded) {
                currentSession = oldSession
                _uiState.value = oldUiState.copy(loading = false)
                contentExplicitlyCleared = oldContentExplicitlyCleared
                inputFrozen = false
                return ChapterSwitchResult.LoadFailed(ChapterKey(projectId, volumeId, chapterId))
            }
            // #595 一：可见提交边界 2 — 加载完成后若已有更新请求，回滚并退出。
            if (!isLatest()) {
                restoreAfterSwitch(oldSession, oldUiState, oldContentExplicitlyCleared)
                return ChapterSwitchResult.Stale
            }

            // #595 一：为 B 预准备 Rust session + 有效 snapshot/bind plan —
            // 在提交前完成，导航后编辑器立即可用；失败时 A 完全不变。
            val content = _uiState.value.content
            preparedTargetId = chapterTargetId(projectId, volumeId, chapterId)
            val prepared = prepareTargetSession(preparedTargetId, content)
            if (!prepared) {
                rollbackPreparedSession(preparedTargetId)
                preparedTargetId = null
                currentSession = oldSession
                _uiState.value = oldUiState.copy(loading = false)
                contentExplicitlyCleared = oldContentExplicitlyCleared
                inputFrozen = false
                return ChapterSwitchResult.LoadFailed(ChapterKey(projectId, volumeId, chapterId))
            }
            // #595 一：可见提交边界 3 — session 预准备后再次校验 requestId。
            if (!isLatest()) {
                rollbackPreparedSession(preparedTargetId)
                preparedTargetId = null
                restoreAfterSwitch(oldSession, oldUiState, oldContentExplicitlyCleared)
                return ChapterSwitchResult.Stale
            }

            // #595 一：提交完成后的独立操作 — recordRecentEdit/统计失败只记录
            // 自身错误，不得回滚已成功打开的正文。
            viewModelScope.launch {
                try {
                    withContext(Dispatchers.IO) {
                        workspaceRepository.recordRecentEdit(projectId, volumeId, chapterId)
                    }
                } catch (e: kotlinx.coroutines.CancellationException) {
                    throw e
                } catch (_: Exception) {
                    // 最近编辑记录失败不影响章节打开
                }
            }
            // Success：inputFrozen 保持 true，由新 pane 附着编辑器后解除。
            return ChapterSwitchResult.Success(newSession, _uiState.value.content)
        } catch (e: kotlinx.coroutines.CancellationException) {
            // #595 一：取消不是失败 — 恢复旧状态后向上重抛，让更新的请求
            // （若有）从一致状态开始；不允许把取消当普通加载失败回滚导航。
            if (preparedTargetId != null) {
                rollbackPreparedSession(preparedTargetId)
            }
            restoreAfterSwitch(oldSession, oldUiState, oldContentExplicitlyCleared)
            throw e
        } finally {
            // 注意：Success 路径不在此解除冻结 — 由 confirmEditorAttached 解除。
        }
    }

    /** 回滚到旧章节：释放预准备的临时 session，恢复 A 为活动 target。 */
    private suspend fun restoreAfterSwitch(
        oldSession: EditorSession?,
        oldUiState: EditorUiState,
        oldContentExplicitlyCleared: Boolean,
    ) {
        val coordinator = _sessionCoordinator
        if (coordinator != null && oldSession != null) {
            val oldTargetId = chapterTargetId(oldSession.projectId, oldSession.volumeId, oldSession.chapterId)
            if (!coordinator.isTargetRegistered(oldTargetId)) {
                coordinator.registerTargetMeta(oldTargetId, TextEditorProfile.DocumentBody, persistent = true)
            }
            val cursorUtf8 = oldUiState.content.toByteArray(Charsets.UTF_8).size
            try {
                coordinator.prepareSessionForEdit(oldTargetId, oldUiState.content, cursorUtf8)
            } catch (_: Exception) {
                // 恢复失败不阻塞状态回滚 — 新 pane 的 beginEdit 会再次尝试。
            }
        }
        currentSession = oldSession
        _uiState.value = oldUiState.copy(loading = false)
        contentExplicitlyCleared = oldContentExplicitlyCleared
        saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
        startSaveActor()
        scheduleAutoSave(_uiState.value.content)
        inputFrozen = false
    }

    /** 释放切换事务预准备的临时 session（过期/失败时）。 */
    private fun rollbackPreparedSession(targetId: String) {
        try {
            _sessionCoordinator?.releasePreparedTarget(targetId)
        } catch (_: Exception) {
            // 释放失败不阻塞回滚
        }
    }

    /**
     * #595 一：为章节预准备 Rust session — 注册 target 元数据并取得
     * 有效 snapshot/bind plan。返回 false 表示 session 不可用
     * （提交前检测，避免导航后才发现编辑器不可用）。
     */
    private fun prepareTargetSession(targetId: String, content: String): Boolean {
        val coordinator = _sessionCoordinator ?: return false
        if (!coordinator.isTargetRegistered(targetId)) {
            coordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
        }
        val cursorUtf8 = content.toByteArray(Charsets.UTF_8).size
        val bindInfo = coordinator.prepareSessionForEdit(targetId, content, cursorUtf8)
        return bindInfo != null && bindInfo.snapshot != null
    }

    fun initChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String) {
        val existing = currentSession
        if (existing != null && existing.projectId == projectId && existing.volumeId == volumeId && existing.chapterId == chapterId) {
            return
        }

        currentSession = EditorSession(
            sessionId = java.util.UUID.randomUUID().toString(),
            projectId = projectId,
            volumeId = volumeId,
            chapterId = chapterId
        )
        // #595 二：章节提交后重置同步合并发射去重（与 switchChapterLocked 一致）。
        syncMergeEmitDedup.reset()
        _uiState.value = _uiState.value.copy(
            loading = true,
            chapterTitle = chapterTitle
        )
        contentExplicitlyCleared = false
        startSaveActor()
        reloadSettings()
        viewModelScope.launch {
            loadChapter(currentSession!!)
        }
    }

    fun initErrorState(errorMessage: String) {
        _uiState.value = _uiState.value.copy(
            loading = false,
            content = errorMessage,
            editorEnabled = false,
            saveStatus = SaveStatus.Idle
        )
    }

    /**
     * #595 三: 读取 Android 系统无障碍"减少动画"设置（Animator 时长缩放为 0 时启用）。
     * 这是系统级偏好，不属于 Core 编辑器设置；初始值与 Core 默认一致（false）。
     */
    private fun isSystemReduceMotionEnabled(): Boolean {
        return try {
            val scale = android.provider.Settings.Global.getFloat(
                getApplication<android.app.Application>().contentResolver,
                android.provider.Settings.Global.ANIMATOR_DURATION_SCALE,
                1f,
            )
            scale == 0f
        } catch (_: Exception) {
            false
        }
    }

    fun reloadSettings() {
        viewModelScope.launch {
            val settings = settingsRepository.getLocalSettings()
            val syncable = settingsRepository.getSyncableSettings()
            val effectiveFontSize = if (syncable.fontSize > 0.0) {
                syncable.fontSize.toFloat()
            } else if (settings.editorFontSize > 0.0f) {
                settings.editorFontSize
            } else {
                16f
            }
            _uiState.value = _uiState.value.copy(
                settings = EditorSettingsState(
                    fontSize = effectiveFontSize,
                    lineSpacingMultiplier = settings.editorLineSpacingMultiplier,
                    autoIndentEnabled = settings.autoIndentEnabled,
                    autoIndentWidth = settings.autoIndentWidth,
                    typingAnimationEnabled = settings.editorTypingAnimationEnabled,
                    typingAnimationDurationMs = settings.editorTypingAnimationDurationMs.toLong(),
                    smoothCursorEnabled = settings.editorSmoothCursorEnabled,
                    smoothCursorDurationMs = settings.editorSmoothCursorDurationMs.toLong(),
                    coordinatedTextCursorAnimationEnabled = settings.editorCoordinatedTextCursorAnimationEnabled,
                    reduceMotion = isSystemReduceMotionEnabled(),
                    autoSaveEnabled = settings.autoSaveEnabled,
                    autoSaveDelayMs = settings.autoSaveDelayMs
                )
            )
        }
    }

    /**
     * #595 一：加载章节内容并在成功时一次性提交 loading=false/content/hash，
     * 并发布 RepositoryLoaded 文档事实（真实 hash）。
     * 返回是否加载成功；失败时调用方（switchChapter 事务）负责回退会话指针。
     * #595 一：recordRecentEdit/统计/诊断属于提交完成后的独立操作 —
     * 这里只保留不参与失败判定的统计与诊断；recordRecentEdit 移到提交后。
     */
    private suspend fun loadChapter(session: EditorSession): Boolean {
        isLoadingChapter = true
        val sessionId = session.sessionId
        return try {
            val result = withContext(kotlinx.coroutines.Dispatchers.IO) {
                workspaceRepository.getChapterContentWithMeta(session.projectId, session.volumeId, session.chapterId)
            }
            val content = result.first
            val meta = result.second

            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterLoad(
                session.projectId, session.chapterId,
                content.toByteArray(Charsets.UTF_8).size, "ok"
            )

            if (currentSession?.sessionId != sessionId) return false

            _uiState.value = _uiState.value.copy(
                loading = false,
                content = content,
                chapterHash = meta.hash,
                chapterNote = meta.note,
                editorEnabled = true,
                saveStatus = SaveStatus.Idle
            )
            // #595 二：Repository 加载完成即发布文档事实（真实 hash 锚点）。
            // 最终 revision 来自 reset 后的真实 Rust snapshot。
            emitDocumentFact(
                TargetDocumentFact(
                    targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                    text = content,
                    sourceVersion = DocumentVersion(contentHash = meta.hash),
                    baseVersion = DocumentVersion(),
                    origin = DocumentFactOrigin.REPOSITORY_LOAD,
                )
            )
            previousText = content
            initialWordCount = calculateWordCount(content)
            sessionStartTime = System.currentTimeMillis()
            updateStats(content)
            isLoadingChapter = false
            true
        } catch (e: Throwable) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterLoad(
                session.projectId, session.chapterId, 0, "error"
            )
            if (currentSession?.sessionId != sessionId) return false
            if (e is kotlinx.coroutines.CancellationException) {
                // #595 一：协程取消不是加载失败 — 恢复现场标记后向上重抛。
                isLoadingChapter = false
                throw e
            }
            isLoadingChapter = false
            _uiState.value = _uiState.value.copy(
                loading = false,
                editorEnabled = false,
                saveStatus = SaveStatus.Idle
            )
            emitErrorEvent(getApplication<Application>().getString(R.string.error_load_chapter_failed, e.message ?: ""))
            false
        }
    }

    /**
     * #595 二/三：同步合并事实已应用到 Rust session 后，同步更新 ViewModel 的
     * 正文/hash/保存状态/字数 — 禁止只更新 Rust session 不更新 ViewModel
     * （否则磁盘/Rust session/ViewModel 三份正文分裂）。
     */
    fun applyExternalContentToUi(targetId: String, text: String, fileHash: String) {
        val s = currentSession ?: return
        if (targetId != chapterTargetId(s.projectId, s.volumeId, s.chapterId)) return
        val current = _uiState.value
        _uiState.value = current.copy(
            content = text,
            chapterHash = fileHash,
            saveStatus = SaveStatus.Saved,
        )
        previousText = text
        updateStats(text)
    }

    /**
     * #595 二：同步下载与本地未保存编辑冲突 — 类型化冲突通知，
     * 不覆盖用户输入（reducer 已拒绝直接 reset）。
     */
    fun notifySyncMergeConflict() {
        viewModelScope.launch {
            emitErrorEvent(getApplication<Application>().getString(R.string.error_sync_document_conflict))
        }
    }

    fun onContentChanged(newContent: String) {
        val currentState = _uiState.value
        if (currentState.loading) return
        if (isLoadingChapter) return
        if (inputFrozen) return

        _uiState.value = currentState.copy(
            content = newContent,
            saveStatus = SaveStatus.Unsaved
        )

        contentExplicitlyCleared = false
        scheduleAutoSave(newContent)
        scheduleStatsUpdate(newContent)

        if (previousText != newContent) {
            reportWritingEvent(previousText, newContent)
            previousText = newContent
        }
    }

    private fun reportWritingEvent(oldText: String, newText: String) {
        val session = currentSession ?: return

        val nowMs = System.currentTimeMillis()
        if (statsLastEventMs == 0L || (nowMs - statsLastEventMs) > 5 * 60 * 1000) {
            statsSessionId = java.util.UUID.randomUUID().toString()
        }
        val durationSeconds = if (statsLastEventMs > 0L) {
            ((nowMs - statsLastEventMs) / 1000).toUInt()
        } else {
            0u
        }
        statsLastEventMs = nowMs

        workspaceRepository.processWritingEvent(
            statsDeviceId, "android", session.projectId, session.volumeId, session.chapterId, oldText, newText, durationSeconds, statsSessionId
        )
    }

    private fun scheduleAutoSave(content: String) {
        val session = currentSession ?: return
        autoSaveJob?.cancel()
        autoSaveJob = viewModelScope.launch {
            val delayMs = _uiState.value.settings.autoSaveDelayMs
            if (!_uiState.value.settings.autoSaveEnabled) return@launch
            delay(delayMs)
            if (_uiState.value.saveStatus == SaveStatus.Unsaved) {
                if (content.trim().isEmpty() && !contentExplicitlyCleared) {
                } else if (content.trim().isEmpty() && contentExplicitlyCleared) {
                    saveCommandChannel.trySend(SaveCommand.Clear(session))
                } else {
                    saveCommandChannel.trySend(SaveCommand.Save(content, session))
                }
            }
        }
    }

    private fun scheduleStatsUpdate(content: String) {
        viewModelScope.launch {
            delay(500)
            updateStats(content)
        }
    }

    private fun updateStats(content: String) {
        val currentWordCount = calculateWordCount(content)
        val sessionAdded = currentWordCount - initialWordCount
        val elapsedMinutes = (System.currentTimeMillis() - sessionStartTime) / 60000.0
        val speed = if (elapsedMinutes > 0 && sessionAdded > 0) {
            (sessionAdded / elapsedMinutes).toInt()
        } else {
            0
        }
        _uiState.value = _uiState.value.copy(
            wordCount = currentWordCount,
            sessionAdded = sessionAdded,
            speed = speed
        )
    }

    fun requestSave(): kotlinx.coroutines.Deferred<Boolean> {
        val content = _uiState.value.content
        val session = currentSession
        val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
        viewModelScope.launch {
            if (session != null) {
                if (content.trim().isEmpty() && contentExplicitlyCleared) {
                    val sendResult = saveCommandChannel.trySend(SaveCommand.Clear(session))
                    if (sendResult.isFailure) {
                        deferred.complete(false)
                        return@launch
                    }
                } else if (content.trim().isNotEmpty()) {
                    val sendResult = saveCommandChannel.trySend(SaveCommand.Save(content, session))
                    if (sendResult.isFailure) {
                        deferred.complete(false)
                        return@launch
                    }
                }
            }
            val flushReply = CompletableDeferred<Boolean>()
            val flushResult = saveCommandChannel.trySend(SaveCommand.Flush(flushReply))
            if (flushResult.isFailure) {
                deferred.complete(false)
                return@launch
            }
            val result = flushReply.await()
            deferred.complete(result)
        }
        return deferred
    }

    fun clearChapterContent() {
        val session = currentSession ?: return
        contentExplicitlyCleared = true
        saveCommandChannel.trySend(SaveCommand.Clear(session))
    }

    private var lastSaveResult: Boolean = true

    private fun startSaveActor() {
        saveActorJob?.cancel()
        saveActorJob = viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            for (cmd in saveCommandChannel) {
                when (cmd) {
                    is SaveCommand.Save -> {
                        if (cmd.session.sessionId == currentSession?.sessionId) {
                            lastSaveResult = performSave(cmd.content, cmd.session, isAutoSave = true)
                        }
                    }
                    is SaveCommand.Clear -> {
                        if (cmd.session.sessionId == currentSession?.sessionId) {
                            lastSaveResult = clearChapterContentInternal(cmd.session)
                        }
                    }
                    is SaveCommand.Flush -> {
                        cmd.reply.complete(lastSaveResult)
                    }
                }
            }
        }
    }

    private suspend fun clearChapterContentInternal(session: EditorSession): Boolean {
        return saveMutex.withLock {
            try {
                val result = workspaceRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
                when (result) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> {
                        _uiState.value = _uiState.value.copy(
                            content = "",
                            saveStatus = SaveStatus.Saved
                        )
                        previousText = ""
                        // #595 二：保存成功上报 — 清除 localDirty，同步合并可安全应用。
                        _sessionCoordinator?.markSaved(
                            chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                            DocumentVersion(contentHash = result.data?.contentHash ?: ""),
                        )
                        true
                    }
                    is com.xiwei.sujian.data.BridgeResult.Error -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                            _events.send(EditorEvent.ShowSaveFailedDialog(
                                getApplication<Application>().getString(R.string.error_empty_overwrite_dialog)))
                        } else {
                            _events.send(EditorEvent.ShowSaveFailedDialog(
                                getApplication<Application>().getString(R.string.error_save_failed, result.message)))
                        }
                        false
                    }
                    com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        false
                    }
                }
            } catch (e: Throwable) {
                _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                _events.send(EditorEvent.ShowSaveFailedDialog(
                    getApplication<Application>().getString(R.string.error_save_exception, e.message ?: "")))
                false
            }
        }
    }

    private suspend fun performSave(content: String, session: EditorSession, isAutoSave: Boolean): Boolean {
        if (content.trim().isEmpty()) {
            if (contentExplicitlyCleared) {
                return clearChapterContentInternal(session)
            }
            return false
        }

        var currentContent = content
        var currentIsAutoSave = isAutoSave
        var lastSaveSuccess = false
        val saveStartedAt = System.currentTimeMillis()

        while (true) {
            val contentToSave = currentContent
            saveMutex.withLock {
                val currentState = _uiState.value
                if (currentState.saveStatus == SaveStatus.Saving) {
                    pendingSaveContent = contentToSave
                    return false
                }

                _uiState.value = currentState.copy(saveStatus = SaveStatus.Saving)

                try {
                    val result = workspaceRepository.saveChapterContent(session.projectId, session.volumeId, session.chapterId, contentToSave)
                    when (result) {
                        is com.xiwei.sujian.data.BridgeResult.Success -> {
                            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Saved)
                            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                                session.projectId, session.chapterId,
                                contentToSave.toByteArray(Charsets.UTF_8).size, "ok",
                                System.currentTimeMillis() - saveStartedAt
                            )
                            // #595 二：保存成功上报 — 记录 lastSavedVersion 并清除
                            // localDirty（同步合并以磁盘为基础，可以安全应用）。
                            _sessionCoordinator?.markSaved(
                                chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                                DocumentVersion(contentHash = result.data?.contentHash ?: ""),
                            )
                            val pending = pendingSaveContent
                            pendingSaveContent = null
                            if (pending != null && pending != contentToSave) {
                                currentContent = pending
                                currentIsAutoSave = true
                                lastSaveSuccess = true
                            } else {
                                lastSaveSuccess = true
                                return true
                            }
                        }
                        is com.xiwei.sujian.data.BridgeResult.Error -> {
                            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                                session.projectId, session.chapterId,
                                contentToSave.toByteArray(Charsets.UTF_8).size, "error",
                                System.currentTimeMillis() - saveStartedAt
                            )
                            if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                                if (!currentIsAutoSave) {
                                    _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_empty_overwrite_dialog)))
                                } else {
                                    emitErrorEvent(getApplication<Application>().getString(R.string.error_empty_overwrite_save_blocked))
                                }
                            } else {
                                if (!currentIsAutoSave) {
                                    _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_failed, result.message)))
                                } else {
                                    emitErrorEvent(getApplication<Application>().getString(R.string.error_auto_save_failed, result.message))
                                }
                            }
                            return false
                        }
                        com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                                session.projectId, session.chapterId,
                                contentToSave.toByteArray(Charsets.UTF_8).size, "not_loaded",
                                System.currentTimeMillis() - saveStartedAt
                            )
                            if (!currentIsAutoSave) {
                                _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_native_not_loaded)))
                            }
                            return false
                        }
                    }
                } catch (e: Throwable) {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                        session.projectId, session.chapterId,
                        contentToSave.toByteArray(Charsets.UTF_8).size, "exception",
                        System.currentTimeMillis() - saveStartedAt
                    )
                    if (!currentIsAutoSave) {
                        _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_exception, e.message ?: "")))
                    } else {
                        emitErrorEvent(getApplication<Application>().getString(R.string.error_auto_save_exception, e.message ?: ""))
                    }
                    return false
                }
            }
            if (!lastSaveSuccess) return false
        }
    }

    fun updateChapterNote(newNote: String) {
        val session = currentSession ?: return

        viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            try {
                workspaceRepository.updateChapterNote(session.projectId, session.volumeId, session.chapterId, newNote)
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    _uiState.value = _uiState.value.copy(chapterNote = newNote)
                }
            } catch (e: Throwable) {
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    emitErrorEvent(getApplication<Application>().getString(R.string.error_update_chapter_note_failed, e.message ?: ""))
                }
            }
        }
    }

    fun onSettingsChanged() {
        reloadSettings()
    }

    override fun onCleared() {
        super.onCleared()
        syncObserverJob?.cancel()
        autoSaveJob?.cancel()
        saveCommandChannel.close()
        try {
            val session = currentSession
            val content = _uiState.value.content
            if (session != null) {
                if (content.isNotEmpty()) {
                    workspaceRepository.saveChapterContent(session.projectId, session.volumeId, session.chapterId, content)
                } else if (contentExplicitlyCleared) {
                    workspaceRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
                }
            }
        } catch (_: Exception) {
        }
        try {
            workspaceRepository.flushWritingStats()
        } catch (_: Exception) {
        }
        try {
            workspaceRepository.flushRecentEdits()
        } catch (_: Exception) {
        }
        // #595 三：ViewModel 销毁后不再有活动正文可 flush。
        WorkspaceDocumentGate.registerFlusher(null)
    }

    private fun calculateWordCount(text: String): Int {
        return workspaceRepository.calculateWordCount(text)
    }

    private suspend fun emitErrorEvent(message: String) {
        _events.send(EditorEvent.ToastMessage(message))
    }

    /**
     * #595 一：章节正文 target ID — 全局唯一命名空间，ViewModel 与窗口层共用。
     */
    fun chapterTargetId(projectId: String, volumeId: String, chapterId: String): String =
        "chapter-body:$projectId:$volumeId:$chapterId"

    /**
     * #595 一：显式 Factory — 从 [SujianAppDependencies]（进程级容器）
     * 注入同一组 Repository；删除 fallback getApplication() 路径。
     */
    class Factory(
        private val application: Application,
        private val deps: SujianAppDependencies,
        private val sessionCoordinator: EditorSessionCoordinator?,
    ) : ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            @Suppress("UNCHECKED_CAST")
            val vm = EditorViewModel(application)
            vm.initialize(
                deps.workspaceRepository,
                deps.settingsRepository,
                deps.syncStatusRepository,
                sessionCoordinator,
            )
            return vm as T
        }
    }
}
