package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable

/**
 * #595 二/五：稳定文档版本 — 来自 Repository/Core 的真实锚点，不是进程内事件序号。
 *
 * 替代旧 contentVersion（AtomicLong 事件序号）：旧实现只能说明"哪个事件较晚被
 * Android 观察到"，不能说明磁盘或同步内容哪个更新；本地已编辑到 B、旧磁盘读取 A
 * 较晚返回时会被错误当作新版本覆盖 B。
 *
 * 锚点：
 * - [contentHash]：章节正文文件的真实 hash（ChapterMeta.hash / save receipt
 *   contentHash），由 Core 计算维护 — 唯一稳定的内容指纹；
 * - [repositoryRevision]：Repository 侧单调版本号（Core 尚无独立章节 revision
 *   时保持 0，仅当两侧都非零时参与新旧判定）；
 * - [syncCommitId]：同步真实 commit/manifest ID；Core provider-neutral 重构后
 *   不再暴露 commit hash，当前恒为 null，保留字段以备未来 provider 重新提供。
 *   不是时间锚点（lastSyncTime 不表达任何因果顺序，不得参与版本比较）；
 * - [parentVersion]：本版本基于的父版本 — 同步合并结果携带 parentVersion=
 *   同步前磁盘版本；incoming 的父版本链包含 committed 时二者可比较
 *   （incoming 是 committed 的后代），否则不同版本不可比较，
 *   不得默认 Apply（进入重新读取/三方合并/冲突）。
 *
 * 判定规则（[EditorSessionCoordinator.shouldApplyExternalContent]）：
 * - 同 [contentHash] 重放 → 忽略（幂等）；
 * - 可比较且旧于 committedVersion → 忽略；
 * - localDirty=true → 冲突，禁止直接 reset；
 * - 空 committed（从未建立版本事实）→ 可应用（首次加载）；
 * - 不可比较的不同版本 → 类型化冲突，禁止盲目覆盖；
 * - 其余（父链可达的后代版本）→ 可应用。
 */
@Immutable
data class DocumentVersion(
    val contentHash: String = "",
    val repositoryRevision: Long = 0L,
    val syncCommitId: String? = null,
    val parentVersion: DocumentVersion? = null,
) {
    /** 没有任何版本锚点 — 调用方不得把空版本当作可应用事件。 */
    val isEmpty: Boolean
        get() = contentHash.isEmpty() && repositoryRevision == 0L && syncCommitId == null
}

/**
 * #595 二/四 / #624 评论9：每个 target 的完整文档事实 — 事件总线保存的是文档事实而非
 * "最后一个事件对象"；新 collector 读到的就是当前文档状态，重放旧命令不会
 * 再次执行副作用（同 sourceVersion 幂等）。
 *
 * #624 评论9：删除 `text` 字段 — 正文只在冷路径（load/snapshot/save/sync/external-apply）
 * 经 [TargetSnapshot.text] 一次性 materialize，热路径不存整章 String。
 * [shouldApplyExternalContent] 需要比较正文时低频调用 [queryTargetSnapshot] 取 snapshot.text。
 *
 * 字段含义：
 * - [revision]：Rust session 的真实 revision（snapshot 镜像）；
 * - [selectionAnchorUtf8]/[selectionHeadUtf8]：真实选区（UTF-8 字节）；
 * - [committedVersion]：最后应用的文档版本（本地输入不改变它；外部版本应用或
 *   保存成功上报后更新）；
 * - [sessionBaseVersion]：Rust session 创建/重置/保存时基于的文档版本（外部事件
 *   据此判断是否基于旧 base — 旧 base 且本地 dirty 时必须走三方合并/冲突，
 *   禁止直接 reset；保存成功后与 committedVersion 一起推进）；
 * - [lastSavedVersion]：最近一次保存成功的版本（无保存记录为 null）；
 * - [localDirty]：存在尚未落盘的本地编辑（本地输入置 true；外部版本应用或
 *   保存成功上报后置 false）；
 * - [lastAppliedTransactionId]：最后应用的编辑事务 ID。
 */
@Immutable
data class DocumentState(
    val revision: Long = 0L,
    val selectionAnchorUtf8: Int = 0,
    val selectionHeadUtf8: Int = 0,
    val committedVersion: DocumentVersion = DocumentVersion(),
    val sessionBaseVersion: DocumentVersion = DocumentVersion(),
    val lastSavedVersion: DocumentVersion? = null,
    val localDirty: Boolean = false,
    val lastAppliedTransactionId: Long = 0L,
    /**
     * #624 评论17 问题3/5：未解决的外部文档事实 — IgnoreDirtyConflict /
     * IgnoreUncomparableConflict 时保存，避免被 hash 去重永久吞掉。
     *
     * #624 评论17 问题5：只存 [PendingExternalVersion]（sourceVersion + origin），
     * 不存 [TargetDocumentFact] 整个 — TargetDocumentFact 带 text: String，
     * 会把已经从 session state 删除的整章正文复制重新引回来。本地保存成功清 dirty
     * 后，调用方检查 pendingExternal 触发 Repository 重新读取最新正文/hash，再走
     * 正常 TargetDocumentFact → shouldApplyExternalContent() 决定 merge/apply
     * （不直接用缓存的旧正文覆盖刚保存的本地正文）。真正 Apply/IgnoreSameContent
     * 提交版本后才清。
     */
    val pendingExternal: PendingExternalVersion? = null,
)

/**
 * #624 评论17 问题5：未解决外部事实的轻量记录 — 只含 sourceVersion + origin，
 * 不含 text: String（不得把整章正文复制重新引回 [DocumentState]）。
 *
 * 本地保存清 dirty 后，调用方据 sourceVersion/origin 重新从 Repository 读最新
 * 正文/hash，构造完整 [TargetDocumentFact] 走 [shouldApplyExternalContent]，
 * 不使用缓存的旧正文。
 */
@Immutable
data class PendingExternalVersion(
    val sourceVersion: DocumentVersion,
    val origin: DocumentFactOrigin,
)

/** 纯数据选区快照（UTF-8 字节偏移）。 */
@Immutable
data class SelectionSnapshot(
    val anchorUtf8: Int = 0,
    val headUtf8: Int = 0,
)
