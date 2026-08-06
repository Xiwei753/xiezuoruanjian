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
//! - **做**：UI 状态管理、自动保存调度、设置加载/应用、写作统计上报
//! - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 SujianEditorView 负责）
//! - **不直接调用 legacy JNI adapter**：只通过 Repository 和领域 Bridge 间接调用
//!
//! ## 关键流程
//!
//! 1. **章节加载**：`initChapter()` → `loadChapter()` → `WorkspaceRepository.getChapterContentWithMeta()`
//! 2. **自动保存**：`onContentChanged()` → `scheduleAutoSave()` → `performSave()`
//! 3. **设置同步**：`onSettingsChanged()` → `reloadSettings()` → 更新 `EditorSettingsState`
//! 4. **写作统计**：`onContentChanged()` → `reportWritingEvent()` → `WorkspaceRepository.processWritingEvent()`
//!
//! ## 线程模型
//!
//! - UI 操作在 `Dispatchers.Main`
//! - 文件 I/O 在 `Dispatchers.IO`
//! - 保存互斥锁 `saveMutex` 防止并发保存冲突

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.model.LocalSettings
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.CompletableDeferred
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
 * - [Success]：旧章节保存成功且新章节内容已加载提交（currentSession/content/loading 一致）。
 * - [SaveFailed]：旧章节保存失败，currentSession 仍指向旧章节，调用方必须回滚导航。
 * - [LoadFailed]：新章节加载失败，currentSession 已回退到旧章节，调用方必须回滚导航。
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
    /** #595 一：当前已加载章节正文的真实文件 hash（ChapterMeta.hash）—
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

    private var _workspaceRepository: WorkspaceRepository? = null
    private var _settingsRepository: SettingsRepository? = null
    private var _syncStatusRepository: com.xiwei.sujian.data.SyncStatusRepository? = null
    private val workspaceRepository: WorkspaceRepository get() = _workspaceRepository ?: WorkspaceRepository(getApplication())
    private val settingsRepository: SettingsRepository get() = _settingsRepository ?: SettingsRepository(getApplication())
    private val syncStatusRepository: com.xiwei.sujian.data.SyncStatusRepository? get() = _syncStatusRepository

    private val syncObserverScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var syncObserverJob: kotlinx.coroutines.Job? = null

    fun initialize(workspaceRepo: WorkspaceRepository, settingsRepo: SettingsRepository) {
        if (_workspaceRepository != null) return
        _workspaceRepository = workspaceRepo
        _settingsRepository = settingsRepo
    }

    /**
     * #595 二：带同步状态观察的初始化 — 同步完成后检查当前章节磁盘内容是否变更，
     * 若变更则发出 [EditorDocumentUpdate.SyncMerged] 事件，WritingPane 据此执行
     * 一次 Core reset 以反映同步合并后的新正文。
     */
    fun initialize(
        workspaceRepo: WorkspaceRepository,
        settingsRepo: SettingsRepository,
        syncStatusRepo: com.xiwei.sujian.data.SyncStatusRepository,
    ) {
        if (_workspaceRepository == null) {
            _workspaceRepository = workspaceRepo
            _settingsRepository = settingsRepo
        }
        if (_syncStatusRepository == syncStatusRepo) return
        _syncStatusRepository = syncStatusRepo
        syncObserverJob?.cancel()
        syncObserverJob = syncObserverScope.launch {
            var lastSynced = false
            syncStatusRepo.state.collect { state ->
                val isSynced = state == com.xiwei.sujian.model.SyncIndicatorState.Synced
                if (isSynced && !lastSynced) {
                    checkSyncMergedChapter()
                }
                lastSynced = isSynced
            }
        }
    }

    private suspend fun checkSyncMergedChapter() {
        val session = currentSession ?: return
        if (inputFrozen || _uiState.value.loading) return
        try {
            val (content, meta) = workspaceRepository.getChapterContentWithMeta(
                session.projectId, session.volumeId, session.chapterId
            )
            val currentHash = _uiState.value.chapterHash
            if (meta.hash.isNotEmpty() && meta.hash != currentHash && content != _uiState.value.content) {
                val syncState = try { settingsRepository.loadSyncState() } catch (_: Exception) { null }
                emitDocumentUpdate(
                    com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.SyncMerged(
                        targetId = "chapter-body:${session.projectId}:${session.volumeId}:${session.chapterId}",
                        text = content,
                        manifestRevision = syncState?.lastSyncTime ?: System.currentTimeMillis(),
                        fileHash = meta.hash,
                        revision = 0L,
                        contentVersion = contentVersionSupplier(),
                    )
                )
            }
        } catch (_: kotlinx.coroutines.CancellationException) {
            throw kotlinx.coroutines.CancellationException()
        } catch (_: Exception) {
            // 同步合并检查失败不阻塞用户操作
        }
    }

    private val _uiState = MutableStateFlow(EditorUiState())
    val uiState: StateFlow<EditorUiState> = _uiState.asStateFlow()

    private val _events = Channel<EditorEvent>(Channel.BUFFERED)
    val events = _events.receiveAsFlow()

    // #595 一/二：Repository 真实来源的正文更新事件流 — 按 target 分区的最新事件
    // 总线（带 replay 语义：新 collector 立即拿到当前最新事件）。
    // 不再使用单消费者 Channel.receiveAsFlow()：章节快速重组或 collector 短暂
    // 重叠时，事件可能被错误页面取走；旧事件由 reducer 的 contentVersion 比较丢弃。
    private val documentUpdateBus = TargetDocumentUpdateBus()

    private fun emitDocumentUpdate(update: com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate) {
        documentUpdateBus.emit(update)
    }

    /** 指定 target 的正文更新事件流 — 只收该 target 的事件，新 collector 先收到最新事件。 */
    fun documentUpdates(targetId: String): kotlinx.coroutines.flow.Flow<com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate> =
        documentUpdateBus.updates(targetId)

    // #595 二：正文版本号源 — 默认用本地 AtomicLong，由 WritingPane 注入
    // Coordinator 的 nextContentVersion() 以共享全局递增序列。
    private val localContentVersionSource = java.util.concurrent.atomic.AtomicLong(0L)
    @Volatile
    private var contentVersionSupplier: () -> Long = { localContentVersionSource.incrementAndGet() }
    fun setContentVersionSupplier(supplier: () -> Long) {
        contentVersionSupplier = supplier
    }

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
    private var inputFrozen = false

    // #595 一：章节切换串行门 — 同一时间只允许一个切换事务执行；
    // 请求序号保证旧请求不得在新请求之后提交或报告失败（过期 → Stale）。
    private val chapterSwitchGate = ChapterSwitchGate()

    /**
     * #595 一：章节切换统一事务入口（手机竖屏、大屏工作台共用）。
     *
     * 固定顺序：串行门 → requestId 校验 → 冻结旧章节输入 → 保存旧章节 →
     * 加载新章节 → 一次性提交 active/正文/hash/标题/session → 返回 Success。
     * 调用方只在 Success 之后提交业务选择和 Navigator；失败时旧 EditorUiState
     * 完整恢复、Navigator 完全不变化。
     *
     * - 过期请求（锁内发现更新的请求已排队）直接返回 [ChapterSwitchResult.Stale]，
     *   不保存、不加载、不改变任何状态；
     * - [kotlinx.coroutines.CancellationException] 重新抛出并恢复旧状态，
     *   不得当作普通加载失败处理。
     */
    suspend fun requestOpenChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String): ChapterSwitchResult {
        return when (val gate = chapterSwitchGate.runLatest {
            switchChapterLocked(projectId, volumeId, chapterId, chapterTitle)
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

    private suspend fun switchChapterLocked(projectId: String, volumeId: String, chapterId: String, chapterTitle: String): ChapterSwitchResult {
        val oldSession = currentSession
        // #595 一：事务回滚需要完整的旧 EditorUiState — 保存/加载失败时整体恢复，
        // 不允许 content/hash/note/editorEnabled/saveStatus/loading 残留新章节
        // 加载过程的痕迹（旧缺陷：只恢复 currentSession 和标题）。
        val oldUiState = _uiState.value
        val oldContentExplicitlyCleared = contentExplicitlyCleared

        if (oldSession != null && oldSession.projectId == projectId && oldSession.volumeId == volumeId && oldSession.chapterId == chapterId) {
            return ChapterSwitchResult.Success(oldSession, _uiState.value.content)
        }

        inputFrozen = true
        try {
            // #595 一/转场：切换章节时同步置 loading — 在旧章节保存完成前就隐藏编辑器。
            // 防止 WritingPane 在保存窗口期（loading 仍为 false）用旧章节正文对目标章节
            // 执行 beginEdit/resetPersistentSession，造成新章节 session 被旧内容短暂占用
            // 后再被外部替换协议重写（多余 Core reset、revision 跳动）。
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
                    // #595 一：保存失败返回明确失败结果，且完整恢复旧 EditorUiState —
                    // 调用方不导航，页面和状态都停在旧章节，不会出现“新标题 + 旧正文”分裂。
                    _uiState.value = oldUiState.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
                    contentExplicitlyCleared = oldContentExplicitlyCleared
                    saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
                    startSaveActor()
                    return ChapterSwitchResult.SaveFailed(oldSession)
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

            _uiState.value = _uiState.value.copy(
                loading = true,
                chapterTitle = chapterTitle,
                saveStatus = SaveStatus.Idle
            )
            contentExplicitlyCleared = false
            startSaveActor()
            reloadSettings()
            // #595 一：加载在事务内完成 — 只有内容就绪后才提交 Success。
            // 加载失败时整体恢复旧 EditorUiState 与旧 session，返回 LoadFailed；
            // 调用方不导航，Navigator 不变化。
            val loaded = loadChapter(newSession)
            if (!loaded) {
                // #595 一：不能让 currentSession 指向未加载内容的章节 —
                // 否则再次输入/自动保存会把旧正文写入新章节。
                currentSession = oldSession
                _uiState.value = oldUiState.copy(loading = false)
                contentExplicitlyCleared = oldContentExplicitlyCleared
                return ChapterSwitchResult.LoadFailed(ChapterKey(projectId, volumeId, chapterId))
            }
            return ChapterSwitchResult.Success(newSession, _uiState.value.content)
        } catch (e: kotlinx.coroutines.CancellationException) {
            // #595 一：取消不是失败 — 恢复旧状态后向上重抛，让更新的请求
            // （若有）从一致状态开始；不允许把取消当普通加载失败回滚导航。
            currentSession = oldSession
            _uiState.value = oldUiState
            contentExplicitlyCleared = oldContentExplicitlyCleared
            saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
            startSaveActor()
            scheduleAutoSave(_uiState.value.content)
            throw e
        } finally {
            inputFrozen = false
        }
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
     * #595 一：加载章节内容并在成功时一次性提交 loading=false/content/hash。
     * 返回是否加载成功；失败时调用方（switchChapter 事务）负责回退会话指针。
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
            // #595 一：Repository 加载完成即发出真实来源事件（真实 hash）。
            // WritingPane 据此决定是否执行外部替换协议；revision 只作参考，
            // 最终 SessionState.revision 来自 reset 后的真实 Rust snapshot。
            emitDocumentUpdate(
                com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.RepositoryLoaded(
                    targetId = "chapter-body:${session.projectId}:${session.volumeId}:${session.chapterId}",
                    text = content,
                    fileHash = meta.hash,
                    revision = 0L,
                    contentVersion = contentVersionSupplier(),
                )
            )
            previousText = content
            initialWordCount = calculateWordCount(content)
            sessionStartTime = System.currentTimeMillis()
            updateStats(content)
            isLoadingChapter = false

            withContext(kotlinx.coroutines.Dispatchers.IO) {
                workspaceRepository.recordRecentEdit(session.projectId, session.volumeId, session.chapterId)
            }
            true
        } catch (e: Throwable) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterLoad(
                session.projectId, session.chapterId, 0, "error"
            )
            if (currentSession?.sessionId != sessionId) return false
            if (e is kotlinx.coroutines.CancellationException) {
                // #595 一：协程取消不是加载失败 — 恢复现场标记后向上重抛，
                // 不得当作普通加载失败回滚导航。
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
    }

    private fun calculateWordCount(text: String): Int {
        return workspaceRepository.calculateWordCount(text)
    }

    private suspend fun emitErrorEvent(message: String) {
        _events.send(EditorEvent.ToastMessage(message))
    }
}
