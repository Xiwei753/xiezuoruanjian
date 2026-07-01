package com.xiwei.sujian.editor.selfrender

import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.model.EditorVisualTransactionData
import com.xiwei.sujian.model.SujianCursorRectData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.SujianGlyphRectData
import com.xiwei.sujian.model.SujianReflowGlyphRectData
import com.xiwei.sujian.model.SujianVisualEditContext
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianAnimationController — 自研写作区动画控制器（唯一主路径）
 *
 * 管理 EditorVisualTransaction 的接收、分发、生命周期。
 *
 * ## 动画路线
 * - 真吞吐：静态层跳过 inserted range + overlay 层绘制
 * - 禁止：ghost overlay（正文完整绘制后叠 ghost 必然重影）
 * - 禁止：透明 span 污染 Editable
 *
 * ## 动画规则
 * - 插入成功后生成 transaction，Core 返回 Insert 事件，layout 计算新 glyph rect，
 *   静态层动画期间跳过 inserted range，动画层从 oldCursorRect 吐到 glyphRect
 * - 删除：删除前记录 deletedText/deletedGlyphRects/oldCursorRect，buffer 删除，
 *   Core 返回 Delete，layout 算 newCursorRect，动画层把 deletedGlyphRects 吞向 newCursorRect
 * - 连续删除每次独立 animation id，不允许 pendingDelete 覆盖丢动画
 * - 输入类型：普通单字 Typing；中文/日文 IME commit 多字 TypingCommit；
 *   composing ImeComposition 不动画；粘贴 Paste 不动画；加载 Load 不动画；
 *   设置变化 Format/Programmatic 不动画
 * - 滚动中不播放动画
 *
 * ## Phase 2 视觉事务路线
 * - runVisualEdit 捕获 oldCursorRect/newCursorRect，构造 SujianVisualEditContext
 * - handleVisualEdit(context) 接收上下文，调用 fetchVisualTransaction 获取 Core 事务
 * - handleInsertTransaction(vt) 从 vt.oldCursorRect 获取起点坐标，不再从 layout 反推
 * - handleDeleteTransaction(vt) 从 consumeDeleteSnapshot 获取 deletedGlyphRects
 */
