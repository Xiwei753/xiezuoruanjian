package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec

/**
 * An immutable binding between a [LineClusterSnapshot] and the [TextRevealSpec] the
 * planner built for it.
 *
 * #605 评论4 问题1: previously [CaretRevealPlanner.planSwallowSpecs] returned a bare
 * `List<TextRevealSpec>` whose element order followed an internal descending sort,
 * while callers in [InsertDeletePlanner] still indexed the original unsorted cluster
 * list with the same index. Multi-cluster deletes could pair cluster A's Bitmap with
 * cluster C's caret geometry. Binding cluster+spec in one object removes the index
 * coupling entirely — callers iterate plans and read `plan.cluster` / `plan.spec`
 * from the same object.
 */
data class CaretRevealPlan(
    val cluster: LineClusterSnapshot,
    val spec: TextRevealSpec,
)

/**
 * Build [TextRevealSpec]s for Insert/Delete slices from cluster caret geometry.
 *
 * #605: Insert becomes caret-bounded REVEAL (clip from anchor toward boundary),
 * Delete becomes caret-bounded SWALLOW (clip from boundary toward anchor).
 *
 * Progress windows: each cluster gets a contiguous sub-window of [0,1] proportional
 * to its caret advance (|caretEndX - caretStartX|). This makes the reveal boundary
 * move continuously along text order for multi-cluster inserts, and contract toward
 * the caret for multi-cluster deletes.
 *
 * Weighting uses caret advance (not byte length) so wide glyphs (e.g. CJK) get a
 * proportionally larger time window, keeping the visual reveal speed approximately
 * constant in pixels per millisecond across scripts.
 *
 * #605 评论4 问题3: hard line breaks ("\n", "\r", "\r\n") have no visible glyph but
 * would still consume progress via coerceAtLeast(1f). They are filtered out before
 * planning so the visible clusters share the full [0,1] window.
 */
class CaretRevealPlanner {
    /**
     * Build reveal plans for inserted clusters (REVEAL mode).
     * Clusters are ordered by logical byte position; progress windows advance
     * along text order so the reveal boundary moves continuously.
     *
     * Hard-break clusters are excluded — they have no glyph to reveal.
     */
    fun planRevealSpecs(clusters: List<LineClusterSnapshot>): List<CaretRevealPlan> {
        // #605 评论5 问题2: Insert 按 documentByteStart 升序由 Planner 自己保证，
        // 不依赖调用方/affected-line/snapshot 收集顺序。跨行粘贴、跨段 replace/IME
        // 时上游集合顺序不保证 byte 升序，必须在此显式排序。
        // Delete 已在 planSwallowSpecs 用 sortedByDescending，Insert 对称用 sortedBy。
        val visible =
            clusters
                .asSequence()
                .filter { !it.isHardBreak }
                .sortedBy { it.documentByteStart }
                .toList()
        if (visible.isEmpty()) return emptyList()
        return buildSpecs(visible, TextRevealMode.REVEAL)
    }

    /**
     * Build swallow plans for deleted clusters (SWALLOW mode).
     *
     * #605: Clusters are ordered by distance from the final caret (far to near),
     * so deleting a range visually contracts toward the caret position.
     *
     * Sort by [LineClusterSnapshot.documentByteStart] descending: clusters with
     * larger byte positions are farther from the final caret (which sits at the
     * deletion start), so they swallow first. In LTR, larger byte = farther right
     * = farther from caret at left. In RTL, larger byte = farther left = farther
     * from caret at right. Both produce the correct "contract toward caret" visual.
     *
     * This is NOT sorted by cluster advance width — a wide cluster near the caret
     * must swallow last, not first, otherwise the text expands before contracting.
     *
     * Hard-break clusters are excluded — they have no glyph to swallow.
     */
    fun planSwallowSpecs(clusters: List<LineClusterSnapshot>): List<CaretRevealPlan> {
        val visible = clusters.filter { !it.isHardBreak }
        if (visible.isEmpty()) return emptyList()
        val sorted = visible.sortedByDescending { it.documentByteStart }
        return buildSpecs(sorted, TextRevealMode.SWALLOW)
    }

    private fun buildSpecs(
        clusters: List<LineClusterSnapshot>,
        mode: TextRevealMode,
    ): List<CaretRevealPlan> {
        val totalWeight =
            clusters.sumOf { cluster ->
                kotlin.math.abs(cluster.caretEndX - cluster.caretStartX).coerceAtLeast(1f).toDouble()
            }.toFloat().coerceAtLeast(0.0001f)

        val plans = mutableListOf<CaretRevealPlan>()
        var accumulatedWeight = 0f
        for (cluster in clusters) {
            val weight = kotlin.math.abs(cluster.caretEndX - cluster.caretStartX).coerceAtLeast(1f)
            val progressStart = accumulatedWeight / totalWeight
            accumulatedWeight += weight
            val progressEnd = accumulatedWeight / totalWeight

            val spec =
                if (mode == TextRevealMode.REVEAL) {
                    TextRevealSpec(
                        mode = TextRevealMode.REVEAL,
                        anchorX = cluster.caretStartX,
                        boundaryFromX = cluster.caretStartX,
                        boundaryToX = cluster.caretEndX,
                        progressStart = progressStart,
                        progressEnd = progressEnd,
                    )
                } else {
                    TextRevealSpec(
                        mode = TextRevealMode.SWALLOW,
                        anchorX = cluster.caretStartX,
                        boundaryFromX = cluster.caretEndX,
                        boundaryToX = cluster.caretStartX,
                        progressStart = progressStart,
                        progressEnd = progressEnd,
                    )
                }
            plans.add(CaretRevealPlan(cluster, spec))
        }
        return plans
    }
}
