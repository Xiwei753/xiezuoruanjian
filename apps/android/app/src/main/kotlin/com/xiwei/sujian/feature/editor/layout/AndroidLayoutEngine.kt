package com.xiwei.sujian.feature.editor.layout

import android.os.Build
import android.text.DynamicLayout
import android.text.Layout
import android.text.SpannableStringBuilder
import android.text.TextDirectionHeuristics
import android.text.TextPaint
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
 * 布局推进职责（#624 评论3）：普通编辑路径由动画引擎 [captureAffectedRevision]
 * 唯一推进 — 不再每次按键重建整章 lineRanges。布局对象配置与本次编辑需要的
 * 不可变视觉快照拆开：
 * - [ensureLayoutConfig]：宽度/字体/行距/首行缩进/方向或显示 source 类型变化时
 *   才重建 DynamicLayout；普通 mirror 内容变化直接复用现有 layout。
 * - [captureAffectedRevision]：普通输入只抓本次受影响段落/视觉行 + 光标/选区
 *   几何 + 稳定后缀锚点；不复制整章 `AndroidLayoutRevision.lineRanges`。
 * - [requestLayout]（全量）：只用于配置变化/加载等非按键路径。
 *
 * 首行缩进（#624 评论3）由 [ParagraphStyleProjection] 以显示层 span
 * （[FirstLineIndentSpan]，`SPAN_PARAGRAPH` + `UpdateLayout`）施加在同一份
 * Spannable 上：配置变化/整篇重载时整篇重同步，正文编辑影响段落结构时只重同步
 * 受影响段落区域，普通按键不触碰任何 span。
 */
