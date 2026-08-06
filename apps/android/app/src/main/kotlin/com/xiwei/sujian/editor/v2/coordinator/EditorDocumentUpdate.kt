package com.xiwei.sujian.editor.v2.coordinator

import androidx.compose.runtime.Immutable

/**
 * #595 一/二：带来源、contentVersion 和 revision 的唯一正文更新协议。
 *
 * 替代 WritingPane 中 localContentGeneration / lastSeenContentGeneration /
 * externalContentHash / String.hashCode() 启发式来源判断。
 *
 * - [LocalInput]：IME/键盘输入经 Rust EditResult 产生，携带 transactionId。
 *   会话层先更新唯一 SessionState，再通知 ViewModel 保存；UI 回显带同一 revision，
 *   WritingPane 发现 revision 已应用，只更新保存状态，不 reset session。
 * - [RepositoryLoaded]：Repository 真实来源的正文版本事件，携带真实 fileHash。
 * - [SyncMerged]：同步合并后磁盘正文变更事件，携带 manifestRevision。
 * - [UndoRestored]：撤销/恢复后正文变更事件，携带 snapshotId。
 * - [ProgrammaticReplace]：程序化批量替换后正文变更事件，携带 commandId。
 *
 * #595 二：已删除 ExternalReplace/ExternalSource — UI 不得再伪造
 * revision+1/source；外部更新只由真实来源事件驱动，最终 revision 永远来自
 * reset 后的真实 Rust snapshot。事件进入 reducer 后比较 targetId、
 * contentVersion、Rust revision 和 lastAppliedTransactionId，只有确认
 * 事件属于当前章节且版本更新时才对 Core 执行一次 reset。
 */
@Immutable
sealed interface EditorDocumentUpdate {
    val targetId: String
    val text: String
    val revision: Long
    /** #595 二：全局递增的正文版本号 — 由 [EditorSessionCoordinator.nextContentVersion] 产生。
     *  reducer 据此判断事件新旧，旧事件（contentVersion <= lastAppliedContentVersion）被跳过。 */
    val contentVersion: Long

    @Immutable
    data class LocalInput(
        override val targetId: String,
        override val text: String,
        override val revision: Long,
        override val contentVersion: Long,
        val transactionId: Long,
        val operationKind: EditorOperationKind = EditorOperationKind.INSERT,
        /** #595 四：本次编辑后 Rust 的真实选区（UTF-8 字节）。
         *  由 View 从 pipeline mirror 读取，会话层据此更新唯一 SessionState，
         *  不再沿用旧 selection。-1 表示调用方未携带（保留旧值）。 */
        val selectionAnchorUtf8: Int = -1,
        val selectionHeadUtf8: Int = -1,
    ) : EditorDocumentUpdate

    /**
     * #595 一：Repository 真实来源的正文更新事件。
     *
     * 由 [com.xiwei.sujian.ui.EditorViewModel] 在章节内容加载完成时发出，
     * 携带 ChapterMeta 的真实 fileHash 和版本号，不再由 UI 根据字符串差异伪造
     * revision/source。revision 只在会话已存在时用作新旧判断参考；
     * 最终进入 [EditorSessionState] 的 revision 永远来自 reset 后的真实 Rust snapshot。
     */
    @Immutable
    data class RepositoryLoaded(
        override val targetId: String,
        override val text: String,
        /** Repository 章节文件的真实 hash（ChapterMeta.hash）— 新旧判断依据。 */
        val fileHash: String,
        override val revision: Long,
        override val contentVersion: Long,
    ) : EditorDocumentUpdate

    /**
     * #595 二：同步合并后磁盘正文变更事件。
     *
     * 由 [com.xiwei.sujian.ui.EditorViewModel] 在同步完成且当前章节磁盘内容
     * 已变更时发出。携带同步 manifestRevision 用于版本比较。
     * WritingPane 收集后经 [EditorSessionCoordinator.shouldApplyExternalUpdate]
     * 判断是否执行一次 Core reset。
     */
    @Immutable
    data class SyncMerged(
        override val targetId: String,
        override val text: String,
        /** 同步 manifest 的版本号 — 用于版本比较。 */
        val manifestRevision: Long,
        /** 合并后磁盘文件的真实 hash — 幂等去重。 */
        val fileHash: String,
        override val revision: Long,
        override val contentVersion: Long,
    ) : EditorDocumentUpdate

    /**
     * #595 二：撤销/恢复后正文变更事件。
     *
     * 由 [EditorWindowHost] 在 SujianEditorView.performUndo/performRedo 产生
     * EditResult 后发出。携带 snapshotId 用于版本比较。
     * 撤销/恢复是本地发起的操作，revision 来自 Rust EditResult，
     * 但来源被类型化以区分于普通本地输入。
     */
    @Immutable
    data class UndoRestored(
        override val targetId: String,
        override val text: String,
        /** Rust 快照 ID — 用于版本比较。 */
        val snapshotId: Long,
        override val revision: Long,
        override val contentVersion: Long,
        val transactionId: Long,
        val selectionAnchorUtf8: Int = -1,
        val selectionHeadUtf8: Int = -1,
    ) : EditorDocumentUpdate

    /**
     * #595 二：程序化批量替换后正文变更事件。
     *
     * 由 [EditorWindowHost] 在 applyTargetCommand(ReplaceAll) 产生
     * EditResult 后发出。携带 commandId 用于版本比较。
     * 程序化替换是本地发起的操作，revision 来自 Rust EditResult，
     * 但来源被类型化以区分于普通本地输入。
     */
    @Immutable
    data class ProgrammaticReplace(
        override val targetId: String,
        override val text: String,
        /** 程序化命令 ID — 用于版本比较。 */
        val commandId: Long,
        override val revision: Long,
        override val contentVersion: Long,
        val transactionId: Long,
        val selectionAnchorUtf8: Int = -1,
        val selectionHeadUtf8: Int = -1,
    ) : EditorDocumentUpdate
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

/**
 * #595 解决二：把 Core 的 [uniffi.writer_core.EditorOperationKindDto] 映射为
 * 平台 [EditorOperationKind]，随 [EditorDocumentUpdate.LocalInput] 透传给会话层。
 *
 * CURSOR_ONLY 对应选区移动（SELECTION）；COMPOSITION_* 统一归为 COMPOSITION；
 * LOAD/FORMAT 属于内容替换语义（REPLACE）。
 */
fun uniffi.writer_core.EditorOperationKindDto.toEditorOperationKind(): EditorOperationKind = when (this) {
    uniffi.writer_core.EditorOperationKindDto.INSERT -> EditorOperationKind.INSERT
    uniffi.writer_core.EditorOperationKindDto.DELETE -> EditorOperationKind.DELETE
    uniffi.writer_core.EditorOperationKindDto.REPLACE -> EditorOperationKind.REPLACE
    uniffi.writer_core.EditorOperationKindDto.CURSOR_ONLY -> EditorOperationKind.SELECTION
    uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE,
    uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
    uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL -> EditorOperationKind.COMPOSITION
    uniffi.writer_core.EditorOperationKindDto.LOAD -> EditorOperationKind.REPLACE
    uniffi.writer_core.EditorOperationKindDto.FORMAT -> EditorOperationKind.REPLACE
}
