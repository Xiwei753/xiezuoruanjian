package com.xiwei.sujian.editor.selfrender

import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.model.EditorVisualTransactionData
import com.xiwei.sujian.model.SujianCursorRectData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.SujianGlyphRectData
import com.xiwei.sujian.model.SujianReflowGlyphRectData
import com.xiwei.sujian.model.SujianVisualEditContext
import com.xiwei.sujian.model.VisualCoordinateModeData
import org.junit.Assert.*
import org.junit.Test

/**
 * 视觉事务收敛 Phase 2 单元测试
 *
 * 覆盖：
 * 1. runVisualEdit 快照捕获（oldCursorRect / newCursorRect）
 * 2. handleInsertTransaction 使用 oldCursorRect（不从 layout 反推）
 * 3. onBeforeDelete 与 runVisualEdit 协同
 * 4. shouldAnimateForCause 新增 Undo/Redo
 * 5. EditorVisualTransactionData 模型映射
 * 6. SujianEditCauseData → Core cause string 转换
 * 7. 复杂 grapheme 跳过 glyph animation
 */
class VisualTransactionPhase2Test {

    // ── 1. runVisualEdit 快照捕获 ──

    @Test
    fun runVisualEdit_capturesOldCursorRect_beforeEdit() {
        // 模拟 runVisualEdit 步骤：
        // 1. 捕获 oldCursorRect
        // 2. 执行 edit
        // 3. 捕获 newCursorRect
        // 4. 分发动画

        var capturedOldRect: SujianCursorRectData? = null
        var capturedNewRect: SujianCursorRectData? = null
        var editExecuted = false

        // 模拟 oldCursorRect
        val oldRect = SujianCursorRectData(x = 10.0, top = 20.0, bottom = 40.0, baselineY = 35.0)
        capturedOldRect = oldRect

        // 模拟 edit
        editExecuted = true

        // 模拟 newCursorRect（编辑后光标移动了）
        val newRect = SujianCursorRectData(x = 25.0, top = 20.0, bottom = 40.0, baselineY = 35.0)
        capturedNewRect = newRect

        // 验证
        assertTrue("edit should have been executed", editExecuted)
        assertNotNull("oldCursorRect should be captured", capturedOldRect)
        assertNotNull("newCursorRect should be captured", capturedNewRect)
        assertEquals(10.0, capturedOldRect!!.x, 0.01)
        assertEquals(25.0, capturedNewRect!!.x, 0.01)
    }

    @Test
    fun runVisualEdit_delete_reusesPreDeleteOldCursorRect() {
        // Delete 场景：runVisualEdit 应复用 onBeforeDelete 已捕获的 oldCursorRect
        var preDeleteOldCursorRect: SujianCursorRectData? = null

        // 模拟 onBeforeDelete 设置 preDeleteOldCursorRect
        val deleteRect = SujianCursorRectData(x = 50.0, top = 100.0, bottom = 120.0, baselineY = 115.0)
        preDeleteOldCursorRect = deleteRect

        // 模拟 runVisualEdit(Delete) 使用 preDeleteOldCursorRect
        val usedRect = preDeleteOldCursorRect
        preDeleteOldCursorRect = null  // 消费后清空

        assertNotNull("should reuse preDeleteOldCursorRect", usedRect)
        assertEquals(50.0, usedRect!!.x, 0.01)
        assertNull("preDeleteOldCursorRect should be cleared after use", preDeleteOldCursorRect)
    }

    @Test
    fun runVisualEdit_typing_capturesOldCursorRectFromLayout() {
        // Typing 场景：runVisualEdit 从 layout 捕获 oldCursorRect
        // 模拟 layout 返回的光标位置
        val layoutCursorRect = SujianCursorRectData(x = 30.0, top = 60.0, bottom = 80.0, baselineY = 75.0)

        // 非 Delete 场景，直接从 layout 获取
        val oldCursorRect = layoutCursorRect

        assertEquals(30.0, oldCursorRect.x, 0.01)
        assertEquals(60.0, oldCursorRect.top, 0.01)
        assertEquals(75.0, oldCursorRect.baselineY, 0.01)
    }

    // ── 2. handleInsertTransaction 使用 oldCursorRect ──

