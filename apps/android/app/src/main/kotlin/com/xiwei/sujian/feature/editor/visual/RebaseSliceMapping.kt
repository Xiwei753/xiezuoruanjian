package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.feature.editor.projection.OffsetMap

/**
 * #606: Rebase slice 继续/结束语义 — 与 Core `RebaseContinuation` 一一对应。
 *
 * Android 端定义（纯数据，无逻辑）。Core 只返回 `Continue` 映射；
 * 未出现在映射中的旧 slice 由平台端按 `End` 处理。
 */
enum class RebaseContinuation {
    Continue,
    End,
}

/**
 * #606: Rebase 匹配依据 — 与 Core `RebaseReason` 一一对应。
 *
 * 匹配依据由 Core 唯一计算：byte range 完全相同（SameByteRange）或
 * OffsetMap 映射后 range 相等（OffsetMapMatched）；无对应关系（NoMapping）。
 */
enum class RebaseReason {
    SameByteRange,
    OffsetMapMatched,
    NoMapping,
}

/**
 * #606: 旧→新逻辑 slice 对应关系 — 与 Core `RebaseSliceMapping` 一一对应。
 *
 * `oldSliceIndex` / `newSliceIndex` 为对应事务 slice 列表中的索引。
 * 由 Core 的 `compute_rebase_slice_mappings` 唯一计算，Android 只消费，
 * 不再自己匹配（`RebasePlanner` 不再包含任何本地匹配逻辑）。
 */
data class RebaseSliceMapping(
    val oldSliceIndex: Int,
    val newSliceIndex: Int,
    val continuation: RebaseContinuation,
    val reason: RebaseReason,
)

/**
 * #606: slice 角色 + byte range 包装，作为 Core 匹配函数的入参。
 *
 * [role] 只取 Core 分类的动画角色（Insert/Delete/Move/CrossfadeOld/CrossfadeNew）；
 * [byteStart]/[byteEndExclusive] 为对应文档坐标下的 UTF-8 byte range。
 */
data class SliceRoleAndByteRange(
    val role: SliceRole,
    val byteStart: Int,
    val byteEndExclusive: Int,
)

/**
 * #606: 从 Core 获取旧事务逻辑 slice → 新事务逻辑 slice 对应关系的提供者。
 *
 * 生产路径由 [com.xiwei.sujian.feature.editor.pipeline.AndroidEditorPipeline] 注入，
 * 经 EditorKernelBridge 调用 Core 的 `compute_rebase_slice_mappings`；[offsetMap]
 * 为本次事务的旧正文 → 新正文偏移映射（`VisualIntent.offsetMap`，可能为 null）。
 * 返回值必须使用完整 slice 列表的原始索引（与传入列表一一对应）。
 */
fun interface RebaseMappingProvider {
    fun compute(
        oldSlices: List<SliceRoleAndByteRange>,
        newSlices: List<SliceRoleAndByteRange>,
        offsetMap: OffsetMap?,
    ): List<RebaseSliceMapping>
}
