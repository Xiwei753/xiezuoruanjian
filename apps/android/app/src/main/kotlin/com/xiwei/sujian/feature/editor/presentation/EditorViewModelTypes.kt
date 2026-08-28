package com.xiwei.sujian.feature.editor.presentation

/**
 * 编辑器 ViewModel 的类型声明 - 从 EditorViewModel 拆分以降低单文件复杂度。
 */

enum class SaveStatus {
    Idle,
    Unsaved,
    Saving,
    Saved,
    SaveFailed,
}

data class EditorSession(
    val sessionId: String,
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)

sealed interface ChapterSwitchResult {
    // #624 评论11 第5项：Success 不再携带正文 — 生产调用方只判断 Success 并不消费
    // 这份正文；继续暴露只会让后续代码再次误把 load-only UI 字段当编辑正文真值。
    // #641：当前编辑正文由 TextFieldState / Rust snapshot 负责。
    data class Success(val session: EditorSession) : ChapterSwitchResult

    data class SaveFailed(val current: EditorSession) : ChapterSwitchResult

    data class LoadFailed(val requested: ChapterKey) : ChapterSwitchResult

    data object Stale : ChapterSwitchResult
}

data class ChapterKey(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)

sealed class SaveCommand {
    /**
     * #624 评论12 第2项：保存命令携带本次权威 [DocumentOperationLease] —
     * 完成提交（回执/markSaved/UI 状态）只消费 lease 字段，不重读当前输入 lease。
     */
    data class Save(
        val content: String,
        val session: EditorSession,
        val lease: com.xiwei.sujian.feature.editor.session.DocumentOperationLease,
    ) : SaveCommand()

    data class Clear(
        val session: EditorSession,
        val lease: com.xiwei.sujian.feature.editor.session.DocumentOperationLease,
    ) : SaveCommand()

    data class Flush(
        val lease: com.xiwei.sujian.feature.editor.session.DocumentOperationLease,
        val reply: kotlinx.coroutines.CompletableDeferred<Boolean>,
    ) : SaveCommand()
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
    val autoSaveDelayMs: Long = 1500L,
)

data class EditorUiState(
    val loading: Boolean = false,
    val content: String = "",
    val chapterHash: String = "",
    val chapterTitle: String = "",
    val chapterNote: String? = null,
    val saveStatus: SaveStatus = SaveStatus.Idle,
    val wordCount: Int = 0,
    val sessionAdded: Int = 0,
    val speed: Int = 0,
    val errorMessage: String? = null,
    val editorEnabled: Boolean = true,
    val settings: EditorSettingsState = EditorSettingsState(),
    // #630 评论 5327560790: 持久化设置已加载完成 — 不用"默认值恰好存在"冒充已加载。
    // 只有 settingsReady = true 才允许构造 EditorTypography 并 beginEdit。
    val settingsReady: Boolean = false,
)

sealed class EditorEvent {
    data class ToastMessage(val message: String) : EditorEvent()

    data class ShowSaveFailedDialog(val message: String) : EditorEvent()
}