class AndroidLayoutEngine(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint,
) {
    private val snapshotBuilder = AndroidLineSnapshotBuilder()
    private val paragraphStyle = ParagraphStyleProjection()
    private val lineCapture = AffectedLineCapture()
    private var layout: DynamicLayout? = null
    private var currentFullRevision: AndroidLayoutRevision? = null
    private var currentAffectedRevision: AffectedLayoutRevision? = null
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

    /** 测试辅助：当前首行缩进像素。 */
    internal fun getFirstLineIndentPxForTest(): Float = currentFirstLineIndentPx()

    internal fun computeConfigFingerprint(): String =
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
     * fingerprint；变化后让布局配置失效，下一次 [ensureLayoutConfig] 整篇重排。
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

    /**
     * #624 评论3：布局配置推进 — 只有配置（宽度/字号/行距/首行缩进/方向）或显示
     * source 类型变化才重建 DynamicLayout；普通 mirror 内容变化在这里直接复用
     * 现有 layout（DynamicLayout 随 Editable 文本/span 变化自动 reflow）。
     * 本方法**不**构建任何 revision — revision 由 [captureAffectedRevision] 或
     * 全量 [requestLayout] 单独产生。
     */
    fun ensureLayoutConfig() {
        if (width <= 0f) return
        val effectiveText =
            displayTextOverride?.let { override ->
                SpannableStringBuilder(override)
            } ?: mirror.getSpannable()

        val currentConfigFp = computeConfigFingerprint()

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
            // 布局对象重建后旧快照的几何全部过期 — 渲染路径退回全量 revision。
            currentAffectedRevision = null
        }
    }

    /**
     * 全量布局推进 — 只用于配置变化/加载/显式重建路径（不是每键热路径）。
     * 推进配置后构建整章 revision（供查询与渲染回退）。
     */
    fun requestLayout() {
        ensureLayoutConfig()
        val l = layout ?: return
        revisionCounter++
        currentFullRevision = buildRevision(l)
    }

    /**
     * #624 评论3：普通编辑路径的受影响区域捕获 — 编辑前（old 侧）用当前
     * DynamicLayout 的 `getLineForOffset()` 找到编辑所在段落，只保存该段落及
     * 删除/合段时的相邻段落的 line geometry；编辑后（new 侧）只读取新的受影响
     * 段落。不复制整章 `lineRanges`。
     *
     * [editStartUtf8]/[editEndUtf8] 为编辑区域（UTF-8 字节，半开区间）在本侧
     * 坐标中的位置（old 侧 = 旧正文坐标，new 侧 = 新正文坐标）。
     * [includeNextParagraph]：删除/替换角色（old 侧）或区域终点后紧跟段落边界
     * （new 侧）时，把区域后的相邻段落一并纳入受影响范围。
     * [previousRevision]：new 侧传入 old 侧快照，用于计算稳定后缀的 deltaY。
     */
    fun captureAffectedRevision(
        editStartUtf8: Int,
        editEndUtf8: Int,
        includeNextParagraph: Boolean,
        previousRevision: AffectedLayoutRevision? = null,
    ): AffectedLayoutRevision? {
        val l = layout ?: return null
        if (width <= 0f) return null
        val projection =
            currentProjection
                ?: DisplayTextProjection.identityFromIndex(mirror.getTextOffsetIndex(), mirror.getSpannable())
        // #624 评论3：热路径不整章复制 — layoutText 直接引用 mirror 的 Spannable
        // （与 DynamicLayout 同源，段落边界扫描/行内容判定一致），不再每次
        // getText() 拷贝整章 String。masked override 时是小型 masked 文本。
        val layoutText: CharSequence =
            displayTextOverride?.let { SpannableStringBuilder(it) } ?: mirror.getSpannable()

        val regionStartUtf16 = projection.realUtf8ToDisplayUtf16(editStartUtf8.coerceAtLeast(0))
        val regionEndUtf16 = projection.realUtf8ToDisplayUtf16(editEndUtf8.coerceAtLeast(0))
        val captured =
            lineCapture.capture(
                AffectedLineCapture.CaptureParams(
                    layout = l,
                    layoutText = layoutText,
                    projection = projection,
                    mirror = mirror,
                    firstLineIndentEnabled = firstLineIndentEnabled,
                    firstLineIndentWidthChars = firstLineIndentWidthChars,
                    firstLineIndentPx = currentFirstLineIndentPx(),
                ),
                regionStartUtf16 = regionStartUtf16,
                regionEndUtf16 = regionEndUtf16,
                includeNextParagraph = includeNextParagraph,
            ) ?: return null

        // ── 稳定后缀锚点：受影响区域之后第一个内容未变化的段落起点 ──
        // 字节长度 O(1) 读取（mirror 增量维护，不做整章 toByteArray — #624 评论3）。
        val textLengthUtf8 = mirror.getTextLengthUtf8()
        val stableAnchor =
            if (captured.affectedEndUtf16 < layoutText.length) {
                val anchorLine = l.getLineForOffset(captured.affectedEndUtf16)
                val anchorTop = l.getLineTop(anchorLine).toFloat()
                val oldAnchor = previousRevision?.stableSuffixAnchor
                val anchorUtf8 = projection.displayUtf16ToRealUtf8(captured.affectedEndUtf16)
                // 两侧锚点指向同一段未变化内容（净长度变化一致）时才可信。
                val suffixCorresponds =
                    oldAnchor != null && anchorUtf8 - oldAnchor.startUtf8 == textLengthUtf8 - oldAnchor.textLengthUtf8
                AffectedLayoutRevision.StableSuffixAnchor(
                    startUtf8 = anchorUtf8,
                    startUtf16 = captured.affectedEndUtf16,
                    lineIndex = anchorLine,
                    top = anchorTop,
                    bottom = l.getLineBottom(anchorLine).toFloat(),
                    left = l.getLineLeft(anchorLine),
                    right = l.getLineRight(anchorLine),
                    textLengthUtf8 = textLengthUtf8,
                    deltaY = if (previousRevision == null || !suffixCorresponds) 0f else anchorTop - oldAnchor.top,
                )
            } else {
                null
            }

        revisionCounter++
        val geometry = captured.cursorGeometry
        val revision =
            AffectedLayoutRevision(
                editorRevision = mirror.getRevision(),
                layoutRevision = revisionCounter,
                layoutConfigFingerprint = computeConfigFingerprint(),
                firstAffectedLineIndex = captured.firstAffectedLineIndex,
                affectedLines = captured.affectedLines,
                lineCount = l.lineCount,
                cursorUtf8 = geometry.cursorUtf8,
                cursorUtf16 = geometry.cursorUtf16,
                cursorX = geometry.cursorX,
                cursorY = geometry.cursorY,
                cursorHeight = geometry.cursorHeight,
                selectionAnchorUtf8 = geometry.selectionAnchorUtf8,
                selectionHeadUtf8 = geometry.selectionHeadUtf8,
                selectionAnchorUtf16 = geometry.selectionAnchorUtf16,
                selectionHeadUtf16 = geometry.selectionHeadUtf16,
                compositionStartUtf16 = geometry.compositionStartUtf16,
                compositionEndUtf16 = geometry.compositionEndUtf16,
                stableSuffixAnchor = stableAnchor,
            )
        currentAffectedRevision = revision
        return revision
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
     *
     * 本方法**不**推进布局（不构建 revision、不触发重建）— 布局推进所有权在
     * 动画引擎 [captureAffectedRevision] / 全量 [requestLayout]，避免一次编辑
     * 出现两次 revision 推进（#624 评论3）。
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
        if (text.isEmpty()) {
            // #637 评论 5386066978 项1：正文从 1 个字符删到 0 时，原
            // FirstLineIndentSpan 会随 Editable 收缩成 0..0。这里必须调用
            // resyncParagraphIndent 清掉塌缩 span（它在空文本时先
            // removeAllParagraphIndentSpans 再 return），否则下一帧
            // AffectedLineCapture 会从残留 span 得到一次缩进、再手工补一次，
            // 形成两倍缩进。
            paragraphStyle.resyncParagraphIndent(
                text,
                0,
                0,
                firstLineIndentEnabled,
                currentFirstLineIndentPx(),
            )
            return
        }

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

    /** 最近一次捕获的受影响 revision（编辑路径）— 渲染/查询优先使用。 */
    fun getCurrentAffectedRevision(): AffectedLayoutRevision? = currentAffectedRevision

    /**
     * 渲染路径的当前 revision：优先返回与当前正文 + 配置一致的受影响 revision；
     * 否则回退到全量 revision（配置变化/加载路径）。
     */
    fun getCurrentRevision(): LayoutRevisionSource? {
        val affected = currentAffectedRevision ?: return currentFullRevision
        if (affected.editorRevision != mirror.getRevision()) return currentFullRevision
        return if (affected.layoutConfigFingerprint == computeConfigFingerprint()) affected else currentFullRevision
    }

    /**
     * 捕获受影响行的 line Bitmap 快照（含 cluster 数据）。
     * 只捕获 [revision] 覆盖的受影响行 — 不触碰整章。
     */
    fun captureLineBitmapSnapshotsWithClusters(revision: AffectedLayoutRevision): Map<Int, AndroidLineSnapshot> {
        val l = layout ?: return emptyMap()
        val result = mutableMapOf<Int, AndroidLineSnapshot>()
        for (i in 0 until revision.affectedLineCount) {
            val lineIndex = revision.firstAffectedLineIndex + i
            val snapshot =
                snapshotBuilder.buildSnapshotForLineWithClusters(l, lineIndex, revision, mirror, currentProjection)
            if (snapshot != null) {
                result[lineIndex] = snapshot
            }
        }
        return result
    }

    /** 全量 revision 的行快照入口（当前仅测试/工具场景使用）。 */
    fun getLayout(): Layout? = layout

    fun getWidth(): Float = width

    fun getMirror(): DisplayTextMirror = mirror

    private fun buildRevision(l: Layout): AndroidLayoutRevision {
        val projection =
            currentProjection
                ?: DisplayTextProjection.identityFromIndex(mirror.getTextOffsetIndex(), mirror.getSpannable())
        val layoutText: CharSequence =
            displayTextOverride?.let { SpannableStringBuilder(it) } ?: mirror.getSpannable()
        // 全量捕获：区域覆盖整个文档，行捕获器按段落推进生成全部 lineRanges。
        val captured =
            lineCapture.capture(
                AffectedLineCapture.CaptureParams(
                    layout = l,
                    layoutText = layoutText,
                    projection = projection,
                    mirror = mirror,
                    firstLineIndentEnabled = firstLineIndentEnabled,
                    firstLineIndentWidthChars = firstLineIndentWidthChars,
                    firstLineIndentPx = currentFirstLineIndentPx(),
                ),
                regionStartUtf16 = 0,
                regionEndUtf16 = layoutText.length,
                includeNextParagraph = false,
            )
        val lineRanges = captured?.affectedLines ?: emptyList()
        val geometry = captured?.cursorGeometry

        val fontFingerprint = "${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}"

        return AndroidLayoutRevision(
            revisionCounter,
            mirror.getRevision(),
            width,
            fontFingerprint,
            l.lineCount,
            lineRanges,
            geometry?.cursorUtf8 ?: 0,
            geometry?.cursorUtf16 ?: 0,
            geometry?.cursorX ?: 0f,
            geometry?.cursorY ?: 0f,
            geometry?.cursorHeight ?: 0f,
            geometry?.selectionAnchorUtf8 ?: 0,
            geometry?.selectionHeadUtf8 ?: 0,
            geometry?.selectionAnchorUtf16 ?: 0,
            geometry?.selectionHeadUtf16 ?: 0,
            geometry?.compositionStartUtf16 ?: -1,
            geometry?.compositionEndUtf16 ?: -1,
            emptyList(),
        )
    }

    /**
     * 切换显示文本 source（mirror ↔ override）— 真正的 source 类型切换才让
     * 配置失效（下一次 [ensureLayoutConfig] 重建 DynamicLayout）；identity 投影的
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
        currentFullRevision = null
        currentAffectedRevision = null
        displayTextOverride = null
    }
}