class SujianAnimationController(
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout,
    private val renderer: SujianEditorRenderer,
    private val cursorController: SujianCursorController
) {
    private val TAG = "SujianAnimCtrl"
    
    var animationEnabled: Boolean = false
    var animationDurationMs: Long = 160L
    var coordinatedAnimationEnabled: Boolean = true
    
    // ── 删除前快照 ──
    // 每次删除操作独立记录，不允许 pendingDelete 覆盖丢动画
    data class DeleteSnapshot(
        val deletedText: String,
        val deletedGlyphRects: List<SujianGlyphRect>,
        val oldCursorRect: SujianCursorRect,
        val animationId: ULong
    )
    private val deleteSnapshots = mutableListOf<DeleteSnapshot>()
    
    // 最近一次 onBeforeDelete 返回的 animationId，
    // 用于在 handleDeleteTransaction 中精确匹配 Core 返回的 Delete 事件
    private var lastDeleteSnapshotId: ULong = 0u
    
    /**
     * 记录删除前快照
     */
    fun recordDeleteSnapshot(
        deletedText: String,
        deletedGlyphRects: List<SujianGlyphRect>,
        oldCursorRect: SujianCursorRect
    ): ULong {
        val id = nextAnimationId()
        deleteSnapshots.add(DeleteSnapshot(deletedText, deletedGlyphRects, oldCursorRect, id))
        lastDeleteSnapshotId = id
        return id
    }
    
    /**
     * 获取并移除删除快照（精确匹配）
     */
    fun consumeDeleteSnapshot(id: ULong): DeleteSnapshot? {
        val idx = deleteSnapshots.indexOfFirst { it.animationId == id }
        if (idx < 0) return null
        return deleteSnapshots.removeAt(idx)
    }
    
    // ── Phase 2: 视觉事务处理 ──
    
    /**
     * 处理视觉编辑上下文（由 runVisualEdit 调用）。
     *
     * 1. 判断是否需要动画（shouldAnimateForCause）
     * 2. 调用 fetchVisualTransaction 获取 Core 视觉事务
     * 3. 根据事务类型分发到 handleInsertTransaction 或 handleDeleteTransaction
     */
    fun handleVisualEdit(context: SujianVisualEditContext, view: SujianEditorView) {
        if (!animationEnabled) return
        if (!shouldAnimateForCause(context.cause)) return
        
        val vt = fetchVisualTransaction(context, view)
        if (vt == null) {
            DiagnosticsLogger.d(TAG, "No visual transaction from Core for cause=${context.cause}")
            return
        }
        
        // 填充坐标字段
        vt.oldCursorRect = context.oldCursorRect
        vt.newCursorRect = context.newCursorRect
        
        // 填充 reflow 数据（由 runVisualEdit 计算并传入）
        vt.reflowGlyphRects = context.reflowGlyphRects
        
        when (vt.kind) {
            EditorAnimationKindData.Insert -> {
                handleInsertTransaction(vt)
                // 协同光标：光标和文字动画共用 vt 的 old/new cursor rect
                if (coordinatedAnimationEnabled && vt.oldCursorRect != null && vt.newCursorRect != null) {
                    val newRect = vt.newCursorRect!!
                    cursorController.updateCursorTarget(
                        newRect.x.toFloat(),
                        newRect.top.toFloat(),
                        newRect.bottom.toFloat(),
                        true
                    )
                }
            }
            EditorAnimationKindData.Delete -> {
                handleDeleteTransaction(vt)
                // 协同光标：光标和文字动画共用 vt 的 old/new cursor rect
                if (coordinatedAnimationEnabled && vt.oldCursorRect != null && vt.newCursorRect != null) {
                    val newRect = vt.newCursorRect!!
                    cursorController.updateCursorTarget(
                        newRect.x.toFloat(),
                        newRect.top.toFloat(),
                        newRect.bottom.toFloat(),
                        true
                    )
                }
            }
            EditorAnimationKindData.Cursor -> {
                // Cursor 动画由 CursorController 处理
            }
        }
    }
    
    /**
     * 处理插入视觉事务。
     *
     * **关键变更**：从 vt.oldCursorRect 获取起点坐标，不再调用 layout.getCursorRect 反推。
     * 使用 vt.insertedRangeStart/End（UTF-8 → UTF-16 转换后）获取 glyph rects。
     */
    fun handleInsertTransaction(vt: EditorVisualTransactionData) {
        if (!animationEnabled) return
        
        val text = buffer.text
        
        // UTF-8 → UTF-16 转换 insertedRange
        val rangeStartUtf16 = buffer.utf8ToUtf16(vt.insertedRangeStart)
        val rangeEndUtf16 = buffer.utf8ToUtf16(vt.insertedRangeEnd)
        
        // 跳过复杂 grapheme（emoji/ZWJ/variation selector/combining mark）
        if (shouldSkipGlyphAnimation(text, rangeStartUtf16, rangeEndUtf16)) {
            DiagnosticsLogger.d(TAG, "Skipping glyph animation for complex grapheme at [$rangeStartUtf16, $rangeEndUtf16)")
            return
        }
        
        // 获取插入的 glyph rects（从新文本的新布局中获取）
        val glyphRects = layout.getGlyphRects(text, rangeStartUtf16, rangeEndUtf16)
        
        // 防御：glyphRects 为空时直接 return，不设置 animatedInsertRange/hidden range，不 addAnimation
        if (glyphRects.isEmpty()) {
            DiagnosticsLogger.d(TAG, "No glyph rects for insert transaction at [$rangeStartUtf16, $rangeEndUtf16), skipping animation")
            return
        }
        
        vt.insertGlyphRects = glyphRects.map { gr ->
            SujianGlyphRectData(gr.x.toDouble(), gr.y.toDouble(), gr.w.toDouble(), gr.h.toDouble(), gr.char, gr.baselineY.toDouble())
        }
        
        // 从 vt.oldCursorRect 获取起点坐标（关键变更：不再从 layout 反推）
        val oldCursorRect = vt.oldCursorRect
        val startX = oldCursorRect?.x?.toFloat() ?: run {
            // fallback：如果 oldCursorRect 未填充，使用 layout 计算
            val cr = layout.getCursorRect(text, rangeStartUtf16)
            cr.x
        }
        val startY = oldCursorRect?.top?.toFloat() ?: run {
            val cr = layout.getCursorRect(text, rangeStartUtf16)
            cr.top
        }
        val startBaselineY = oldCursorRect?.baselineY?.toFloat() ?: run {
            val cr = layout.getCursorRect(text, rangeStartUtf16)
            cr.baselineY
        }
        
        // 设置静态层跳过范围，避免插入动画期间重影
        // 每个 insert 动画独立添加 range，动画完成时只移除自己的 range
        val insertRangeUtf16 = IntRange(rangeStartUtf16, rangeEndUtf16)
        renderer.addActiveInsertRange(insertRangeUtf16)
        
        renderer.addAnimation(SujianOverlayAnim(
            id = vt.id,
            kind = "insert",
            text = vt.newText.substring(rangeStartUtf16, rangeEndUtf16.coerceAtMost(vt.newText.length)),
            startX = startX,
            startY = startY,
            startBaselineY = startBaselineY,
            endX = if (glyphRects.isNotEmpty()) glyphRects.first().x else startX,
            endY = if (glyphRects.isNotEmpty()) glyphRects.first().y else startY,
            endBaselineY = if (glyphRects.isNotEmpty()) glyphRects.first().baselineY else startBaselineY,
            durationMs = vt.durationMs,
            startTimeMs = System.currentTimeMillis(),
            glyphRects = glyphRects,
            insertRange = insertRangeUtf16
        ))
        
        // ── Reflow 处理（与 Desktop 方案 B 一致：暂不做 reflow 动画，右侧文字 snap） ──
        // 检查 vt.reflowGlyphRects 是否存在，记录日志但不创建 reflow 动画
        // 为未来启用 reflow 预留：数据已从 context 传入并存储在 vt 中
        val reflowRects = vt.reflowGlyphRects
        if (reflowRects.isNotEmpty()) {
            DiagnosticsLogger.d(TAG, "Insert transaction ${vt.id}: ${reflowRects.size} reflow glyphs detected, " +
                "animation disabled (Plan B: snap to final position)")
            // 未来启用 reflow 动画时，在此处将 reflowRects 转换为 Android 坐标格式
            // 并调用 renderer.addAnimation(kind="reflow", ...)
            // 当前：右侧文字直接 snap 到最终位置，不做位移动画
        }
    }
    
    /**
     * 处理删除视觉事务。
     *
     * 从 consumeDeleteSnapshot() 获取 deletedGlyphRects。
     * 使用 vt.newCursorRect 作为动画终点。
     * 禁止从新 layout 反推已删 glyph 位置。
     */
    fun handleDeleteTransaction(vt: EditorVisualTransactionData) {
        if (!animationEnabled) return
        
        // 使用 lastDeleteSnapshotId 精确匹配最近的删除快照
        val snapshot = consumeDeleteSnapshot(lastDeleteSnapshotId)
        
        // 从 vt.newCursorRect 获取终点坐标
        val newCursorRect = vt.newCursorRect
        val endX = newCursorRect?.x?.toFloat() ?: layout.getCursorRect(buffer.text, buffer.selection.head).x
        val endY = newCursorRect?.top?.toFloat() ?: layout.getCursorRect(buffer.text, buffer.selection.head).top
        val endBaselineY = newCursorRect?.baselineY?.toFloat() ?: layout.getCursorRect(buffer.text, buffer.selection.head).baselineY
        
        if (snapshot != null) {
            renderer.addAnimation(SujianOverlayAnim(
                id = vt.id,
                kind = "delete",
                text = snapshot.deletedText,
                startX = snapshot.oldCursorRect.x,
                startY = snapshot.oldCursorRect.top,
                startBaselineY = snapshot.oldCursorRect.baselineY,
                endX = endX,
                endY = endY,
                endBaselineY = endBaselineY,
                durationMs = vt.durationMs,
                startTimeMs = System.currentTimeMillis(),
                glyphRects = snapshot.deletedGlyphRects
            ))
        } else {
            // 没有匹配的快照时，尝试 FIFO fallback
            val fallbackSnapshot = deleteSnapshots.firstOrNull()
            if (fallbackSnapshot != null) {
                deleteSnapshots.remove(fallbackSnapshot)
                renderer.addAnimation(SujianOverlayAnim(
                    id = vt.id,
                    kind = "delete",
                    text = fallbackSnapshot.deletedText,
                    startX = fallbackSnapshot.oldCursorRect.x,
                    startY = fallbackSnapshot.oldCursorRect.top,
                    startBaselineY = fallbackSnapshot.oldCursorRect.baselineY,
                    endX = endX,
                    endY = endY,
                    endBaselineY = endBaselineY,
                    durationMs = vt.durationMs,
                    startTimeMs = System.currentTimeMillis(),
                    glyphRects = fallbackSnapshot.deletedGlyphRects
                ))
            } else {
                // 完全没有快照时，使用 Core 事件信息创建简化动画
                DiagnosticsLogger.d(TAG, "No delete snapshot for transaction ${vt.id}, using fallback")
                renderer.addAnimation(SujianOverlayAnim(
                    id = vt.id,
                    kind = "delete",
                    text = vt.oldText,
                    startX = endX,
                    startY = endY,
                    startBaselineY = endBaselineY,
                    endX = endX,
                    endY = endY,
                    endBaselineY = endBaselineY,
                    durationMs = vt.durationMs,
                    startTimeMs = System.currentTimeMillis(),
                    glyphRects = emptyList()
                ))
            }
        }
    }
    
    /**
     * 从 Core 获取视觉事务。
     *
     * 通过 SujianEditorView 的 visualTransactionProvider 调用 Core API。
     * 必须使用 context 中的快照，不许再用当前 buffer 同时当 old/new，
     * 否则 Core 无法算出真实的 Insert/Delete。
     */
    private fun fetchVisualTransaction(
        context: SujianVisualEditContext,
        view: SujianEditorView
    ): EditorVisualTransactionData? {
        val provider = view.visualTransactionProvider ?: return null

        // 使用 context 中的快照，不再从 buffer 取
        val oldText = context.oldText
        val newText = context.newText

        // UTF-8 offset 用对应文本转换：oldSelection 用 oldText，newSelection 用 newText
        val oldCursorUtf8 = SujianEditorBuffer.utf16ToUtf8(oldText, context.oldSelectionHead)
        val newCursorUtf8 = SujianEditorBuffer.utf16ToUtf8(newText, context.newSelectionHead)

        val causeStr = context.cause.toCoreCauseString()

        return try {
            provider.provide(
                oldText = oldText,
                newText = newText,
                oldCursorIndex = oldCursorUtf8.toUInt(),
                newCursorIndex = newCursorUtf8.toUInt(),
                cause = causeStr,
                maxAnimatedChars = buffer.maxAnimatedChars.toUInt(),
                animationDurationMs = buffer.animationDurationMs.toULong()
            )
        } catch (e: Exception) {
            DiagnosticsLogger.d(TAG, "fetchVisualTransaction failed: ${e.message}")
            null
        }
    }
    
    /**
     * 设置滚动状态
     */
    fun setScrolling(scrolling: Boolean) {
        renderer.setScrolling(scrolling)
    }
    
    /**
     * Tick 动画（每帧调用）
     */
    fun tick() {
        renderer.tickAnimations()
    }
    
    /**
     * 是否有活跃动画
     */
    fun hasActiveAnimations(): Boolean = renderer.hasActiveAnimations()
    
    fun onDetachedFromWindow() {
        renderer.clearAnimations()
        deleteSnapshots.clear()
    }
    
    // ── 内部方法 ──
    
    /**
     * Phase 2: 判断是否需要动画（使用 SujianEditCauseData）
     */
    private fun shouldAnimateForCause(cause: SujianEditCauseData): Boolean {
        return when (cause) {
            SujianEditCauseData.Typing,
            SujianEditCauseData.Delete,
            SujianEditCauseData.TypingCommit -> true
            SujianEditCauseData.Paste,
            SujianEditCauseData.Load,
            SujianEditCauseData.Format,
            SujianEditCauseData.ImeComposition,
            SujianEditCauseData.Undo,
            SujianEditCauseData.Redo,
            SujianEditCauseData.Programmatic -> false
        }
    }
    
    /**
     * 检查是否应跳过复杂 grapheme 的 glyph 动画
     * （emoji/ZWJ/variation selector/combining mark/regional indicator/surrogate pair）
     */
    private fun shouldSkipGlyphAnimation(text: String, startUtf16: Int, endUtf16: Int): Boolean {
        if (startUtf16 >= endUtf16 || startUtf16 >= text.length) return false
        for (i in startUtf16 until endUtf16.coerceAtMost(text.length)) {
            val codePoint = text.codePointAt(i)
            // Surrogate pair（emoji 等）
            if (Character.isHighSurrogate(text[i]) || Character.isLowSurrogate(text[i])) {
                return true
            }
            // ZWJ (U+200D)
            if (codePoint == 0x200D) {
                return true
            }
            // Variation Selector (U+FE00-U+FE0F, U+E0100-U+E01EF)
            if ((codePoint in 0xFE00..0xFE0F) || (codePoint in 0xE0100..0xE01EF)) {
                return true
            }
            // Combining mark (Unicode general category M*)
            val charType = Character.getType(codePoint)
            if (charType == Character.NON_SPACING_MARK.toInt()
                || charType == Character.COMBINING_SPACING_MARK.toInt()
                || charType == Character.ENCLOSING_MARK.toInt()
            ) {
                return true
            }
            // Regional Indicator (U+1F1E6-U+1F1FF)
            if (codePoint in 0x1F1E6..0x1F1FF) {
                return true
            }
        }
        return false
    }
    
    companion object {
        private var globalAnimationId: ULong = 1u
        
        private fun nextAnimationId(): ULong {
            val id = globalAnimationId
            globalAnimationId = globalAnimationId.inc()
            return id
        }
    }
}

/**
 * SujianEditCauseData → Core cause string 转换
 */
private fun SujianEditCauseData.toCoreCauseString(): String = when (this) {
    SujianEditCauseData.Typing -> "Typing"
    SujianEditCauseData.Delete -> "Delete"
    SujianEditCauseData.ImeComposition -> "ImeComposition"
    SujianEditCauseData.TypingCommit -> "TypingCommit"
    SujianEditCauseData.Paste -> "Paste"
    SujianEditCauseData.Undo -> "Undo"
    SujianEditCauseData.Redo -> "Redo"
    SujianEditCauseData.Load -> "Load"
    SujianEditCauseData.Format -> "Format"
    SujianEditCauseData.Programmatic -> "Programmatic"
}