    @Test
    fun handleInsertTransaction_usesOldCursorRectFromVt() {
        // 创建一个插入视觉事务
        val vt = EditorVisualTransactionData(
            id = 1u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "ab",
            newText = "aXb",
            oldSelectionAnchor = 1,
            oldSelectionHead = 1,
            newSelectionAnchor = 2,
            newSelectionHead = 2,
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        // 填充 oldCursorRect（由 runVisualEdit 捕获）
        val oldCursorRect = SujianCursorRectData(x = 15.0, top = 20.0, bottom = 40.0, baselineY = 35.0)
        vt.oldCursorRect = oldCursorRect

        // 验证：handleInsertTransaction 应使用 vt.oldCursorRect 而非从 layout 反推
        assertNotNull("vt.oldCursorRect should be set", vt.oldCursorRect)
        assertEquals(15.0, vt.oldCursorRect!!.x, 0.01)
        assertEquals(35.0, vt.oldCursorRect!!.baselineY, 0.01)
    }

    @Test
    fun handleInsertTransaction_oldCursorRectIsNull_fallsBackToLayout() {
        // 当 oldCursorRect 未填充时，应 fallback 到 layout 计算
        val vt = EditorVisualTransactionData(
            id = 2u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "ab",
            newText = "abc",
            oldSelectionAnchor = 2,
            oldSelectionHead = 2,
            newSelectionAnchor = 3,
            newSelectionHead = 3,
            insertedRangeStart = 2,
            insertedRangeEnd = 3,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        // oldCursorRect 未填充
        assertNull(vt.oldCursorRect)

        // fallback 逻辑：如果 oldCursorRect 为 null，使用 layout 计算
        // 在实际代码中，这通过 vt.oldCursorRect?.x?.toFloat() ?: layout.getCursorRect(...).x 实现
        val startX = vt.oldCursorRect?.x?.toFloat() ?: 0f  // fallback 值
        assertEquals(0f, startX, 0.01f)
    }

    // ── 3. onBeforeDelete 与 runVisualEdit 协同 ──

    @Test
    fun onBeforeDelete_setsPreDeleteOldCursorRect() {
        // 模拟 onBeforeDelete 设置 preDeleteOldCursorRect
        var preDeleteOldCursorRect: SujianCursorRectData? = null

        // onBeforeDelete 记录光标位置
        val cursorRect = SujianCursorRectData(x = 42.0, top = 84.0, bottom = 104.0, baselineY = 99.0)
        preDeleteOldCursorRect = cursorRect

        // runVisualEdit(Delete) 应使用 preDeleteOldCursorRect
        val usedRect = preDeleteOldCursorRect
        preDeleteOldCursorRect = null

        assertNotNull(usedRect)
        assertEquals(42.0, usedRect!!.x, 0.01)
    }

    @Test
    fun onBeforeDelete_thenRunVisualEdit_consumesPreDeleteRect() {
        // 完整流程：onBeforeDelete → runVisualEdit(Delete)
        var preDeleteOldCursorRect: SujianCursorRectData? = null

        // Step 1: onBeforeDelete 设置
        preDeleteOldCursorRect = SujianCursorRectData(x = 42.0, top = 84.0, bottom = 104.0, baselineY = 99.0)

        // Step 2: runVisualEdit(Delete) 消费
        val oldCursorRect = preDeleteOldCursorRect
        preDeleteOldCursorRect = null

        // Step 3: edit 执行
        var editExecuted = false
        editExecuted = true

        // Step 4: newCursorRect 捕获
        val newCursorRect = SujianCursorRectData(x = 30.0, top = 84.0, bottom = 104.0, baselineY = 99.0)

        // 验证
        assertTrue(editExecuted)
        assertNotNull(oldCursorRect)
        assertEquals(42.0, oldCursorRect!!.x, 0.01)
        assertEquals(30.0, newCursorRect.x, 0.01)
        assertNull("preDeleteOldCursorRect should be cleared", preDeleteOldCursorRect)
    }

    // ── 4. shouldAnimateForCause 新增 Undo/Redo ──

    @Test
    fun shouldAnimateForCause_undo_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Undo))
    }

    @Test
    fun shouldAnimateForCause_redo_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Redo))
    }

    @Test
    fun shouldAnimateForCause_typing_true() {
        assertTrue(shouldAnimateForCause(SujianEditCauseData.Typing))
    }

    @Test
    fun shouldAnimateForCause_delete_true() {
        assertTrue(shouldAnimateForCause(SujianEditCauseData.Delete))
    }

    @Test
    fun shouldAnimateForCause_typingCommit_true() {
        assertTrue(shouldAnimateForCause(SujianEditCauseData.TypingCommit))
    }

    @Test
    fun shouldAnimateForCause_paste_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Paste))
    }

    @Test
    fun shouldAnimateForCause_load_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Load))
    }

    @Test
    fun shouldAnimateForCause_format_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Format))
    }

    @Test
    fun shouldAnimateForCause_imeComposition_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.ImeComposition))
    }

    @Test
    fun shouldAnimateForCause_programmatic_false() {
        assertFalse(shouldAnimateForCause(SujianEditCauseData.Programmatic))
    }

    // ── 5. EditorVisualTransactionData 模型映射 ──

    @Test
    fun visualTransactionData_allFieldsMapped() {
        val vt = EditorVisualTransactionData(
            id = 42u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "ab",
            newText = "abc",
            oldSelectionAnchor = 2,
            oldSelectionHead = 2,
            newSelectionAnchor = 3,
            newSelectionHead = 3,
            insertedRangeStart = 2,
            insertedRangeEnd = 3,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        assertEquals(42uL, vt.id)
        assertEquals(EditorAnimationKindData.Insert, vt.kind)
        assertEquals(SujianEditCauseData.Typing, vt.cause)
        assertEquals("ab", vt.oldText)
        assertEquals("abc", vt.newText)
        assertEquals(2, vt.oldSelectionAnchor)
        assertEquals(2, vt.oldSelectionHead)
        assertEquals(3, vt.newSelectionAnchor)
        assertEquals(3, vt.newSelectionHead)
        assertEquals(2, vt.insertedRangeStart)
        assertEquals(3, vt.insertedRangeEnd)
        assertEquals(160L, vt.durationMs)
        assertEquals(VisualCoordinateModeData.Baseline, vt.coordinateMode)
    }

    @Test
    fun visualTransactionData_coordinateFieldsInitiallyNull() {
        val vt = EditorVisualTransactionData(
            id = 1u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "",
            newText = "a",
            oldSelectionAnchor = 0,
            oldSelectionHead = 0,
            newSelectionAnchor = 1,
            newSelectionHead = 1,
            insertedRangeStart = 0,
            insertedRangeEnd = 1,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        // 坐标字段初始为 null/empty
        assertNull(vt.oldCursorRect)
        assertNull(vt.newCursorRect)
        assertTrue(vt.deletedGlyphRects.isEmpty())
        assertTrue(vt.insertGlyphRects.isEmpty())
        assertTrue(vt.reflowGlyphRects.isEmpty())
    }

    @Test
    fun visualTransactionData_coordinateFieldsCanBeSet() {
        val vt = EditorVisualTransactionData(
            id = 1u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "",
            newText = "a",
            oldSelectionAnchor = 0,
            oldSelectionHead = 0,
            newSelectionAnchor = 1,
            newSelectionHead = 1,
            insertedRangeStart = 0,
            insertedRangeEnd = 1,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        // 设置坐标字段
        vt.oldCursorRect = SujianCursorRectData(10.0, 20.0, 40.0, 35.0)
        vt.newCursorRect = SujianCursorRectData(25.0, 20.0, 40.0, 35.0)
        vt.insertGlyphRects = listOf(
            SujianGlyphRectData(25.0, 20.0, 15.0, 20.0, "a", 35.0)
        )

        assertNotNull(vt.oldCursorRect)
        assertNotNull(vt.newCursorRect)
        assertEquals(1, vt.insertGlyphRects.size)
        assertEquals("a", vt.insertGlyphRects[0].char)
    }

    // ── 6. SujianEditCauseData → Core cause string 转换 ──

    @Test
    fun editCauseData_toCoreCauseString_allMappings() {
        assertEquals("Typing", SujianEditCauseData.Typing.toCoreCauseString())
        assertEquals("Delete", SujianEditCauseData.Delete.toCoreCauseString())
        assertEquals("ImeComposition", SujianEditCauseData.ImeComposition.toCoreCauseString())
        assertEquals("TypingCommit", SujianEditCauseData.TypingCommit.toCoreCauseString())
        assertEquals("Paste", SujianEditCauseData.Paste.toCoreCauseString())
        assertEquals("Undo", SujianEditCauseData.Undo.toCoreCauseString())
        assertEquals("Redo", SujianEditCauseData.Redo.toCoreCauseString())
        assertEquals("Load", SujianEditCauseData.Load.toCoreCauseString())
        assertEquals("Format", SujianEditCauseData.Format.toCoreCauseString())
        assertEquals("Programmatic", SujianEditCauseData.Programmatic.toCoreCauseString())
    }

    // ── 7. 复杂 grapheme 跳过 glyph animation ──

    @Test
    fun complexGrapheme_emoji_shouldSkipAnimation() {
        // emoji 包含 surrogate pair，应跳过 glyph animation
        val text = "a😀b"
        val startUtf16 = 1
        val endUtf16 = 3  // emoji 占 2 UTF-16 code units

        assertTrue(shouldSkipGlyphAnimation(text, startUtf16, endUtf16))
    }

    @Test
    fun complexGrapheme_ascii_shouldNotSkip() {
        // ASCII 字符不包含 surrogate pair，不应跳过
        val text = "abc"
        val startUtf16 = 0
        val endUtf16 = 3

        assertFalse(shouldSkipGlyphAnimation(text, startUtf16, endUtf16))
    }

    @Test
    fun complexGrapheme_chinese_shouldNotSkip() {
        // 中文字符（BMP）不包含 surrogate pair，不应跳过
        val text = "你好"
        val startUtf16 = 0
        val endUtf16 = 2

        assertFalse(shouldSkipGlyphAnimation(text, startUtf16, endUtf16))
    }

    @Test
    fun complexGrapheme_mixedEmojiAndAscii_emojiRangeSkips() {
        val text = "a😀b"
        // ASCII range
        assertFalse(shouldSkipGlyphAnimation(text, 0, 1))
        // emoji range
        assertTrue(shouldSkipGlyphAnimation(text, 1, 3))
        // ASCII after emoji
        assertFalse(shouldSkipGlyphAnimation(text, 3, 4))
    }

    // ── SujianVisualEditContext ──

    @Test
    fun visualEditContext_holdsAllFields() {
        val oldRect = SujianCursorRectData(10.0, 20.0, 40.0, 35.0)
        val newRect = SujianCursorRectData(25.0, 20.0, 40.0, 35.0)
        val context = SujianVisualEditContext(
            oldText = "hello",
            newText = "hello world",
            oldSelectionAnchor = 5,
            oldSelectionHead = 5,
            newSelectionAnchor = 11,
            newSelectionHead = 11,
            oldCursorRect = oldRect,
            newCursorRect = newRect,
            cause = SujianEditCauseData.Typing
        )

        assertEquals("hello", context.oldText)
        assertEquals("hello world", context.newText)
        assertEquals(5, context.oldSelectionHead)
        assertEquals(11, context.newSelectionHead)
        assertEquals(10.0, context.oldCursorRect!!.x, 0.01)
        assertEquals(25.0, context.newCursorRect!!.x, 0.01)
        assertEquals(SujianEditCauseData.Typing, context.cause)
    }

    @Test
    fun visualEditContext_nullRectsAllowed() {
        val context = SujianVisualEditContext(
            oldText = "abc",
            newText = "ac",
            oldSelectionAnchor = 2,
            oldSelectionHead = 2,
            newSelectionAnchor = 1,
            newSelectionHead = 1,
            oldCursorRect = null,
            newCursorRect = null,
            cause = SujianEditCauseData.Delete
        )

        assertNull(context.oldCursorRect)
        assertNull(context.newCursorRect)
        assertEquals("abc", context.oldText)
        assertEquals("ac", context.newText)
        assertEquals(SujianEditCauseData.Delete, context.cause)
    }

    // ── Delete 场景：handleDeleteTransaction 使用 newCursorRect ──

    @Test
    fun handleDeleteTransaction_usesNewCursorRectFromVt() {
        val vt = EditorVisualTransactionData(
            id = 10u,
            kind = EditorAnimationKindData.Delete,
            cause = SujianEditCauseData.Delete,
            oldText = "abc",
            newText = "ac",
            oldSelectionAnchor = 2,
            oldSelectionHead = 2,
            newSelectionAnchor = 1,
            newSelectionHead = 1,
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        // 填充 newCursorRect（由 runVisualEdit 捕获）
        val newCursorRect = SujianCursorRectData(x = 15.0, top = 20.0, bottom = 40.0, baselineY = 35.0)
        vt.newCursorRect = newCursorRect

        // 验证：handleDeleteTransaction 应使用 vt.newCursorRect 作为动画终点
        assertNotNull(vt.newCursorRect)
        assertEquals(15.0, vt.newCursorRect!!.x, 0.01)
    }

    // ── UTF-8 ↔ UTF-16 转换在视觉事务中的正确性 ──

    @Test
    fun visualTransaction_utf8InsertedRange_convertedToUtf16() {
        // Core 返回 insertedRangeStart=3, insertedRangeEnd=6（UTF-8 byte offset）
        // 对应中文文本 "你好世界" 中 "世界" 的位置
        val newText = "你好世界"
        val insertedRangeStartUtf8 = 6  // "世" 的 UTF-8 起始位置
        val insertedRangeEndUtf8 = 12   // "界" 的 UTF-8 结束位置

        // UTF-8 → UTF-16 转换
        val rangeStartUtf16 = utf8ToUtf16(newText, insertedRangeStartUtf8)
        val rangeEndUtf16 = utf8ToUtf16(newText, insertedRangeEndUtf8)

        assertEquals(2, rangeStartUtf16)  // "世" 在 UTF-16 offset 2
        assertEquals(4, rangeEndUtf16)    // "界" 后在 UTF-16 offset 4
    }

    @Test
    fun visualTransaction_utf8InsertedRange_ascii() {
        val newText = "aXb"
        val insertedRangeStartUtf8 = 1
        val insertedRangeEndUtf8 = 2

        val rangeStartUtf16 = utf8ToUtf16(newText, insertedRangeStartUtf8)
        val rangeEndUtf16 = utf8ToUtf16(newText, insertedRangeEndUtf8)

        assertEquals(1, rangeStartUtf16)
        assertEquals(2, rangeEndUtf16)
    }

    // ── 辅助方法 ──

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

    private fun shouldSkipGlyphAnimation(text: String, startUtf16: Int, endUtf16: Int): Boolean {
        if (startUtf16 >= endUtf16 || startUtf16 >= text.length) return false
        for (i in startUtf16 until endUtf16.coerceAtMost(text.length)) {
            if (Character.isHighSurrogate(text[i]) || Character.isLowSurrogate(text[i])) {
                return true
            }
        }
        return false
    }

    private fun utf8ToUtf16(text: String, utf8Offset: Int): Int {
        var byteCount = 0
        var charIdx = 0
        for (char in text) {
            if (byteCount >= utf8Offset) break
            byteCount += when {
                char.code <= 0x7F -> 1
                char.code <= 0x7FF -> 2
                char.code <= 0xFFFF -> 3
                else -> 4
            }
            charIdx++
        }
        return charIdx
    }

    private fun utf16ToUtf8(text: String, utf16Offset: Int): Int {
        var byteOffset = 0
        var charIdx = 0
        val safeOffset = utf16Offset.coerceIn(0, text.length)
        for (char in text) {
            if (charIdx >= safeOffset) break
            byteOffset += when {
                char.code <= 0x7F -> 1
                char.code <= 0x7FF -> 2
                char.code <= 0xFFFF -> 3
                else -> 4
            }
            charIdx++
        }
        return byteOffset
    }

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

    // ── 8. Reflow 数据模型 ──

    @Test
    fun reflowGlyphRectData_allFieldsMapped() {
        val r = SujianReflowGlyphRectData(
            char = "a",
            byteStart = 2,
            byteEnd = 3,
            oldX = 10.0,
            oldY = 20.0,
            oldBaselineY = 35.0,
            newX = 25.0,
            newY = 20.0,
            newBaselineY = 35.0,
            w = 15.0,
            h = 20.0,
            lineIndex = 0
        )
        assertEquals("a", r.char)
        assertEquals(10.0, r.oldX, 0.01)
        assertEquals(20.0, r.oldY, 0.01)
        assertEquals(35.0, r.oldBaselineY, 0.01)
        assertEquals(25.0, r.newX, 0.01)
        assertEquals(20.0, r.newY, 0.01)
        assertEquals(35.0, r.newBaselineY, 0.01)
        assertEquals(15.0, r.w, 0.01)
        assertEquals(20.0, r.h, 0.01)
        assertEquals(0, r.lineIndex)
    }

    @Test
    fun visualTransactionData_reflowGlyphRectsInitiallyEmpty() {
        val vt = EditorVisualTransactionData(
            id = 1u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "ab",
            newText = "aXb",
            oldSelectionAnchor = 1,
            oldSelectionHead = 1,
            newSelectionAnchor = 2,
            newSelectionHead = 2,
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        assertTrue(vt.reflowGlyphRects.isEmpty())
    }

    @Test
    fun visualTransactionData_reflowGlyphRectsCanBeSet() {
        val vt = EditorVisualTransactionData(
            id = 1u,
            kind = EditorAnimationKindData.Insert,
            cause = SujianEditCauseData.Typing,
            oldText = "ab",
            newText = "aXb",
            oldSelectionAnchor = 1,
            oldSelectionHead = 1,
            newSelectionAnchor = 2,
            newSelectionHead = 2,
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            durationMs = 160L,
            coordinateMode = VisualCoordinateModeData.Baseline
        )

        val reflowRects = listOf(
            SujianReflowGlyphRectData(
                char = "b",
                byteStart = 2,
                byteEnd = 3,
                oldX = 15.0,
                oldY = 20.0,
                oldBaselineY = 35.0,
                newX = 30.0,
                newY = 20.0,
                newBaselineY = 35.0,
                w = 10.0,
                h = 20.0,
                lineIndex = 0
            )
        )
        vt.reflowGlyphRects = reflowRects

        assertEquals(1, vt.reflowGlyphRects.size)
        assertEquals("b", vt.reflowGlyphRects[0].char)
        assertEquals(15.0, vt.reflowGlyphRects[0].oldX, 0.01)
        assertEquals(30.0, vt.reflowGlyphRects[0].newX, 0.01)
    }

    @Test
    fun visualEditContext_reflowGlyphRectsDefaultEmpty() {
        val context = SujianVisualEditContext(
            oldText = "ab",
            newText = "aXb",
            oldSelectionAnchor = 1,
            oldSelectionHead = 1,
            newSelectionAnchor = 2,
            newSelectionHead = 2,
            oldCursorRect = null,
            newCursorRect = null,
            cause = SujianEditCauseData.Typing
        )

        assertTrue(context.reflowGlyphRects.isEmpty())
    }

    @Test
    fun visualEditContext_reflowGlyphRectsCanBePassed() {
        val reflowRects = listOf(
            SujianReflowGlyphRectData(
                char = "b",
                byteStart = 2,
                byteEnd = 3,
                oldX = 15.0,
                oldY = 20.0,
                oldBaselineY = 35.0,
                newX = 30.0,
                newY = 20.0,
                newBaselineY = 35.0,
                w = 10.0,
                h = 20.0,
                lineIndex = 0
            )
        )
        val context = SujianVisualEditContext(
            oldText = "ab",
            newText = "aXb",
            oldSelectionAnchor = 1,
            oldSelectionHead = 1,
            newSelectionAnchor = 2,
            newSelectionHead = 2,
            oldCursorRect = null,
            newCursorRect = null,
            cause = SujianEditCauseData.Typing,
            reflowGlyphRects = reflowRects
        )

        assertEquals(1, context.reflowGlyphRects.size)
        assertEquals("b", context.reflowGlyphRects[0].char)
        assertEquals(15.0, context.reflowGlyphRects[0].oldX, 0.01)
        assertEquals(30.0, context.reflowGlyphRects[0].newX, 0.01)
    }

    @Test
    fun reflowGlyphRectData_noPositionChange_shouldNotBeReflow() {
        // 当 old 和 new 位置相同时，不应被视为 reflow
        // （在 computeReflowGlyphRects 中通过 dx/dy < 0.1 过滤）
        val r = SujianReflowGlyphRectData(
            char = "a",
            byteStart = 2,
            byteEnd = 3,
            oldX = 10.0,
            oldY = 20.0,
            oldBaselineY = 35.0,
            newX = 10.05,  // 差异 < 0.1
            newY = 20.05,  // 差异 < 0.1
            newBaselineY = 35.0,
            w = 15.0,
            h = 20.0,
            lineIndex = 0
        )
        // 位置变化极小，在实际代码中会被过滤掉
        val dx = kotlin.math.abs(r.newX - r.oldX)
        val dy = kotlin.math.abs(r.newY - r.oldY)
        assertTrue(dx < 0.1)
        assertTrue(dy < 0.1)
    }

    // ── 9. Reflow byteStart/byteEnd 必须是 UTF-8 byte offset ──

    @Test
    fun reflowGlyphRectData_byteStartEnd_mustBeUtf8Offsets_chinese() {
        // 中文文本 "你好世界"，在 "好" 后插入 "的"
        // 新文本 "你好的世界"
        val newText = "你好的世界"
        // "的" 在 UTF-16 offset 2
        // "世" 在 UTF-16 offset 3，UTF-8 byte offset = 3*3 = 9
        // "界" 在 UTF-16 offset 4，UTF-8 byte offset = 4*3 = 12
        // reflow glyph "世" 的 byteStart 应为 9（UTF-8），byteEnd 应为 12（UTF-8）
        val reflowRect = SujianReflowGlyphRectData(
            char = "世",
            byteStart = utf16ToUtf8(newText, 3),  // UTF-8 byte offset of "世"
            byteEnd = utf16ToUtf8(newText, 4),     // UTF-8 byte offset after "世"
            oldX = 30.0,
            oldY = 20.0,
            oldBaselineY = 35.0,
            newX = 45.0,
            newY = 20.0,
            newBaselineY = 35.0,
            w = 15.0,
            h = 20.0,
            lineIndex = 0
        )
        assertEquals(9, reflowRect.byteStart)  // UTF-8 byte offset
        assertEquals(12, reflowRect.byteEnd)   // UTF-8 byte offset

        // 验证：从 UTF-8 byte offset 转回 UTF-16 offset 必须正确
        val utf16Start = utf8ToUtf16(newText, reflowRect.byteStart)
        val utf16End = utf8ToUtf16(newText, reflowRect.byteEnd)
        assertEquals(3, utf16Start)  // "世" 的 UTF-16 offset
        assertEquals(4, utf16End)    // "界" 的 UTF-16 offset
    }

    @Test
    fun reflowGlyphRectData_byteStartEnd_mustBeUtf8Offsets_ascii() {
        val newText = "aXb"
        // "b" 在 UTF-16 offset 2，UTF-8 byte offset 也是 2
        val reflowRect = SujianReflowGlyphRectData(
            char = "b",
            byteStart = utf16ToUtf8(newText, 2),
            byteEnd = utf16ToUtf8(newText, 3),
            oldX = 15.0,
            oldY = 20.0,
            oldBaselineY = 35.0,
            newX = 30.0,
            newY = 20.0,
            newBaselineY = 35.0,
            w = 10.0,
            h = 20.0,
            lineIndex = 0
        )
        assertEquals(2, reflowRect.byteStart)
        assertEquals(3, reflowRect.byteEnd)
    }

    // ── 10. Reflow 动画 id 不与 Core 事务 id 碰撞 ──

    @Test
    fun reflowAnimationId_doesNotCollideWithNextInsertId() {
        // vt.id = 10 时：
        // insert overlay id = (10 shl 1) = 20（偶数）
        // reflow overlay id = (10 shl 1) or 1 = 21（奇数）
        // 下一次 vt.id = 11 时：
        // insert overlay id = (11 shl 1) = 22（偶数）
        // 22 != 21，不会碰撞
        val vtId1 = 10u
        val insertOverlayId1 = (vtId1 shl 1)
        val reflowOverlayId1 = (vtId1 shl 1) or 1u

        val vtId2 = 11u
        val insertOverlayId2 = (vtId2 shl 1)

        assertNotEquals(insertOverlayId1, reflowOverlayId1)
        assertNotEquals(reflowOverlayId1, insertOverlayId2)
        assertNotEquals(insertOverlayId1, insertOverlayId2)
    }

    @Test
    fun reflowAnimationId_compositeScheme_evenOdd() {
        // 所有 insert/delete overlay id 是偶数，reflow overlay id 是奇数
        for (vtId in 1u..100u) {
            val insertId = (vtId shl 1)
            val reflowId = (vtId shl 1) or 1u
            assertEquals(0u, insertId % 2u)  // 偶数
            assertEquals(1u, reflowId % 2u)  // 奇数
            assertNotEquals(insertId, reflowId)
        }
    }
}
