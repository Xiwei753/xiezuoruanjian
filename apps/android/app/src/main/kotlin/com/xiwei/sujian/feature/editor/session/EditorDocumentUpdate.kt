package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.feature.editor.window.EditorWindowHost

/**
 * #595 一/二/四 / #624 评论9：类型化正文更新协议。
 *
 * ## 直接回调事件（View → 会话层，不走事件总线）
 *
 * - [LocalInput]：IME/键盘输入经 Rust EditResult 产生，携带 transactionId。
 * - [UndoRestored]：撤销/恢复后正文变更，携带 snapshotId/transactionId。
 * - [ProgrammaticReplace]：程序化替换后正文变更，携带 commandId/transactionId。
 *
 * 这些事件在 [EditorWindowHost.installContentCallback] 中由 View 的
 * onLocalEdit / onExternalEdit 同步回调直接送入会话层 reducer，
 * 不经 [com.xiwei.sujian.feature.editor.presentation.TargetDocumentUpdateBus]。
 *
 * #624 评论9：热路径不再传整章 String — 只携带 [contentChanged]/[contentDelta]
 * 增量信号；正文只在冷路径（load/snapshot/save/sync）经 [TargetSnapshot.text]
 * 一次性 materialize。dirty 判定由 [contentChanged] 替代旧 `text != previousDoc.text`
 * 字符串比较。
 *
 * ## 事件总线事实（[TargetDocumentFact]）
 *
 * Repository 加载与同步合并走按 target 分区的事件总线。总线保存每个 target 的
 * 完整文档事实（text + sourceVersion + baseVersion），新 collector 读到的是当前
 * 文档事实；同 sourceVersion 重放由 reducer 幂等忽略，不会再次执行副作用。
 *
 * #595 二：已删除 ExternalReplace/ExternalSource — UI 不得伪造 revision/source；
 * 外部更新只由真实来源事实驱动，最终 revision 永远来自 reset 后的真实 Rust snapshot。
 * 已删除 contentVersion（进程内事件序号）— 新旧判断由 [DocumentVersion] 锚点完成。
 */
@Immutable
sealed interface EditorDocumentUpdate {
    val targetId: String
    val revision: Long
    val transactionId: Long

    /** 编辑后的真实选区（UTF-8 字节）；-1 表示调用方未携带（保留旧值）。 */
    val selectionAnchorUtf8: Int
    val selectionHeadUtf8: Int

    /**
     * #595 二：窗口绑定时的输入 lease — 会话层 reducer 只接受仍匹配当前
     * 活动 target/session/epoch 的事件；章节切换提交后旧 View 晚到的
     * 事件被拒绝，不能写入新章节的会话。
     */
    val lease: EditorInputLease

    /**
     * #624 评论9：本次编辑是否真改了正文（displayPatches 非空）。
     * 替代旧 `text != previousDoc.text` 字符串比较 — 热路径不传整章 String。
     */
    val contentChanged: Boolean

    /** #624 评论9：增量字符统计 — 不依赖整章 String。 */
    val contentDelta: EditorContentDelta

    @Immutable
    data class LocalInput(
        override val targetId: String,
        override val revision: Long,
        override val transactionId: Long,
        val operationKind: EditorOperationKind = EditorOperationKind.INSERT,
        /** #595 四：本次编辑后 Rust 的真实选区（UTF-8 字节）。
         *  由 View 从 pipeline mirror 读取，会话层据此更新唯一 SessionState。
         *  -1 表示调用方未携带（保留旧值）。 */
        override val selectionAnchorUtf8: Int = -1,
        override val selectionHeadUtf8: Int = -1,
        override val lease: EditorInputLease = EditorInputLease(targetId, 0UL, 0L),
        override val contentChanged: Boolean = true,
        override val contentDelta: EditorContentDelta = EditorContentDelta(),
    ) : EditorDocumentUpdate

