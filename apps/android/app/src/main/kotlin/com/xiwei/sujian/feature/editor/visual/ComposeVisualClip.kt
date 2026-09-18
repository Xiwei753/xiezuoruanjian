package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect

/**
 * #708 评论 5727808906：抽取的共享 clip fraction 计算 —
 * timeline 的 [ComposeVisualTimeline.computeUnitClipFractions] 和
 * handoff rebase 后重建 [ComposeVisualScene.unitClipFractions] 共用同一份纯函数。
 *
 * **背景**：handoff 首帧 `scene.copy(units = rebasedUnits)` 不重建 `unitClipFractions`，
 * 旧 parent key 的 fraction 被保留，新 split 出来的 child key 查不到 fraction。
 * draw 层在 coordinated 模式下缺 key 会默认成 0（inserted 分支）或 1，
 * 导致 split 后三段文字共用父块空间进度，出现吞字/吐字错位。
 *
 * 本 object 把"根据 cursorRect + unit 的 layout/range/role 算单个 unit fraction"的逻辑抽成
 * 无状态纯函数 [fractionFor]，timeline 和 handoff 都调用它，保证两套路径算出一致结果。
 */
internal object ComposeVisualClip {
    /**
     * 计算单个 unit 的可见 clip fraction。
     *
     * 实现与旧 [ComposeVisualTimeline.computeUnitClipFractions] 循环体内逻辑一致：
     * - `!coordinatedSpatialClip && alpha <= 0` → return null（跳过，alpha 主导显隐）
     * - glyph bounds 取不到 → return null
     * - 零宽 glyph → return 1f（由 alpha 单独决定）
     * - RetainedMove / ReflowMove → return 1f（始终完整可见，不参与 spatial clip）
     * - Inserted / DeletedGhost → 跨行裁切 + 同行裁切，return fraction
     *
     * @param unit 要计算 fraction 的 unit（alpha/position 已插值到当前帧）。
     * @param cursorRect 当前光标 rect。
     * @param coordinatedSpatialClip coordinated + spatial clip 模式标记。
     *   true 时所有 unit 都算 fraction（不能用 alpha<=0 跳过）。
     *   false 时 alpha<=0 的 unit 返回 null（跳过，alpha 主导显隐）。
     * @return 可见 fraction（0..1），或 null 表示应跳过（不放入 map）。
     */
    @Suppress("CyclomaticComplexMethod")
    fun fractionFor(
        unit: VisualTextUnit,
        cursorRect: Rect,
        coordinatedSpatialClip: Boolean,
    ): Float? {
        val alphaNow = unit.alpha.from
        // #703 评论 5710419102 问题2：coordinated 模式下空间裁切是主导，
        // 不能用 alpha 决定是否计算 clipFraction。新插入 unit 首帧 alpha=0，
        // 如果跳过则 unitClipFractions 缺 key，draw 层默认成 1，整字首帧完整出现。
        // coordinated 模式：所有 scene unit 都计算 clipFraction。
        // 非 coordinated 模式：保留 alpha<=0 跳过（alpha 仍主导显隐）。
        if (!coordinatedSpatialClip && alphaNow <= 0f) return null
        // 取 glyph bounds（用 unit 当前 layout + range）
        val bounds = safePathBoundsForUnit(unit) ?: return null
        val glyphLeft = bounds.left
        val glyphRight = bounds.right
        val glyphTop = bounds.top
        val glyphBottom = bounds.bottom
        val glyphWidth = glyphRight - glyphLeft
        val cursorLeft = cursorRect.left
        val cursorTop = cursorRect.top
        val cursorBottom = cursorRect.bottom
        // 零宽 glyph（如空字符）或极窄 glyph：fraction = 1，由 alpha 单独决定
        if (glyphWidth < 0.5f) return 1f
        // #703 评论 5710977972 缺陷2：RetainedMove（幸存回流文字）始终完整可见，
        // 不进入 spatial clip 裁切。显式返回 fraction=1 最稳妥，
        // 避免 draw 层 coordinated 模式下缺失 key 默认成 0（inserted 分支）。
        // #708 评论 5723410606 第四节：ReflowMove 同样始终完整可见（alpha 永远 1），
        // 不参加 cursor spatial clip。
        if (unit.role == VisualUnitRole.RetainedMove ||
            unit.role == VisualUnitRole.ReflowMove
        ) {
            return 1f
        }
        // #703 评论 5710977972 缺陷1：跨行裁切改为按行序单调状态。
        // 用 layout 的 line index 判断方向（不拿 glyph bounds 的 top/bottom，
        // glyph 可能只占行一部分）：
        // - glyphLine = unit.layout.result.getLineForOffset(unit.range.start)
        // - glyphLineTop = getLineTop(glyphLine), glyphLineBottom = getLineBottom(glyphLine)
        // - cursorBeforeGlyphLine = cursorBottom <= glyphLineTop（光标在 glyph 行之前/上方）
        // - cursorAfterGlyphLine = cursorTop >= glyphLineBottom（光标在 glyph 行之后/下方）
        //
        // 按行序单调状态：
        // - Inserted（吐字，光标从左向右移动，字从左向右出现）：
        //   * cursorAfterGlyphLine（光标已过这一行）→ fraction = 1（字完整可见，已吐完）
        //   * 同一行 → ((cursorLeft - glyphLeft) / glyphWidth).coerceIn(0f, 1f)
        //   * cursorBeforeGlyphLine（光标还没到这一行）→ fraction = 0（字不可见）
        // - DeletedGhost（吞字，光标从右向左退，从下方往上退）：
        //   * cursorBeforeGlyphLine（光标在 ghost 行上方，已退过这一行）→ fraction = 0（字被吞掉）
        //   * 同一行 → ((cursorLeft - glyphLeft) / glyphWidth).coerceIn(0f, 1f)
        //   * cursorAfterGlyphLine（光标在 ghost 行下方，还没退到这一行）→ fraction = 1（字仍完整可见）
        val (glyphLineTop, glyphLineBottom) =
            try {
                val glyphLine = unit.layout.result.getLineForOffset(unit.range.start)
                unit.layout.result.getLineTop(glyphLine) to
                    unit.layout.result.getLineBottom(glyphLine)
            } catch (_: Throwable) {
                // layout 行信息取不到时 fallback 到旧 sameLine 语义（用 glyph bounds），
                // 保证不会因 layout API 异常而整字消失。
                glyphTop to glyphBottom
            }
        val cursorBeforeGlyphLine = cursorBottom <= glyphLineTop
        val cursorAfterGlyphLine = cursorTop >= glyphLineBottom
        return when (unit.role) {
            VisualUnitRole.Inserted -> {
                when {
                    cursorAfterGlyphLine -> 1f
                    cursorBeforeGlyphLine -> 0f
                    else -> ((cursorLeft - glyphLeft) / glyphWidth).coerceIn(0f, 1f)
                }
            }
            VisualUnitRole.DeletedGhost -> {
                when {
                    cursorBeforeGlyphLine -> 0f
                    cursorAfterGlyphLine -> 1f
                    else -> ((cursorLeft - glyphLeft) / glyphWidth).coerceIn(0f, 1f)
                }
            }
            VisualUnitRole.RetainedMove -> 1f
            VisualUnitRole.ReflowMove -> 1f
        }
    }

    /**
     * #708 评论 5727808906：安全取 unit 的 glyph bounds —
     * 用 unit 当前 layout + range 取 path bounds。
     *
     * 与旧 [ComposeVisualTimeline.safePathBoundsForUnit] 逻辑一致，
     * 抽到本共享 object 供 timeline 和 handoff 共用。
     * 底层调用 [ComposeVisualRebase.safePathBounds]（已是 internal static）。
     */
    private fun safePathBoundsForUnit(unit: VisualTextUnit): Rect? =
        ComposeVisualRebase.safePathBounds(unit.layout.result, unit.range)
}
