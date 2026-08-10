package com.xiwei.sujian.feature.editor.visual

/**
 * #606: Rebase slice 继续/结束语义 — 与 Core `RebaseContinuation` 一一对应。
 *
 * Android 端定义（不依赖 UniFFI 绑定），与 Core `compute_rebase_slice_mappings`
 * 的输出语义完全一致。当 UniFFI 绑定最终重新生成时，可直接切换到 Core 的 DTO。
 */
enum class RebaseContinuation {
    Continue,
    End,
}

/**
 * #606: Rebase 匹配依据 — 与 Core `RebaseReason` 一一对应。
 *
 * 当前 Android 端只实现 `SameByteRange` 和 `NoMapping`（与 Core
 * `compute_rebase_slice_mappings` 当前行为一致）。
 */
enum class RebaseReason {
    SameByteRange,
    NoMapping,
}

/**
 * #606: 旧→新逻辑 slice 对应关系 — 与 Core `RebaseSliceMapping` 一一对应。
 *
 * `oldSliceIndex` / `newSliceIndex` 为对应事务 slice 列表中的索引。
 * 由 [RebasePlanner.computeRebaseSliceMappings] 计算，逻辑与 Core
 * `compute_rebase_slice_mappings` 完全一致。
 */
data class RebaseSliceMapping(
    val oldSliceIndex: Int,
    val newSliceIndex: Int,
    val continuation: RebaseContinuation,
    val reason: RebaseReason,
)

/**
 * #606: slice 角色 + byte range 包装，用于 [RebasePlanner.computeRebaseSliceMappings]
 * 的入参。
 */
data class SliceRoleAndByteRange(
    val role: SliceRole,
    val byteStart: Int,
    val byteEndExclusive: Int,
)
