package com.xiwei.sujian.feature.editor.layout

import android.os.Build
import android.text.DynamicLayout
import android.text.Layout
import android.text.SpannableStringBuilder
import android.text.TextDirectionHeuristics
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.input.AndroidTextIndexMap
import com.xiwei.sujian.feature.editor.projection.DisplayPatch
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection

/**
 * 排版引擎 — 单一正文宽度 + DynamicLayout 复用（#624 评论4/7）。
 *
 * 布局配置（contentWidthPx / fontSizePx / lineSpacingMultiplier / firstLineIndentPx /
 * layoutDirection）收敛为一份 fingerprint：只有配置真正变化时才重建 DynamicLayout；
 * 普通 mirror 内容变化继续持有同一个 SpannableStringBuilder 和同一个
 * DynamicLayout（DynamicLayout 本身就是给可编辑 Spannable 增量变化使用的，
 * 文本/span 变化触发区域 reflow）。
 *
 * 首行缩进（#624 评论3）由 [ParagraphStyleProjection] 以显示层 span 施加在同一份
 * Spannable 上：配置变化/整篇重载时整篇重同步，正文编辑影响段落结构时只重同步
 * 受影响段落区域，普通按键不触碰任何 span。
 */
class AndroidLayoutEngine(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint,
) {
    private val snapshotBuilder = AndroidLineSnapshotBuilder()
    private val paragraphStyle = ParagraphStyleProjection()
    private var layout: DynamicLayout? = null
    private var currentRevision: AndroidLayoutRevision? = null
    private var width: Float = 0f
    private var lineSpacingMultiplier: Float = 1.0f
    private var firstLineIndentEnabled: Boolean = false
    private var firstLineIndentWidthChars: Float = 0f
    private val textDirection = TextDirectionHeuristics.FIRSTSTRONG_LTR
    private var revisionCounter: Long = 0
    private var lastConfigFingerprint: String = ""
    private var displayTextOverride: String? = null
    private var currentProjection: DisplayTextProjection? = null

    private fun currentFirstLineIndentPx(): Float =
        if (firstLineIndentEnabled && firstLineIndentWidthChars > 0f) {
            paragraphStyle.firstLineIndentPx(textPaint, firstLineIndentWidthChars)
        } else {
            0f
        }

    private fun computeConfigFingerprint(): String =
        "${width}_${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}_" +
            "${lineSpacingMultiplier}_${currentFirstLineIndentPx()}_${textDirection.hashCode()}"

    /** 唯一正文宽度来源 — 由宿主按真实可绘制宽度 `(width - paddingL - paddingR)` 传入。 */
    fun setWidth(width: Float) {
        if (this.width != width) {
            this.width = width
        }
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
    }

    /**
     * #624 评论3：首行缩进设置（开关 + 字符宽度）— 只影响显示层 span 与
     * fingerprint；变化后让布局配置失效，下一次 requestLayout 整篇重排。
     */
    fun setFirstLineIndent(
        enabled: Boolean,
        widthChars: Float,
    ) {
        if (firstLineIndentEnabled != enabled || firstLineIndentWidthChars != widthChars) {
            firstLineIndentEnabled = enabled
            firstLineIndentWidthChars = widthChars
            lastConfigFingerprint = ""
        }
    }

    fun requestLayout() {
        if (width <= 0f) return
        val effectiveText =
            displayTextOverride?.let { override ->
                SpannableStringBuilder(override)
            } ?: mirror.getSpannable()

        val currentConfigFp = computeConfigFingerprint()

        // 只有配置（宽度/字号/行距/首行缩进/方向）或显示 source 类型变化才重建
        // DynamicLayout；普通 mirror 内容变化在这里直接复用现有 layout。
        if (currentConfigFp != lastConfigFingerprint || layout == null) {
            if (displayTextOverride == null) {
                // 重建前整篇重同步段落缩进 span（attach/配置变化后 span 可能
                // 已随 buffer.clear() 消失或过期；启用时整篇应用，禁用时清空）。
                if (firstLineIndentEnabled && firstLineIndentWidthChars > 0f) {
                    paragraphStyle.applyFirstLineIndent(
                        effectiveText,
                        true,
                        firstLineIndentWidthChars,
                        textPaint,
                    )
                } else {
                    paragraphStyle.clearParagraphIndent(effectiveText)
                }
            }
            layout =
                if (Build.VERSION.SDK_INT >= 28) {
                    DynamicLayout.Builder.obtain(effectiveText, textPaint, width.toInt())
                        .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                        .setLineSpacing(0f, lineSpacingMultiplier)
                        .setIncludePad(false)
                        .setTextDirection(textDirection)
                        .build()
                } else {
                    @Suppress("DEPRECATION")
                    DynamicLayout(
                        effectiveText, textPaint, width.toInt(),
                        Layout.Alignment.ALIGN_NORMAL,
                        0f, lineSpacingMultiplier,
                        false,
                    )
                }
            lastConfigFingerprint = currentConfigFp
        }

        revisionCounter++
        currentRevision = buildRevision(layout!!)
    }

    /**
     * #624 评论7：普通 mirror 内容变化后的投影刷新 — 更新 identity 投影的
     * offset 映射，不触碰 displayTextOverride（display source 类型没变，
     * 不得清 fingerprint 导致 DynamicLayout 重建）；只有真正的 mirror ← override
     * 源切换（secret 关闭等）才让配置失效。
     *
     * #624 评论3：随后增量维护首行缩进 span — 只处理影响段落结构的编辑
     * （插入/删除包含 `\n`、替换选区、段落边界插入），普通按键跳过，
     * 避免每字符都产生 span 抖动。patch 的 byte offset 在其应用时刻的缓冲区
     * 坐标中，替换起点前的字节在新旧缓冲区一致，因此用当前（新）投影换算
     * UTF-16 偏移仍正确。
     */
    fun onMirrorContentChanged(
        projection: DisplayTextProjection,
        patches: List<DisplayPatch>,
    ) {
        if (displayTextOverride != null) {
            displayTextOverride = null
            lastConfigFingerprint = ""
        }
        currentProjection = projection
        if (displayTextOverride != null || !firstLineIndentEnabled || firstLineIndentWidthChars <= 0f) {
            return
        }
        if (patches.isEmpty()) return
        val text = mirror.getSpannable()
        if (text.isEmpty()) return

        var regionStart = Int.MAX_VALUE
        var regionEnd = -1
        for (patch in patches) {
            val startUtf16 =
                projection.realUtf8ToDisplayUtf16(patch.replaceByteStart).coerceIn(0, text.length)
            val endUtf16 = (startUtf16 + patch.insertedText.length).coerceAtMost(text.length)
            val structural =
                patch.insertedText.indexOf('\n') >= 0 ||
                    patch.replaceByteStart != patch.replaceByteEndExclusive ||
                    startUtf16 == 0 ||
                    text[startUtf16 - 1] == '\n'
            if (!structural) continue
            regionStart = minOf(regionStart, startUtf16)
            regionEnd = maxOf(regionEnd, endUtf16)
        }
        if (regionEnd < 0) return
        paragraphStyle.resyncParagraphIndent(
            text,
            regionStart,
            regionEnd,
            firstLineIndentEnabled,
            currentFirstLineIndentPx(),
        )
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
        val utf16 =
            currentProjection?.realUtf8ToDisplayUtf16(byteOffset)
                ?: AndroidTextIndexMap(mirror).utf8ToUtf16(byteOffset)
        val l = layout ?: return 0
        return l.getLineForOffset(utf16)
    }

    fun getCursorLine(): Int {
        return getLineForUtf8(mirror.getCursorUtf8())
    }

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float {
        val utf16 =
            currentProjection?.realUtf8ToDisplayUtf16(byteOffset)
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

            val endsWithHardBreak =
                lineEndUtf16 > 0 && lineEndUtf16 <= layoutText.length &&
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

            lineRanges.add(
                AndroidLayoutRevision.LineRange(
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
                    paragraphLocalLineIndex = currentParagraphLocalLineIndex,
                ),
            )
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
            emptyList(),
        )
    }

    /**
     * 切换显示文本 source（mirror ↔ override）— 真正的 source 类型切换才让
     * 配置失效（下一次 requestLayout 重建 DynamicLayout）；identity 投影的
     * 内容更新走 [onMirrorContentChanged]，不清 fingerprint。
     */
    fun applyDisplaySource(
        override: String?,
        projection: DisplayTextProjection? = null,
    ) {
        displayTextOverride = override
        currentProjection = projection
        lastConfigFingerprint = ""
    }

    fun release() {
        layout = null
        currentRevision = null
        displayTextOverride = null
    }
}
