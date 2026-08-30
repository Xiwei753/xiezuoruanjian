package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.feature.editor.projection.ProjectionSnapshot

/**
 * #595 四：按 target 保存完整状态的会话记录 — 会话层唯一的持久事实存储。
 *
 * 替代旧并行 map 集合（targetProfiles / targetPersistentFlags /
 * persistentSessionIds / targetDecorations / projectionSnapshots 五份 map）。
 *
 * 语义：
 * - 每个活动过的 target 都有 [EditorSessionRecord]（包括非持久 target）—
 *   sessionId 属于所有活动 session，"是否持久"只决定 detach/close 时是否
 *   保留记录与 Rust session，不能决定记录里是否保存 ID；
 * - 窗口重绑只更新 binding 相关字段，正文版本、hash、transaction 和 selection
 *   保留（配置变化后 Undo/Redo、composition、光标状态继续保留）；
 * - [activeTargetId] 是窗口层唯一活动目标；Detached 时窗口无活动目标，
 *   但记录保留等待重新附着。
 */
@Immutable
data class EditorSessionRecord(
    val targetId: String,
    /** 所有活动 session 都记录 ID；非持久 target 同样有值。 */
    val sessionId: ULong = 0UL,
    /** 是否持久 — 只决定 detach/close 时的保留策略。 */
    val persistent: Boolean = false,
    val profile: TextEditorProfile = TextEditorProfile(),
    /** 完整文档事实（正文/revision/选区/版本/localDirty/事务）。 */
    val documentState: DocumentState = DocumentState(),
    val decorations: TargetDecorations = TargetDecorations(),
    val projection: ProjectionSnapshot = ProjectionSnapshot(),
) {
    fun withDocumentState(transform: (DocumentState) -> DocumentState): EditorSessionRecord =
        copy(documentState = transform(documentState))

    fun withSelection(
        anchorUtf8: Int,
        headUtf8: Int,
    ): EditorSessionRecord =
        copy(
            documentState =
                documentState.copy(
                    selectionAnchorUtf8 = anchorUtf8,
                    selectionHeadUtf8 = headUtf8,
                ),
        )
}

/**
 * #595 四：会话记录存储 — Map<TargetId, EditorSessionRecord>。
 *
 * #624 评论17 问题1：删除旧 [activeTargetId] 字段 — 真正活动目标已在
 * [EditorSessionState.activeTargetId]，Store 这份是第二状态，只会形成分裂。
 *
 * 线程安全：所有读写都通过 [EditorSessionCoordinator.readSession] /
 * [EditorSessionCoordinator.mutateSession] 的单一临界区执行，不存在并行写入。
 * 生产代码不得在 gateway 之外直接调用 record/allRecords/isRegistered。
 */
class EditorSessionStore {
    private val records = mutableMapOf<String, EditorSessionRecord>()

    fun record(targetId: String): EditorSessionRecord? = records[targetId]

    fun allRecords(): List<EditorSessionRecord> = records.values.toList()

    fun isRegistered(targetId: String): Boolean = records.containsKey(targetId)

    internal fun put(record: EditorSessionRecord) {
        records[record.targetId] = record
    }

    internal fun update(
        targetId: String,
        transform: (EditorSessionRecord) -> EditorSessionRecord,
    ) {
        val current = records[targetId] ?: return
        records[targetId] = transform(current)
    }

    internal fun remove(targetId: String): EditorSessionRecord? = records.remove(targetId)

    internal fun clear() {
        records.clear()
    }

    val size: Int get() = records.size
}
