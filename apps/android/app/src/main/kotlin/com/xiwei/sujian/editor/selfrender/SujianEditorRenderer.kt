package com.xiwei.sujian.editor.selfrender

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.model.AnimationModeData

class SujianEditorRenderer(
    private val textPaint: TextPaint,
    private val density: Float
) {
    internal val selectionPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0
        style = Paint.Style.FILL
    }

    internal val cursorPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0
        style = Paint.Style.FILL
        strokeWidth = 1.5f * density
    }

    internal val composingUnderlinePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }

    internal val searchHighlightPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0
        style = Paint.Style.FILL
    }

    var searchHighlights: List<Pair<Int, Int>> = emptyList()
        private set

    private var borderColor: Int = 0
    private var helperTextColor: Int = 0
    private var selectedTextColor: Int = 0
    private var preeditTextColor: Int = 0

    fun setThemeColors(textColor: Int, cursorColor: Int, composingColor: Int, selectionColor: Int) {
        cursorPaint.color = cursorColor
        composingUnderlinePaint.color = composingColor
        selectionPaint.color = selectionColor
        searchHighlightPaint.color = selectionColor
    }

    fun setThemeColorsExtended(
        textColor: Int,
        cursorColor: Int,
        composingColor: Int,
        selectionColor: Int,
        selectedTextColor: Int,
        preeditTextColor: Int,
        borderColor: Int,
        helperTextColor: Int
    ) {
        cursorPaint.color = cursorColor
        composingUnderlinePaint.color = composingColor
        selectionPaint.color = selectionColor
        searchHighlightPaint.color = selectionColor
        this.selectedTextColor = selectedTextColor
        this.preeditTextColor = preeditTextColor
        this.borderColor = borderColor
        this.helperTextColor = helperTextColor
    }

    fun getBorderColor(): Int = borderColor
    fun getHelperTextColor(): Int = helperTextColor
    fun getSelectedTextColor(): Int = selectedTextColor
    fun getPreeditTextColor(): Int = preeditTextColor

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlights = highlights
    }

    fun clearSearchHighlights() {
        searchHighlights = emptyList()
    }

    // ── Platform Visual Transaction 状态 ──
    private val activeTransactions = mutableListOf<AndroidPlatformVisualTransaction>()
    private val pendingQueue = mutableListOf<AndroidPlatformVisualTransaction>()
    private var isScrolling = false
    private var animationTimeoutMs: Long = 520L

    // ── 光标状态 ──
    var cursorVisible: Boolean = true
    var cursorBlinkOn: Boolean = true

    var cursorVisualX: Float = 0f
    var cursorVisualTop: Float = 0f
    var cursorVisualBottom: Float = 0f
    var cursorTargetX: Float = 0f
    var cursorTargetTop: Float = 0f
    var cursorTargetBottom: Float = 0f
    var smoothCursorEnabled: Boolean = false
    var isCursorAnimating: Boolean = false

    internal var hasComposingCursor: Boolean = false
    internal var composingCursorX: Float = 0f
    internal var composingCursorTop: Float = 0f
    internal var composingCursorBottom: Float = 0f

    // ── 文字颜色（用于 snapshot 录制）──
    private var textColor: Int = 0

    fun setTextColor(color: Int) {
        textColor = color
    }

    fun getTextColor(): Int = textColor

    fun addTransaction(tx: AndroidPlatformVisualTransaction): Boolean {
        if (isScrolling) {
            pendingQueue.removeAll { it.key == tx.key }
            pendingQueue.add(tx)
            return true
        }
        val conflicting = findConflictingTransaction(
            tx.slices.minOfOrNull { it.documentByteStart } ?: 0,
            tx.slices.maxOfOrNull { it.documentByteEnd } ?: 0
        )
        if (conflicting != null) {
            val currentProgress = conflicting.progress
            val detachedOldRevision = conflicting.detachOldRevisionForRebase()
            val detachedNewRevision = conflicting.takeNewRevisionForRebase()
            if (currentProgress in 0.01f..0.99f) {
                for (slice in conflicting.slices) {
                    val frame = slice.computeFrame(currentProgress, 1f - (1f - currentProgress) * (1f - currentProgress))
                    val matchingNewSlice = tx.slices.find { ns ->
                        ns.documentByteStart == slice.documentByteStart && ns.documentByteEnd == slice.documentByteEnd
                    }
                    if (matchingNewSlice != null) {
                        matchingNewSlice.rebaseFrom(frame.destinationRect, frame.alpha)
                    }
                }
                if (conflicting.cursorTransition.isSnap.not() && tx.cursorTransition.isSnap.not()) {
                    val oldCursorFrame = conflicting.cursorTransition.let { ct ->
                        val fromRect = ct.oldRect ?: RectF()
                        val toRect = ct.newRect ?: RectF()
                        val easedP = 1f - (1f - currentProgress) * (1f - currentProgress)
                        RectF(
                            fromRect.left + (toRect.left - fromRect.left) * easedP,
                            fromRect.top + (toRect.top - fromRect.top) * easedP,
                            fromRect.right + (toRect.right - fromRect.right) * easedP,
                            fromRect.bottom + (toRect.bottom - fromRect.bottom) * easedP
                        )
                    }
                    val newOldRect = tx.cursorTransition.oldRect ?: oldCursorFrame
                    tx.cursorTransition = AndroidCursorTransition.tween(
                        newOldRect,
                        tx.cursorTransition.newRect ?: oldCursorFrame,
                        tx.durationMs
                    )
                }
            }
            if (detachedNewRevision != null && (tx.operationKind == AndroidVisualOperationKind.CompositionUpdate || tx.operationKind == AndroidVisualOperationKind.CompositionCommitOrCancel)) {
                val previousOldRevision = tx.ownedOldRevision
                detachedNewRevision.reassignToTransaction(tx.key)
                tx.ownedOldRevision = detachedNewRevision
                if (previousOldRevision != null) {
                    previousOldRevision.release(previousOldRevision.owner)
                }
                if (detachedOldRevision != null) {
                    detachedOldRevision.release(SnapshotOwner.OwnedByTransaction(conflicting.key))
                }
            } else {
                if (detachedOldRevision != null) {
                    detachedOldRevision.release(SnapshotOwner.OwnedByTransaction(conflicting.key))
                }
                if (detachedNewRevision != null) {
                    detachedNewRevision.release(SnapshotOwner.OwnedByTransaction(conflicting.key))
                }
            }
            conflicting.cancel("rebased")
            activeTransactions.removeAll { it.key == conflicting.key }
        }
        activeTransactions.removeAll { it.key == tx.key }
        activeTransactions.add(tx)
        tx.markPrepared()
        animationTimeoutMs = (tx.durationMs * 3 + 500L).coerceIn(520L, 5000L)
        return true
    }

    fun findConflictingTransaction(byteStart: Int, byteEnd: Int): AndroidPlatformVisualTransaction? {
        val tx = activeTransactions.find {
            it.state == AndroidVisualTransactionState.Rendering ||
            it.state == AndroidVisualTransactionState.Prepared ||
            it.state == AndroidVisualTransactionState.Paused
        } ?: return null

        return when {
            tx.operationKind == AndroidVisualOperationKind.Cursor -> tx
            tx.operationKind == AndroidVisualOperationKind.CompositionCommitOrCancel ||
            tx.operationKind == AndroidVisualOperationKind.CompositionUpdate -> tx
            else -> {
                val hasOverlap = tx.slices.any { slice ->
                    !(byteEnd <= slice.documentByteStart || byteStart >= slice.documentByteEnd)
                }
                if (hasOverlap) tx else null
            }
        }
    }

    fun tickAnimations() {
        val now = System.currentTimeMillis()
        val toComplete = mutableListOf<AndroidPlatformVisualTransaction>()
        val toTimeout = mutableListOf<AndroidPlatformVisualTransaction>()

        for (tx in activeTransactions) {
            if (tx.state == AndroidVisualTransactionState.Prepared) {
                tx.markRendering()
            }
            if (tx.state == AndroidVisualTransactionState.Rendering) {
                if (tx.isFinished) {
                    toComplete.add(tx)
                } else if (tx.timeline.isStarted && (now - tx.timeline.firstVisibleFrameTimeMs - tx.timeline.accumulatedPausedDurationMs) > animationTimeoutMs) {
                    toTimeout.add(tx)
                }
            }
        }

        for (tx in toComplete) {
            tx.complete()
        }
        for (tx in toTimeout) {
            tx.cancel("timeout")
        }
        activeTransactions.removeAll { it.state == AndroidVisualTransactionState.Completed || it.state == AndroidVisualTransactionState.Cancelled }
    }

    fun isScrolling(): Boolean = isScrolling

    fun setScrolling(scrolling: Boolean) {
        if (isScrolling == scrolling) return
        isScrolling = scrolling
        if (scrolling) {
            for (tx in activeTransactions) {
                if (tx.state == AndroidVisualTransactionState.Rendering) {
                    tx.pause()
                }
            }
        } else {
            for (tx in activeTransactions) {
                if (tx.state == AndroidVisualTransactionState.Paused) {
                    tx.resume()
                }
            }
            flushPendingQueue()
        }
    }

    private fun flushPendingQueue() {
        if (pendingQueue.isEmpty()) return
        val toAdd = pendingQueue.toList()
        pendingQueue.clear()
        for (tx in toAdd) {
            if (tx.state == AndroidVisualTransactionState.Cancelled) continue
            addTransaction(tx)
        }
    }

    fun pauseAll() {
        for (tx in activeTransactions) {
            if (tx.state == AndroidVisualTransactionState.Rendering) {
                tx.pause()
            }
        }
    }

    fun resumeAll() {
        for (tx in activeTransactions) {
            if (tx.state == AndroidVisualTransactionState.Paused) {
                tx.resume()
            }
        }
    }

    fun clearAnimations() {
        for (tx in activeTransactions) {
            tx.cancel("clear")
        }
        activeTransactions.clear()
        for (tx in pendingQueue) {
            tx.cancel("clear")
        }
        pendingQueue.clear()
    }

    fun hasActiveAnimations(): Boolean = activeTransactions.any {
        it.state == AndroidVisualTransactionState.Rendering ||
        it.state == AndroidVisualTransactionState.Prepared ||
        it.state == AndroidVisualTransactionState.Paused
    }

    fun getActiveTransactions(): List<AndroidPlatformVisualTransaction> =
        activeTransactions.toList()

    fun getAffectedLineIndices(layout: Layout): Set<Int> {
        val affectedLines = mutableSetOf<Int>()
        for (tx in activeTransactions) {
            if (tx.state != AndroidVisualTransactionState.Rendering &&
                tx.state != AndroidVisualTransactionState.Prepared &&
                tx.state != AndroidVisualTransactionState.Paused) continue

            for (patch in tx.staticLinePatches) {
                for (lineIdx in 0 until layout.lineCount) {
                    val lineTop = layout.getLineTop(lineIdx).toFloat()
                    val lineBottom = layout.getLineBottom(lineIdx).toFloat()
                    if (lineBottom > patch.destinationDocumentRect.top &&
                        lineTop < patch.destinationDocumentRect.bottom) {
                        affectedLines.add(lineIdx)
                    }
                }
            }
        }
        return affectedLines
    }

    /**
     * 主绘制入口。
     *
     * 绘制顺序（从底到顶）：
     * 1. 搜索高亮背景
     * 2. 选区背景
     * 3. 静态正文（含 static patch 裁剪）
     * 4. 预输入文字和下划线
     * 5. 动画切片 overlay
     * 6. 光标
     *
     * 滚动偏移应用点：`canvas.translate(-scrollX, -scrollY)` 在方法开头一次性应用，
     * 所有后续绘制使用文档坐标。动画切片的 destination rect 也是文档坐标，
     * 不需要额外减去滚动量。
     */
    fun draw(
        canvas: Canvas,
        layout: Layout,
        text: String,
        scrollX: Int,
        scrollY: Int,
        selection: SujianSelection,
        composingStart: Int,
        composingEnd: Int,
        composingText: String,
        composingCursor: Int,
        viewportWidth: Int,
        viewportHeight: Int
    ) {
        hasComposingCursor = false

        canvas.save()
        canvas.translate(-scrollX.toFloat(), -scrollY.toFloat())

        drawSearchHighlights(canvas, layout, text)
        drawSelection(canvas, layout, text, selection)
        drawStaticTextWithPatches(canvas, layout, text, scrollY, viewportHeight)

        val hasCompositionTransaction = activeTransactions.any {
            it.operationKind == AndroidVisualOperationKind.CompositionUpdate &&
            (it.state == AndroidVisualTransactionState.Rendering ||
             it.state == AndroidVisualTransactionState.Paused ||
             it.state == AndroidVisualTransactionState.Prepared)
        }

        if (!hasCompositionTransaction) {
            if (composingText.isNotEmpty() && composingStart >= 0) {
                drawComposingTextAndUnderline(canvas, layout, text, composingStart, composingText, composingCursor)
            } else if (composingStart >= 0 && composingEnd >= 0 && composingStart < composingEnd && composingStart < text.length) {
                drawComposingUnderline(canvas, layout, text, composingStart, composingEnd)
            }
        }

        drawAnimatedSlices(canvas)

        if (hasCompositionTransaction) {
            drawTransactionDecorationSlices(canvas, layout, text, composingStart, composingText, composingCursor)
        }

        if (cursorVisible && cursorBlinkOn && selection.isCollapsed) {
            if (hasComposingCursor) {
                canvas.drawRect(
                    composingCursorX - cursorPaint.strokeWidth / 2f,
                    composingCursorTop,
                    composingCursorX + cursorPaint.strokeWidth / 2f,
                    composingCursorBottom,
                    cursorPaint
                )
            } else {
                drawCursor(canvas, layout, text, selection.head)
            }
        }

        canvas.restore()
    }

    /**
     * 绘制静态正文层，含 static patch 裁剪。
     *
     * 裁剪依据是已经排版好的视觉矩形（来自 [AndroidStaticLinePatch]），
     * 而不是重新从 byte range 反推 x 坐标——这样才能保持 ligature、RTL、
     * emoji/ZWJ 和复杂 shaping 的一致性。
     */
    private fun drawStaticTextWithPatches(
        canvas: Canvas,
        layout: Layout,
        text: String,
        scrollY: Int,
        viewportHeight: Int
    ) {
        if (text.isEmpty()) return

        val affectedLines = getAffectedLineIndices(layout)
        val firstVisLine = layout.getLineForVertical(scrollY.coerceAtLeast(0))
        val lastVisLine = layout.getLineForVertical((scrollY + viewportHeight).coerceAtLeast(0))
            .coerceAtMost(layout.lineCount - 1)

        if (affectedLines.isEmpty()) {
            val visTop = layout.getLineTop(firstVisLine).toFloat()
            val visBottom = layout.getLineBottom(lastVisLine).toFloat()
            canvas.save()
            canvas.clipRect(0f, visTop, layout.width.toFloat(), visBottom)
            layout.draw(canvas)
            canvas.restore()
            return
        }

        val nonAffectedLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in firstVisLine..lastVisLine) {
            if (lineIdx !in affectedLines) {
                if (rangeStart < 0) rangeStart = lineIdx
            } else {
                if (rangeStart >= 0) {
                    nonAffectedLineRanges.add(Pair(rangeStart, lineIdx - 1))
                    rangeStart = -1
                }
            }
        }
        if (rangeStart >= 0) {
            nonAffectedLineRanges.add(Pair(rangeStart, lastVisLine))
        }

        for ((rangeFirst, rangeLast) in nonAffectedLineRanges) {
            val visTop = layout.getLineTop(rangeFirst).toFloat()
            val visBottom = layout.getLineBottom(rangeLast).toFloat()
            canvas.save()
            canvas.clipRect(0f, visTop, layout.width.toFloat(), visBottom)
            layout.draw(canvas)
            canvas.restore()
        }

        for (lineIdx in affectedLines) {
            if (lineIdx < firstVisLine || lineIdx > lastVisLine) continue
            drawPatchedLine(canvas, layout, text, lineIdx)
        }
    }

    private fun drawPatchedLine(
        canvas: Canvas,
        layout: Layout,
        text: String,
        lineIdx: Int
    ) {
        val patches = mutableListOf<AndroidStaticLinePatch>()
        for (tx in activeTransactions) {
            if (tx.state != AndroidVisualTransactionState.Rendering &&
                tx.state != AndroidVisualTransactionState.Prepared &&
                tx.state != AndroidVisualTransactionState.Paused) continue
            patches.addAll(tx.staticLinePatches)
        }

        val lineStart = layout.getLineStart(lineIdx)
        val lineEnd = layout.getLineEnd(lineIdx)
        val lineTop = layout.getLineTop(lineIdx)
        val lineBottom = layout.getLineBottom(lineIdx)

        val animatedByteRanges = mutableListOf<Pair<Int, Int>>()
        for (tx in activeTransactions) {
            if (tx.state != AndroidVisualTransactionState.Rendering &&
                tx.state != AndroidVisualTransactionState.Prepared &&
                tx.state != AndroidVisualTransactionState.Paused) continue
            for (slice in tx.slices) {
                animatedByteRanges.add(Pair(slice.documentByteStart, slice.documentByteEnd))
            }
        }

        val visibleSegments = mutableListOf<Pair<Int, Int>>()
        var pos = lineStart
        val excludeSegments = mutableListOf<Pair<Int, Int>>()

        for ((byteStart, byteEnd) in animatedByteRanges) {
            val utf16Start = SujianEditorBuffer.utf8ToUtf16(text, byteStart)
            val utf16End = SujianEditorBuffer.utf8ToUtf16(text, byteEnd)
            val segStart = maxOf(lineStart, utf16Start)
            val segEnd = minOf(lineEnd, utf16End)
            if (segStart < segEnd) {
                excludeSegments.add(Pair(segStart, segEnd))
            }
        }
        excludeSegments.sortBy { it.first }
        val merged = mutableListOf<Pair<Int, Int>>()
        for (seg in excludeSegments) {
            if (merged.isNotEmpty() && merged.last().second >= seg.first) {
                val last = merged.removeLast()
                merged.add(Pair(last.first, maxOf(last.second, seg.second)))
            } else {
                merged.add(seg)
            }
        }

        for ((exStart, exEnd) in merged) {
            if (pos < exStart) {
                visibleSegments.add(Pair(pos, exStart))
            }
            pos = maxOf(pos, exEnd)
        }
        if (pos < lineEnd) {
            visibleSegments.add(Pair(pos, lineEnd))
        }

        val hasPatchForLine = patches.any { patch ->
            val patchLineTop = patch.destinationDocumentRect.top
            val patchLineBottom = patch.destinationDocumentRect.bottom
            patchLineBottom > lineTop && patchLineTop < lineBottom
        }

        if (!hasPatchForLine) {
            for ((segStart, segEnd) in visibleSegments) {
                drawLineSegment(canvas, layout, text, lineIdx, segStart, segEnd)
            }
        }

        for (patch in patches) {
            val patchLineTop = patch.destinationDocumentRect.top
            val patchLineBottom = patch.destinationDocumentRect.bottom
            if (patchLineBottom <= lineTop || patchLineTop >= lineBottom) continue

            val newSnapshot = findNewSnapshotForPatch(patch)
            if (newSnapshot?.visualResource != null) {
                for (visRect in patch.visibleSourceRects) {
                    newSnapshot.visualResource!!.drawSlice(
                        canvas,
                        visRect,
                        RectF(
                            patch.destinationDocumentRect.left + visRect.left,
                            patch.destinationDocumentRect.top + visRect.top,
                            patch.destinationDocumentRect.left + visRect.right,
                            patch.destinationDocumentRect.top + visRect.bottom
                        ),
                        255,
                        1f
                    )
                }
            }
        }
    }

    private fun findNewSnapshotForPatch(patch: AndroidStaticLinePatch): AndroidLineSnapshot? {
        for (tx in activeTransactions) {
            val snapshot = tx.newLineSnapshots.find { it.id == patch.newSnapshotId }
            if (snapshot != null) return snapshot
        }
        return null
    }

    /**
     * 绘制动画切片 overlay。
     *
     * 遍历活跃事务的 slices，通过 [AndroidLineVisualResource.drawSlice] 绘制。
     * sourceRect 使用行视觉资源局部坐标，destinationRect 使用文档坐标，
     * alpha 和 scale 由 [AndroidAnimatedSlice.computeFrame] 插值计算。
     */
    private fun drawAnimatedSlices(canvas: Canvas) {
        for (tx in activeTransactions) {
            if (tx.state != AndroidVisualTransactionState.Rendering &&
                tx.state != AndroidVisualTransactionState.Paused) continue

            val progress = tx.progress
            if (progress >= 1f && tx.state == AndroidVisualTransactionState.Rendering) continue

            val easedProgress = 1f - (1f - progress) * (1f - progress)

            for (slice in tx.slices) {
                val frame = slice.computeFrame(progress, easedProgress)
                val snapshot = findSnapshotForSlice(slice, tx)
                if (snapshot?.visualResource != null) {
                    snapshot.visualResource!!.drawSlice(
                        canvas,
                        slice.sourceRect,
                        frame.destinationRect,
                        frame.alpha,
                        frame.scale
                    )
                }
            }
        }
    }

    private fun findSnapshotForSlice(
        slice: AndroidAnimatedSlice,
        tx: AndroidPlatformVisualTransaction
    ): AndroidLineSnapshot? {
        val snapshotId = slice.sourceSnapshotId ?: return null
        val oldMatch = tx.oldLineSnapshots.find { it.id == snapshotId }
        if (oldMatch != null) return oldMatch
        return tx.newLineSnapshots.find { it.id == snapshotId }
    }

    private fun drawLineSegment(
        canvas: Canvas,
        layout: Layout,
        text: String,
        lineIdx: Int,
        segStart: Int,
        segEnd: Int
    ) {
        if (segStart >= segEnd || segStart >= text.length) return
        val safeEnd = minOf(segEnd, text.length)

        val path = Path()
        layout.getSelectionPath(segStart, safeEnd, path)
        if (path.isEmpty) return

        canvas.save()
        canvas.clipPath(path)
        layout.draw(canvas)
        canvas.restore()
    }

    private fun drawSearchHighlights(canvas: Canvas, layout: Layout, text: String) {
        if (searchHighlights.isEmpty() || text.isEmpty()) return
        for ((start, end) in searchHighlights) {
            val clampedStart = start.coerceIn(0, text.length)
            val clampedEnd = end.coerceIn(0, text.length)
            if (clampedStart >= clampedEnd) continue

            val path = Path()
            layout.getSelectionPath(clampedStart, clampedEnd, path)
            if (path.isEmpty) continue

            canvas.drawPath(path, searchHighlightPaint)
        }
    }

    private fun drawSelection(canvas: Canvas, layout: Layout, text: String, selection: SujianSelection) {
        if (selection.isCollapsed) return
        val start = selection.start.coerceIn(0, text.length)
        val end = selection.end.coerceIn(0, text.length)
        if (start >= end) return

        val path = Path()
        layout.getSelectionPath(start, end, path)
        if (path.isEmpty) return

        canvas.drawPath(path, selectionPaint)
    }

    private fun drawCursor(canvas: Canvas, layout: Layout, text: String, offset: Int) {
        if (text.isEmpty() && offset == 0) {
            canvas.drawRect(0f, 0f, cursorPaint.strokeWidth, textPaint.textSize, cursorPaint)
            return
        }

        val drawX: Float
        val drawTop: Float
        val drawBottom: Float

        if (smoothCursorEnabled && isCursorAnimating) {
            drawX = cursorVisualX
            drawTop = cursorVisualTop
            drawBottom = cursorVisualBottom
        } else {
            val safeOffset = offset.coerceIn(0, text.length)
            val line = layout.getLineForOffset(safeOffset)
            drawX = layout.getPrimaryHorizontal(safeOffset)
            val baseline = layout.getLineBaseline(line).toFloat()
            val ascent = layout.getLineAscent(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()
            drawTop = baseline + ascent
            drawBottom = baseline + descent
        }

        canvas.drawRect(
            drawX - cursorPaint.strokeWidth / 2f,
            drawTop,
            drawX + cursorPaint.strokeWidth / 2f,
            drawBottom,
            cursorPaint
        )
    }

    private fun drawComposingUnderline(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingEnd: Int
    ) {
        val startLine = layout.getLineForOffset(composingStart)
        val endLine = layout.getLineForOffset(composingEnd)

        for (line in startLine..endLine) {
            val lineStart = layout.getLineStart(line)
            val lineEnd = layout.getLineEnd(line)
            val cStart = if (line == startLine) composingStart else lineStart
            val cEnd = if (line == endLine) composingEnd else lineEnd

            if (cStart >= cEnd) continue

            val linePath = Path()
            layout.getSelectionPath(cStart, cEnd, linePath)
            if (linePath.isEmpty) continue

            val baseline = layout.getLineBaseline(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()
            val underlineY = baseline + descent + 2f

            canvas.save()
            canvas.clipPath(linePath)
            canvas.drawLine(0f, underlineY, layout.width.toFloat(), underlineY, composingUnderlinePaint)
            canvas.restore()
        }
    }

    private fun drawComposingTextAndUnderline(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingText: String,
        composingCursor: Int
    ) {
        if (composingText.isEmpty()) return

        val safeOffset = composingStart.coerceIn(0, text.length)
        val startX: Float
        val startBaselineY: Float

        if (text.isEmpty()) {
            startX = 0f
            startBaselineY = textPaint.textSize
        } else {
            val startLine = layout.getLineForOffset(safeOffset)
            startX = layout.getPrimaryHorizontal(safeOffset)
            startBaselineY = layout.getLineBaseline(startLine).toFloat()
        }

        val layoutWidth = layout.width.coerceAtLeast(1)
        val composingLayout: StaticLayout

        if (startX > 0f) {
            val indentPx = Math.round(startX)
            val spannedString = android.text.SpannableString(composingText)
            val marginSpan = android.text.style.LeadingMarginSpan.Standard(indentPx, 0)
            spannedString.setSpan(marginSpan, 0, composingText.length, android.text.Spannable.SPAN_INCLUSIVE_INCLUSIVE)

            composingLayout = StaticLayout.Builder.obtain(
                spannedString, 0, spannedString.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        } else {
            composingLayout = StaticLayout.Builder.obtain(
                composingText, 0, composingText.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        }

        val firstLineBaseline = composingLayout.getLineBaseline(0).toFloat()

        for (i in 0 until composingLayout.lineCount) {
            val lineStart = composingLayout.getLineStart(i)
            val lineEnd = composingLayout.getLineEnd(i)
            if (lineStart >= lineEnd) continue

            val lineText = composingText.substring(lineStart, lineEnd)
            val drawBaselineY = startBaselineY + (composingLayout.getLineBaseline(i).toFloat() - firstLineBaseline)

            val lineDrawX = composingLayout.getLineLeft(i)

            canvas.drawText(lineText, lineDrawX, drawBaselineY, textPaint)

            val descent = composingLayout.getLineDescent(i).toFloat()
            val underlineY = drawBaselineY + descent + 2f
            val textWidth = textPaint.measureText(lineText)
            if (textWidth > 0f) {
                canvas.drawLine(lineDrawX, underlineY, lineDrawX + textWidth, underlineY, composingUnderlinePaint)
            }
        }

        val cursorOffset = composingCursor.coerceIn(0, composingText.length)
        val cursorLine = composingLayout.getLineForOffset(cursorOffset)
        val cursorXInComposing = composingLayout.getPrimaryHorizontal(cursorOffset)
        val cursorBaselineY = startBaselineY + (composingLayout.getLineBaseline(cursorLine).toFloat() - firstLineBaseline)
        val cursorAscent = composingLayout.getLineAscent(cursorLine).toFloat()
        val cursorDescent = composingLayout.getLineDescent(cursorLine).toFloat()

        hasComposingCursor = true
        composingCursorX = cursorXInComposing
        composingCursorTop = cursorBaselineY + cursorAscent
        composingCursorBottom = cursorBaselineY + cursorDescent
    }

    /**
     * 从事务的 decorationSlices 绘制预输入装饰（下划线、IME cursor）。
     * 预输入文字本身已通过 AnimatedSlice 渲染，这里只画装饰。
     * DecorationSlice 与正文切片共享 Timeline。
     */
    private fun drawTransactionDecorationSlices(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingText: String,
        composingCursor: Int
    ) {
        val compositionTx = activeTransactions.find {
            it.operationKind == AndroidVisualOperationKind.CompositionUpdate &&
            (it.state == AndroidVisualTransactionState.Rendering ||
             it.state == AndroidVisualTransactionState.Paused ||
             it.state == AndroidVisualTransactionState.Prepared)
        }

        if (compositionTx == null || composingText.isEmpty()) return

        val safeOffset = composingStart.coerceIn(0, text.length)
        val startLine = if (text.isNotEmpty()) layout.getLineForOffset(safeOffset) else 0
        val startX = if (text.isNotEmpty()) layout.getPrimaryHorizontal(safeOffset) else 0f
        val startBaselineY = if (text.isNotEmpty()) layout.getLineBaseline(startLine).toFloat() else textPaint.textSize

        val layoutWidth = layout.width.coerceAtLeast(1)
        val composingLayout: StaticLayout

        if (startX > 0f) {
            val indentPx = Math.round(startX)
            val spannedString = android.text.SpannableString(composingText)
            val marginSpan = android.text.style.LeadingMarginSpan.Standard(indentPx, 0)
            spannedString.setSpan(marginSpan, 0, composingText.length, android.text.Spannable.SPAN_INCLUSIVE_INCLUSIVE)

            composingLayout = StaticLayout.Builder.obtain(
                spannedString, 0, spannedString.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        } else {
            composingLayout = StaticLayout.Builder.obtain(
                composingText, 0, composingText.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        }

        val firstLineBaseline = composingLayout.getLineBaseline(0).toFloat()

        for (decSlice in compositionTx.decorationSlices) {
            when (decSlice.kind) {
                DecorationKind.Underline -> {
                    val decStart = decSlice.rangeUtf16.first.coerceIn(0, composingText.length)
                    val decEnd = decSlice.rangeUtf16.last.coerceIn(0, composingText.length)
                    if (decStart >= decEnd) continue

                    val startDecLine = composingLayout.getLineForOffset(decStart)
                    val endDecLine = composingLayout.getLineForOffset(decEnd)

                    for (line in startDecLine..endDecLine) {
                        val lineStart = composingLayout.getLineStart(line)
                        val lineEnd = composingLayout.getLineEnd(line)
                        val cStart = if (line == startDecLine) decStart else lineStart
                        val cEnd = if (line == endDecLine) decEnd else lineEnd
                        if (cStart >= cEnd) continue

                        val lineText = composingText.substring(cStart, cEnd)
                        val drawBaselineY = startBaselineY + (composingLayout.getLineBaseline(line).toFloat() - firstLineBaseline)
                        val lineDrawX = composingLayout.getPrimaryHorizontal(cStart)
                        val descent = composingLayout.getLineDescent(line).toFloat()
                        val underlineY = drawBaselineY + descent + 2f
                        val textWidth = textPaint.measureText(lineText)
                        if (textWidth > 0f) {
                            canvas.drawLine(lineDrawX, underlineY, lineDrawX + textWidth, underlineY, composingUnderlinePaint)
                        }
                    }
                }
                DecorationKind.ComposingCursor -> {
                    val cursorOffset = composingCursor.coerceIn(0, composingText.length)
                    val cursorLine = composingLayout.getLineForOffset(cursorOffset)
                    val cursorXInComposing = composingLayout.getPrimaryHorizontal(cursorOffset)
                    val cursorBaselineY = startBaselineY + (composingLayout.getLineBaseline(cursorLine).toFloat() - firstLineBaseline)
                    val cursorAscent = composingLayout.getLineAscent(cursorLine).toFloat()
                    val cursorDescent = composingLayout.getLineDescent(cursorLine).toFloat()

                    hasComposingCursor = true
                    composingCursorX = cursorXInComposing
                    composingCursorTop = cursorBaselineY + cursorAscent
                    composingCursorBottom = cursorBaselineY + cursorDescent
                }
                DecorationKind.SegmentColor -> {
                    // TODO: segment text color decoration
                }
            }
        }
    }

    private fun drawCompositionDecoration(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingText: String,
        composingCursor: Int
    ) {
        if (composingText.isEmpty()) return

        val safeOffset = composingStart.coerceIn(0, text.length)
        val startLine = if (text.isNotEmpty()) layout.getLineForOffset(safeOffset) else 0
        val startX = if (text.isNotEmpty()) layout.getPrimaryHorizontal(safeOffset) else 0f
        val startBaselineY = if (text.isNotEmpty()) layout.getLineBaseline(startLine).toFloat() else textPaint.textSize

        val layoutWidth = layout.width.coerceAtLeast(1)
        val composingLayout: StaticLayout

        if (startX > 0f) {
            val indentPx = Math.round(startX)
            val spannedString = android.text.SpannableString(composingText)
            val marginSpan = android.text.style.LeadingMarginSpan.Standard(indentPx, 0)
            spannedString.setSpan(marginSpan, 0, composingText.length, android.text.Spannable.SPAN_INCLUSIVE_INCLUSIVE)

            composingLayout = StaticLayout.Builder.obtain(
                spannedString, 0, spannedString.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        } else {
            composingLayout = StaticLayout.Builder.obtain(
                composingText, 0, composingText.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        }

        val firstLineBaseline = composingLayout.getLineBaseline(0).toFloat()

        for (i in 0 until composingLayout.lineCount) {
            val lineStart = composingLayout.getLineStart(i)
            val lineEnd = composingLayout.getLineEnd(i)
            if (lineStart >= lineEnd) continue

            val lineText = composingText.substring(lineStart, lineEnd)
            val drawBaselineY = startBaselineY + (composingLayout.getLineBaseline(i).toFloat() - firstLineBaseline)
            val lineDrawX = composingLayout.getLineLeft(i)

            val descent = composingLayout.getLineDescent(i).toFloat()
            val underlineY = drawBaselineY + descent + 2f
            val textWidth = textPaint.measureText(lineText)
            if (textWidth > 0f) {
                canvas.drawLine(lineDrawX, underlineY, lineDrawX + textWidth, underlineY, composingUnderlinePaint)
            }
        }

        val cursorOffset = composingCursor.coerceIn(0, composingText.length)
        val cursorLine = composingLayout.getLineForOffset(cursorOffset)
        val cursorXInComposing = composingLayout.getPrimaryHorizontal(cursorOffset)
        val cursorBaselineY = startBaselineY + (composingLayout.getLineBaseline(cursorLine).toFloat() - firstLineBaseline)
        val cursorAscent = composingLayout.getLineAscent(cursorLine).toFloat()
        val cursorDescent = composingLayout.getLineDescent(cursorLine).toFloat()

        hasComposingCursor = true
        composingCursorX = cursorXInComposing
        composingCursorTop = cursorBaselineY + cursorAscent
        composingCursorBottom = cursorBaselineY + cursorDescent
    }
}
