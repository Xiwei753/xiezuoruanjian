package com.xiwei.sujian.editor.v2.coordinator

import androidx.compose.runtime.Immutable

/**
 * #595 二：稳定文档版本 — 来自 Repository/Core 的真实锚点，不是进程内事件序号。
 *
 * 替代旧 contentVersion（AtomicLong 事件序号）：旧实现只能说明"哪个事件较晚被
 * Android 观察到"，不能说明磁盘或同步内容哪个更新；本地已编辑到 B、旧磁盘读取 A
 * 较晚返回时会被错误当作新版本覆盖 B。
 *
 * 锚点：
 * - [contentHash]：章节正文文件的真实 hash（ChapterMeta.hash / save receipt
 *   contentHash），由 Core 计算维护 — 唯一稳定的内容指纹；
 * - [repositoryRevision]：Repository 侧版本号（Core 无独立章节 revision 时保持 0，
 *   仅当同一锚点可比较时参与新旧判定）；
 * - [syncManifestRevision]：同步 manifest 锚点（lastSyncTime / lastSyncedCommit 派生），
 *   只有两侧都携带时才参与新旧判定。
 *
 * 判定规则（[EditorSessionCoordinator.shouldApplyExternalContent]）：
 * - 同 [contentHash] 重放 → 忽略（幂等）；
 * - 可比较且旧于 committedVersion → 忽略；
 * - localDirty=true → 冲突，禁止直接 reset；
 * - 其余不同版本 → 可应用。
 */
@Immutable
data class DocumentVersion(
    val contentHash: String = "",
    val repositoryRevision: Long = 0L,
    val syncManifestRevision: Long? = null,
) {
    /** 没有任何版本锚点 — 调用方不得把空版本当作可应用事件。 */
    val isEmpty: Boolean
        get() = contentHash.isEmpty() && repositoryRevision == 0L && syncManifestRevision == null
}

/**
 * #595 二/四：每个 target 的完整文档事实 — 事件总线保存的是文档事实而非
 * "最后一个事件对象"；新 collector 读到的就是当前文档状态，重放旧命令不会
 * 再次执行副作用（同 sourceVersion 幂等）。
 *
 * 字段含义：
 * - [text]/[revision]：Rust session 的真实正文与 revision（snapshot 镜像）；
 * - [selectionAnchorUtf8]/[selectionHeadUtf8]：真实选区（UTF-8 字节）；
 * - [committedVersion]：最后应用的文档版本（本地输入不改变它；外部版本应用后更新）；
 * - [sessionBaseVersion]：Rust session 创建/重置时基于的文档版本（外部事件据此判断
 *   是否基于旧 base — 旧 base 且本地 dirty 时必须走三方合并/冲突，禁止直接 reset）；
 * - [lastSavedVersion]：最近一次保存成功的版本（无保存记录为 null）；
 * - [localDirty]：存在尚未落盘的本地编辑（本地输入置 true；外部版本应用或
 *   保存成功上报后置 false）；
 * - [lastAppliedTransactionId]：最后应用的编辑事务 ID。
 */
@Immutable
data class DocumentState(
    val text: String = "",
    val revision: Long = 0L,
    val selectionAnchorUtf8: Int = 0,
    val selectionHeadUtf8: Int = 0,
    val committedVersion: DocumentVersion = DocumentVersion(),
    val sessionBaseVersion: DocumentVersion = DocumentVersion(),
    val lastSavedVersion: DocumentVersion? = null,
    val localDirty: Boolean = false,
    val lastAppliedTransactionId: Long = 0L,
)

/** 纯数据选区快照（UTF-8 字节偏移）。 */
@Immutable
data class SelectionSnapshot(
    val anchorUtf8: Int = 0,
    val headUtf8: Int = 0,
)