    /**
     * #595 二：撤销/恢复后正文变更事件。
     *
     * 由 [EditorWindowHost] 在 SujianEditorView.performUndo/performRedo 产生
     * EditResult 后发出（PipelineOutput 携带来源，无可变侧信道）。
     * 撤销/恢复是本地发起的操作，revision 来自 Rust EditResult，
     * 但来源被类型化以区分于普通本地输入。
     */
    @Immutable
    data class UndoRestored(
        override val targetId: String,
        /** Rust 快照 ID — 用于版本比较。 */
        val snapshotId: Long,
        override val revision: Long,
        override val transactionId: Long,
        override val selectionAnchorUtf8: Int = -1,
        override val selectionHeadUtf8: Int = -1,
        override val lease: EditorInputLease = EditorInputLease(targetId, 0UL, 0L),
        override val contentChanged: Boolean = true,
        override val contentDelta: EditorContentDelta = EditorContentDelta(),
    ) : EditorDocumentUpdate

    /**
     * #595 二：程序化批量替换后正文变更事件。
     *
     * 由 [EditorWindowHost] 在 applyTargetCommand(ReplaceAll/Replace) 产生
     * EditResult 后发出（PipelineOutput 携带 PROGRAMMATIC 来源，无可变侧信道）。
     * 程序化替换是本地发起的操作，revision 来自 Rust EditResult，
     * 但来源被类型化以区分于普通本地输入。
     */
    @Immutable
    data class ProgrammaticReplace(
        override val targetId: String,
        /** 程序化命令 ID — 用于版本比较。 */
        val commandId: Long,
        override val revision: Long,
        override val transactionId: Long,
        override val selectionAnchorUtf8: Int = -1,
        override val selectionHeadUtf8: Int = -1,
        override val lease: EditorInputLease = EditorInputLease(targetId, 0UL, 0L),
        override val contentChanged: Boolean = true,
        override val contentDelta: EditorContentDelta = EditorContentDelta(),
    ) : EditorDocumentUpdate
}

/** 文档事实来源 — 决定事实应用后的 [EditorSessionOrigin]。 */
@Immutable
enum class DocumentFactOrigin {
    /** Repository 章节加载（真实 fileHash）。 */
    REPOSITORY_LOAD,

    /** 同步合并后磁盘正文变更。 */
    SYNC_MERGED,
}

/**
 * #595 二：事件总线保存的每 target 完整文档事实 — 不是"最后一个事件对象"。
 *
 * 由 [com.xiwei.sujian.feature.editor.presentation.EditorViewModel] 在章节加载完成 / 同步合并检测时发布。
 * WritingPane collector 经 [EditorSessionCoordinator.shouldApplyExternalContent]
 * 判断是否执行一次 Core reset；同 sourceVersion 重放幂等忽略。
 */
@Immutable
data class TargetDocumentFact(
    val targetId: String,
    val text: String,
    /** 本次事实的文档版本（contentHash + 同步锚点）。 */
    val sourceVersion: DocumentVersion,
    /** 本地正文基于的版本 — 用于判断外部更新是否基于旧 base。 */
    val baseVersion: DocumentVersion,
    val origin: DocumentFactOrigin,
    /** 仅供参考的 Rust revision；最终 revision 来自 reset 后的真实 snapshot。 */
    val revision: Long = 0L,
)

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
fun uniffi.writer_core.EditorOperationKindDto.toEditorOperationKind(): EditorOperationKind =
    when (this) {
        uniffi.writer_core.EditorOperationKindDto.INSERT -> EditorOperationKind.INSERT
        uniffi.writer_core.EditorOperationKindDto.DELETE -> EditorOperationKind.DELETE
        uniffi.writer_core.EditorOperationKindDto.REPLACE -> EditorOperationKind.REPLACE
        uniffi.writer_core.EditorOperationKindDto.CURSOR_ONLY -> EditorOperationKind.SELECTION
        uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE,
        uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
        uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL,
        -> EditorOperationKind.COMPOSITION
        uniffi.writer_core.EditorOperationKindDto.LOAD -> EditorOperationKind.REPLACE
        uniffi.writer_core.EditorOperationKindDto.FORMAT -> EditorOperationKind.REPLACE
    }
