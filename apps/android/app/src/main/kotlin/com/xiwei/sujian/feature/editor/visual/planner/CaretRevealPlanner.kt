package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec

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
 */
class CaretRevealPlanner {
    /**
     * Build reveal specs for inserted clusters (REVEAL mode).
     * Clusters are ordered by logical byte position; progress windows advance
     * along text order so the reveal boundary moves continuously.
     */
    fun planRevealSpecs(clusters: List<LineClusterSnapshot>): List<TextRevealSpec> {
        if (clusters.isEmpty()) return emptyList()
        return buildSpecs(clusters, TextRevealMode.REVEAL)
    }

    /**
     * Build swallow specs for deleted clusters (SWALLOW mode).
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
     */
    fun planSwallowSpecs(clusters: List<LineClusterSnapshot>): List<TextRevealSpec> {
        if (clusters.isEmpty()) return emptyList()
        val sorted = clusters.sortedByDescending { it.documentByteStart }
        return buildSpecs(sorted, TextRevealMode.SWALLOW)
    }

    private fun buildSpecs(
        clusters: List<LineClusterSnapshot>,
        mode: TextRevealMode,
    ): List<TextRevealSpec> {
        val totalWeight =
            clusters.sumOf { cluster ->
                kotlin.math.abs(cluster.caretEndX - cluster.caretStartX).coerceAtLeast(1f).toDouble()
            }.toFloat().coerceAtLeast(0.0001f)

        val specs = mutableListOf<TextRevealSpec>()
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
            specs.add(spec)
        }
        return specs
    }
}
