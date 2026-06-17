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
//! - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 WriterEditText 负责）
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
import androidx.lifecycle.viewModelScope
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

enum class SaveStatus {
    Idle,
    Unsaved,
    Saving,
    Saved,
    SaveFailed
}

data class EditorSettingsState(
    val fontSize: Float = 16f,
    val lineSpacingMultiplier: Float = 1.5f,
    val autoIndentEnabled: Boolean = true,
    val autoIndentWidth: Float = 2.0f,
    val typingAnimationEnabled: Boolean = false,
    val typingAnimationDurationMs: Long = 100L,
    val smoothCursorEnabled: Boolean = true,
    val smoothCursorDurationMs: Long = 80L,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L
)

data class EditorUiState(
    val loading: Boolean = false,
    val content: String = "",
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

    private val workspaceRepository = WorkspaceRepository(application)
    private val settingsRepository = SettingsRepository(application)

    private val _uiState = MutableStateFlow(EditorUiState())
    val uiState: StateFlow<EditorUiState> = _uiState.asStateFlow()

    private val _events = Channel<EditorEvent>(Channel.BUFFERED)
    val events = _events.receiveAsFlow()

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    private var initialWordCount = 0
    private var sessionStartTime = System.currentTimeMillis()

    private val saveMutex = Mutex()
    private var pendingSaveContent: String? = null
    private var autoSaveJob: kotlinx.coroutines.Job? = null

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

    fun initChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String) {
        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        _uiState.value = _uiState.value.copy(
            loading = true,
            chapterTitle = chapterTitle
        )
        reloadSettings()
        loadChapter()
    }

    fun initErrorState(errorMessage: String) {
        _uiState.value = _uiState.value.copy(
            loading = false,
            content = errorMessage,
            editorEnabled = false,
            saveStatus = SaveStatus.Idle
        )
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
                    autoSaveEnabled = settings.autoSaveEnabled,
                    autoSaveDelayMs = settings.autoSaveDelayMs
                )
            )
        }
    }

    private fun loadChapter() {
        val pid = projectId ?: return
        val vid = volumeId ?: return
        val cid = chapterId ?: return

        viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            try {
                val result = workspaceRepository.getChapterContentWithMeta(pid, vid, cid)
                val content = result.first
                val meta = result.second

                launch(kotlinx.coroutines.Dispatchers.Main) {
                    isLoadingChapter = true
                    _uiState.value = _uiState.value.copy(
                        loading = false,
                        content = content,
                        chapterNote = meta.note,
                        editorEnabled = true,
                        saveStatus = SaveStatus.Idle
                    )
                    previousText = content
                    isLoadingChapter = false
                    initialWordCount = calculateWordCount(content)
                    sessionStartTime = System.currentTimeMillis()
                    updateStats(content)
                }

                workspaceRepository.recordRecentEdit(pid, vid, cid)
            } catch (e: Throwable) {
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    _uiState.value = _uiState.value.copy(
                        loading = false,
                        editorEnabled = false,
                        saveStatus = SaveStatus.Idle
                    )
                    emitErrorEvent("获取章节内容失败: ${e.message}")
                }
            }
        }
    }

    fun onContentChanged(newContent: String) {
        val currentState = _uiState.value
        if (currentState.loading) return

        _uiState.value = currentState.copy(
            content = newContent,
            saveStatus = SaveStatus.Unsaved
        )

        scheduleAutoSave(newContent)
        scheduleStatsUpdate(newContent)

        if (!isLoadingChapter && previousText != newContent) {
            reportWritingEvent(previousText, newContent)
            previousText = newContent
        }
    }

    private fun reportWritingEvent(oldText: String, newText: String) {
        val pid = projectId ?: return
        val vid = volumeId ?: return
        val cid = chapterId ?: return

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
            statsDeviceId, "android", pid, vid, cid, oldText, newText, durationSeconds, statsSessionId
        )
    }

    private fun scheduleAutoSave(content: String) {
        autoSaveJob?.cancel()
        autoSaveJob = viewModelScope.launch {
            val delayMs = _uiState.value.settings.autoSaveDelayMs
            if (!_uiState.value.settings.autoSaveEnabled) return@launch
            delay(delayMs)
            if (_uiState.value.saveStatus == SaveStatus.Unsaved) {
                performSave(content, isAutoSave = true)
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
        val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
        viewModelScope.launch {
            val result = performSave(content, isAutoSave = false)
            deferred.complete(result)
        }
        return deferred
    }

    private suspend fun performSave(content: String, isAutoSave: Boolean): Boolean {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid == null || vid == null || cid == null) return false

        saveMutex.withLock {
            val currentState = _uiState.value
            if (currentState.saveStatus == SaveStatus.Saving) {
                pendingSaveContent = content
                return false
            }

            _uiState.value = currentState.copy(saveStatus = SaveStatus.Saving)

            try {
                val result = workspaceRepository.saveChapterContent(pid, vid, cid, content)
                return when (result) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Saved)
                        pendingSaveContent = null
                        true
                    }
                    is com.xiwei.sujian.data.BridgeResult.Error -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                            if (!isAutoSave) {
                                _events.send(EditorEvent.ShowSaveFailedDialog("已阻止空内容覆盖，请检查内容是否丢失。"))
                            } else {
                                emitErrorEvent("已阻止空内容覆盖保存")
                            }
                        } else {
                            if (!isAutoSave) {
                                _events.send(EditorEvent.ShowSaveFailedDialog("保存失败: ${result.message}"))
                            } else {
                                emitErrorEvent("自动保存失败: ${result.message}")
                            }
                        }
                        false
                    }
                    com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        if (!isAutoSave) {
                            _events.send(EditorEvent.ShowSaveFailedDialog("保存失败: Native库未加载"))
                        }
                        false
                    }
                }
            } catch (e: Throwable) {
                _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                if (!isAutoSave) {
                    _events.send(EditorEvent.ShowSaveFailedDialog("保存异常: ${e.message}"))
                } else {
                    emitErrorEvent("自动保存异常: ${e.message}")
                }
                return false
            }
        }
    }

    fun updateChapterNote(newNote: String) {
        val pid = projectId ?: return
        val vid = volumeId ?: return
        val cid = chapterId ?: return

        viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            try {
                workspaceRepository.updateChapterNote(pid, vid, cid, newNote)
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    _uiState.value = _uiState.value.copy(chapterNote = newNote)
                }
            } catch (e: Throwable) {
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    emitErrorEvent("更新章节备注失败: ${e.message}")
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
    }

    private fun calculateWordCount(text: String): Int {
        return workspaceRepository.calculateWordCount(text)
    }

    private suspend fun emitErrorEvent(message: String) {
        _events.send(EditorEvent.ToastMessage(message))
    }
}
