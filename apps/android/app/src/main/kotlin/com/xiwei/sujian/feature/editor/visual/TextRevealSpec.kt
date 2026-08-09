package com.xiwei.sujian.feature.editor.visual

enum class TextRevealMode {
    REVEAL,
    SWALLOW,
}

/**
 * Reveal/swallow animation spec for Insert/Delete slices.
 *
 * #605: replaces alpha-based fade with caret-bounded clip drawing.
 *
 * - REVEAL: the glyph is revealed from [anchorX] toward the interpolated boundary
 *   between [boundaryFromX] and [boundaryToX]. fraction=0 -> invisible, fraction=1 -> full.
 * - SWALLOW: the glyph contracts from the interpolated boundary toward [anchorX].
 *   fraction=0 -> full, fraction=1 -> invisible.
 *
 * [progressStart]/[progressEnd] define this slice's window within the global
 * transaction progress [0,1]. Multi-cluster reveals use contiguous windows so the
 * boundary moves continuously along text order.
 *
 * [initialFraction] supports rebase continuity: when a new transaction rebases an
 * in-flight reveal, the next spec starts from the on-screen fraction rather than 0,
 * so the animation continues smoothly instead of restarting.
 */
data class TextRevealSpec(
    val mode: TextRevealMode,
    val anchorX: Float,
    val boundaryFromX: Float,
    val boundaryToX: Float,
    val progressStart: Float,
    val progressEnd: Float,
    val initialFraction: Float = 0f,
) {
    /**
     * Map global transaction progress to this slice's local reveal fraction.
     *
     * Returns [initialFraction] at [progressStart] and 1f at [progressEnd].
     * Clamped to [0,1] and linearly interpolated in between. The span is
     * coerced to a minimum to avoid division by zero when progressStart == progressEnd
     * (e.g. single-cluster transactions where both are 0/1).
     */
    fun fraction(globalProgress: Float): Float {
        val span = (progressEnd - progressStart).coerceAtLeast(0.0001f)
        val local = ((globalProgress - progressStart) / span).coerceIn(0f, 1f)
        return initialFraction + (1f - initialFraction) * local
    }
}
