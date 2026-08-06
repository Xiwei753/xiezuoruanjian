package com.xiwei.sujian.editor.v2.coordinator

import androidx.compose.runtime.Immutable

/**
 * #595 一：带来源和 revision 的唯一正文更新协议。
 *
 * 替代 WritingPane 中 localContentGeneration / lastSeenContentGeneration /
 * externalContentHash / String.hashCode() 启发式来源判断。
 *
 * - [LocalInput]：IME/键盘输入经 Rust EditResult 产生，携带 transactionId。
 *   会话层先更新唯一 SessionState，再通知 ViewModel 保存；UI 回显带同一 revision，
 *   WritingPane 发现 revision 已应用，只更新保存状态，不 reset session。
 * - [ExternalReplace]：Repository/Sync/Undo/ProgrammaticReplace 产生。
 *   与当前 Rust snapshot revision/content 比较，只有确认是新的外部版本时
 *   执行 replace/reset 协议。
 */
@Immutable
sealed interface EditorDocumentUpdate {
    val targetId: String
    val text: String
    val revision: Long

    @Immutable
    data class LocalInput(
        override val targetId: String,
        override val text: String,
        override val revision: Long,
        val transactionId: Long,
        val operationKind: EditorOperationKind = EditorOperationKind.INSERT,
    ) : EditorDocumentUpdate

    @Immutable
    data class ExternalReplace(
        override val targetId: String,
        override val text: String,
        override val revision: Long,
        val source: ExternalSource,
    ) : EditorDocumentUpdate
}

@Immutable
enum class ExternalSource {
    REPOSITORY_LOAD,
    SYNC_MERGE,
    UNDO_RESTORE,
    PROGRAMMATIC_REPLACE,
}

@Immutable
enum class EditorOperationKind {
    INSERT,
    DELETE,
    REPLACE,
    SELECTION,
    LINE_BREAK,
    COMPOSITION,
}
