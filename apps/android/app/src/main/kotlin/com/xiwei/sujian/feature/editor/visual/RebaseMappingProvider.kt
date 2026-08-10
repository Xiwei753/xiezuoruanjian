package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.feature.editor.projection.OffsetMap

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
 * 返回值直接使用 Core 的 `RebaseSliceMappingDto`（平台端不再维护本地副本）；
 * 索引为完整 slice 列表的原始索引（与传入列表一一对应，Static 过滤后的
 * Core 索引已由平台翻译回完整列表索引）。
 */
fun interface RebaseMappingProvider {
    fun compute(
        oldSlices: List<SliceRoleAndByteRange>,
        newSlices: List<SliceRoleAndByteRange>,
        offsetMap: OffsetMap?,
    ): List<uniffi.writer_core.RebaseSliceMappingDto>
}
