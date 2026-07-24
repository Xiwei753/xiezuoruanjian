package com.xiwei.sujian.editor.v2.layout

import android.text.DynamicLayout
import android.text.Layout
import android.text.TextPaint
import android.os.Build
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.projection.DisplayTextProjection

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
    private var currentProjection: DisplayTextProjection? = null

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
            layout = if (Build.VERSION.SDK_INT >= 28) {
                DynamicLayout.Builder.obtain(effectiveText, textPaint, width.toInt())
                    .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                    .setLineSpacing(0f, lineSpacingMultiplier)
                    .setIncludePad(false)
                    .build()
            } else {
                @Suppress("DEPRECATION")
                DynamicLayout(
                    effectiveText, textPaint, width.toInt(),
                    Layout.Alignment.ALIGN_NORMAL,
                    0f, lineSpacingMultiplier,
                    false
                )
            }
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
            val snapshot = snapshotBuilder.buildSnapshotForLineWithClusters(l, idx, rev, mirror, currentProjection)
            if (snapshot != null) {
                result[idx] = snapshot
            }
        }
        return result
    }

    fun captureLineBitmapSnapshotsWithClusters(lineIndices: Set<Int>): Map<Int, AndroidLineSnapshot> {
        val l = layout ?: return emptyMap()
        val rev = currentRevision ?: return emptyMap()
        val result = mutableMapOf<Int, AndroidLineSnapshot>()
        for (idx in lineIndices) {
            val snapshot = snapshotBuilder.buildSnapshotForLineWithClusters(l, idx, rev, mirror, currentProjection)
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
        val utf16 = currentProjection?.realUtf8ToDisplayUtf16(byteOffset)
            ?: AndroidTextIndexMap(mirror).utf8ToUtf16(byteOffset)
        val l = layout ?: return 0
        return l.getLineForOffset(utf16)
    }

    fun getCursorLine(): Int {
        return getLineForUtf8(mirror.getCursorUtf8())
    }

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float {
        val utf16 = currentProjection?.realUtf8ToDisplayUtf16(byteOffset)
            ?: AndroidTextIndexMap(mirror).utf8ToUtf16(byteOffset)
        val l = layout ?: return 0f
        return l.getPrimaryHorizontal(utf16)
    }

    private fun buildRevision(l: Layout): AndroidLayoutRevision {
        val projection = currentProjection ?: DisplayTextProjection.identity(mirror.getText())
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

            val startUtf8 = projection.displayUtf16ToRealUtf8(lineStartUtf16)
            val endUtf8 = projection.displayUtf16ToRealUtf8(lineEndUtf16)

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

        val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
        val cursorLine = if (cursorDisplayUtf16 in 0..layoutText.length) l.getLineForOffset(cursorDisplayUtf16) else 0
        val cursorX = if (cursorDisplayUtf16 in 0..layoutText.length) l.getPrimaryHorizontal(cursorDisplayUtf16) else 0f
        val cursorY = l.getLineTop(cursorLine).toFloat()
        val cursorHeight = (l.getLineBottom(cursorLine) - l.getLineTop(cursorLine)).toFloat()

        val compRange = mirror.getCompositionRangeUtf16()
        val compStartDisplayUtf16: Int
        val compEndDisplayUtf16: Int
        if (compRange != null && compRange.first >= 0 && compRange.second >= 0) {
            compStartDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRange.first)
            compEndDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRange.second)
        } else {
            compStartDisplayUtf16 = compRange?.first ?: -1
            compEndDisplayUtf16 = compRange?.second ?: -1
        }

        val selectionAnchorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionAnchorUtf8())
        val selectionHeadDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionHeadUtf8())

        return AndroidLayoutRevision(
            revisionCounter,
            mirror.getRevision(),
            width,
            fontFingerprint,
            l.lineCount,
            lineRanges.toList(),
            mirror.getCursorUtf8(),
            cursorDisplayUtf16,
            cursorX,
            cursorY,
            cursorHeight,
            mirror.getSelectionAnchorUtf8(),
            mirror.getSelectionHeadUtf8(),
            selectionAnchorDisplayUtf16,
            selectionHeadDisplayUtf16,
            compStartDisplayUtf16,
            compEndDisplayUtf16,
            emptyList()
        )
    }

    fun setDisplayTextOverride(override: String, projection: DisplayTextProjection? = null) {
        displayTextOverride = override
        currentProjection = projection
        lastConfigFingerprint = ""
    }

    fun clearDisplayTextOverride(projection: DisplayTextProjection? = null) {
        displayTextOverride = null
        currentProjection = projection
        lastConfigFingerprint = ""
    }

    fun release() {
        layout = null
        currentRevision = null
        displayTextOverride = null
    }
}
