package com.xiwei.sujian.editor.v2.layout

import android.text.DynamicLayout
import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap

class AndroidLayoutEngine(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint
) {
    private val snapshotBuilder = AndroidLineSnapshotBuilder()
    private var layout: DynamicLayout? = null
    private var currentRevision: AndroidLayoutRevision? = null
    private var width: Float = 0f
    private var lineSpacingMultiplier: Float = 1.0f
    private var revisionCounter: Long = 0
    private var lastConfigFingerprint: String = ""
    private var displayTextOverride: String? = null

    private fun computeConfigFingerprint(): String {
        return "${width}_${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}_${lineSpacingMultiplier}"
    }

    fun setWidth(width: Float) {
        if (this.width != width) {
            this.width = width
        }
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
    }

    fun requestLayout() {
        val effectiveText = displayTextOverride?.let { override ->
            android.text.SpannableStringBuilder(override)
        } ?: mirror.getSpannable()
        if (width <= 0f) return

        val currentConfigFp = computeConfigFingerprint()

        if (currentConfigFp != lastConfigFingerprint || layout == null) {
            layout = DynamicLayout.Builder.obtain(effectiveText, textPaint, width.toInt())
                .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                .setLineSpacing(0f, lineSpacingMultiplier)
                .setIncludePad(false)
                .build()
            lastConfigFingerprint = currentConfigFp
        }

        revisionCounter++
        currentRevision = buildRevision(layout!!)
    }

    fun captureImmutableRevision(): AndroidLayoutRevision? {
        return currentRevision?.copy()
    }

    /** Capture line snapshots with per-cluster data. Both this method and
     *  [captureLineBitmapSnapshotsWithClusters] currently delegate to
     *  [AndroidLineSnapshotBuilder.buildSnapshotForLineWithClusters] — cluster data is
     *  always included so that callers can switch animation modes without re-capturing.
     *
     *  Design intent: always capturing cluster data is a deliberate trade-off. The alternative
     *  (cluster-less snapshots for SnapshotAnimation mode) would require re-capturing if the
     *  animation mode changes or if [addMoveSlicesForShiftedClustersCrossLine] needs cluster
     *  data for cross-line Move generation. Since cluster data adds negligible overhead (the
     *  Bitmap is the expensive part, cluster rects are computed from Layout API calls), always
     *  including it avoids a capture-mode mismatch that would silently produce whole-line
     *  crossfade instead of cluster-level animation. */
    fun captureLineBitmapSnapshots(lineIndices: Set<Int>): Map<Int, AndroidLineSnapshot> {
        val l = layout ?: return emptyMap()
        val rev = currentRevision ?: return emptyMap()
        val result = mutableMapOf<Int, AndroidLineSnapshot>()
        for (idx in lineIndices) {
            val snapshot = snapshotBuilder.buildSnapshotForLineWithClusters(l, idx, rev, mirror)
            if (snapshot != null) {
                result[idx] = snapshot
            }
        }
        return result
    }

    /** Capture line snapshots WITH per-cluster data (source rects, shaping fingerprints).
     *  This is the primary path used by [AndroidTextAnimationEngine.prepareAndSubmit] —
     *  cluster data is required for Insert/Delete/Move/Crossfade slice generation. */
    fun captureLineBitmapSnapshotsWithClusters(lineIndices: Set<Int>): Map<Int, AndroidLineSnapshot> {
        val l = layout ?: return emptyMap()
        val rev = currentRevision ?: return emptyMap()
        val result = mutableMapOf<Int, AndroidLineSnapshot>()
        for (idx in lineIndices) {
            val snapshot = snapshotBuilder.buildSnapshotForLineWithClusters(l, idx, rev, mirror)
            if (snapshot != null) {
                result[idx] = snapshot
            }
        }
        return result
    }

    fun getLayout(): Layout? = layout

    fun getCurrentRevision(): AndroidLayoutRevision? = currentRevision

    fun getWidth(): Float = width

    fun getMirror(): DisplayTextMirror = mirror

    fun getLineForUtf8(byteOffset: Int): Int {
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16 = indexMap.utf8ToUtf16(byteOffset)
        val l = layout ?: return 0
        return l.getLineForOffset(utf16)
    }

    fun getCursorLine(): Int {
        return getLineForUtf8(mirror.getCursorUtf8())
    }

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float {
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16 = indexMap.utf8ToUtf16(byteOffset)
        val l = layout ?: return 0f
        return l.getPrimaryHorizontal(utf16)
    }

    private fun buildRevision(l: Layout): AndroidLayoutRevision {
        val indexMap = AndroidTextIndexMap(mirror)
        val layoutText = displayTextOverride ?: mirror.getText()
        val lineRanges = mutableListOf<AndroidLayoutRevision.LineRange>()
        var currentParagraphId = 0
        var currentParagraphLocalLineIndex = 0
        for (i in 0 until l.lineCount) {
            val lineStartUtf16 = l.getLineStart(i)
            val lineEndUtf16 = l.getLineEnd(i)
            val top = l.getLineTop(i).toFloat()
            val bottom = l.getLineBottom(i).toFloat()
            val baseline = l.getLineBaseline(i).toFloat()
            val left = l.getLineLeft(i)
            val right = l.getLineRight(i)

            val startUtf8 = indexMap.utf16ToUtf8(lineStartUtf16)
            val endUtf8 = indexMap.utf16ToUtf8(lineEndUtf16)

            // Source-text inspection: the visual line's last character is `\n`.
            // This is the only reliable way to detect paragraph boundaries because
            // Android Layout byte ranges are contiguous across `\n` — there is no
            // byte gap between adjacent visual lines to detect. A previous approach
            // compared curr.startUtf8 > prev.endUtf8, but this never fires because
            // Android Layout's lineEnd is the position after the last character,
            // which is also the start of the next line — the ranges are contiguous
            // even across hard breaks.
            //
            // getLineEnd() returns an *exclusive* boundary (one past the last character),
            // so the last character is at index lineEndUtf16 - 1. Checking text[lineEndUtf16]
            // would read the first character of the *next* line, producing a false positive
            // when the next line starts with `\n`. The -1 adjustment is essential and
            // must not be removed — it is the only correct way to inspect the line's own
            // last character.
            //
            // Invariant: this field is used by the animation planner to stop reflow scanning.
            // Text reflow (soft-wrap changes) cannot propagate across a hard paragraph break,
            // so the planner stops expanding the affected-line set at the first line where
            // endsWithHardBreak is true. Subsequent paragraphs are handled via BlockShift
            // (uniform Y translation) rather than per-line Bitmap snapshots.
            val endsWithHardBreak = lineEndUtf16 > 0 && lineEndUtf16 <= layoutText.length &&
                layoutText[lineEndUtf16 - 1] == '\n'

            if (i == 0) {
                currentParagraphId = 0
                currentParagraphLocalLineIndex = 0
            } else if (lineRanges.lastOrNull()?.endsWithHardBreak == true) {
                currentParagraphId++
                currentParagraphLocalLineIndex = 0
            } else {
                currentParagraphLocalLineIndex++
            }
            // NOTE: paragraphId is a sequential integer that changes when hard breaks are
            // inserted or deleted (all subsequent paragraphs get new IDs). It is NOT a stable
            // identity for cross-revision paragraph matching. The animation planner uses
            // offset-map-based alignment (buildOffsetMapper) to match old/new paragraphs by
            // their UTF-8 byte range, not by paragraphId. paragraphId is only used for
            // grouping lines within a single revision (e.g. collecting all lines of the
            // current edit paragraph for snapshot capture).

            lineRanges.add(AndroidLayoutRevision.LineRange(
                startUtf8 = startUtf8,
                endUtf8 = endUtf8,
                startUtf16 = lineStartUtf16,
                endUtf16 = lineEndUtf16,
                top = top,
                bottom = bottom,
                baseline = baseline,
                left = left,
                right = right,
                endsWithHardBreak = endsWithHardBreak,
                paragraphId = currentParagraphId,
                paragraphLocalLineIndex = currentParagraphLocalLineIndex
            ))
        }

        val fontFingerprint = "${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}"

        val cursorUtf16 = mirror.getCursorUtf16()
        val cursorLine = if (cursorUtf16 in 0..mirror.getLengthUtf16()) l.getLineForOffset(cursorUtf16) else 0
        val cursorX = if (cursorUtf16 in 0..mirror.getLengthUtf16()) l.getPrimaryHorizontal(cursorUtf16) else 0f
        val cursorY = l.getLineTop(cursorLine).toFloat()
        val cursorHeight = (l.getLineBottom(cursorLine) - l.getLineTop(cursorLine)).toFloat()

        val compRange = mirror.getCompositionRangeUtf16()

        return AndroidLayoutRevision(
            revisionCounter,
            mirror.getRevision(),
            width,
            fontFingerprint,
            l.lineCount,
            lineRanges.toList(),
            mirror.getCursorUtf8(),
            cursorUtf16,
            cursorX,
            cursorY,
            cursorHeight,
            mirror.getSelectionAnchorUtf8(),
            mirror.getSelectionHeadUtf8(),
            mirror.getSelectionAnchorUtf16(),
            mirror.getSelectionHeadUtf16(),
            compRange?.first ?: -1,
            compRange?.second ?: -1,
            emptyList()
        )
    }

    fun setDisplayTextOverride(override: String) {
        displayTextOverride = override
        lastConfigFingerprint = ""
    }

    fun clearDisplayTextOverride() {
        displayTextOverride = null
        lastConfigFingerprint = ""
    }

    fun release() {
        layout = null
        currentRevision = null
        displayTextOverride = null
    }
}
