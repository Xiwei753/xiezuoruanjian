package com.xiwei.sujian.editor.selfrender

import android.content.Context
import android.content.res.Configuration
import android.graphics.Canvas

import android.graphics.Paint
import android.graphics.Typeface
import android.text.TextPaint
import android.util.AttributeSet
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.accessibility.AccessibilityEvent
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.SujianCursorRectData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.SujianVisualEditContext
import com.google.android.material.color.MaterialColors

/**
 * SujianEditorView — Android 自研写作区核心 View（唯一主路径）
 *
 * 继承 View（不继承 EditText），整合所有控制器。
 * 只替换写作正文区域，不重写整个 Android UI。
 *
 * ## 动画路线
 * - 真吞吐：静态层跳过 inserted range + overlay 层绘制
 * - 禁止：ghost overlay（正文完整绘制后叠 ghost 必然重影）
 * - 禁止：透明 span 污染 Editable
 *
 * ## 架构原则
 * - Core 仍唯一业务语义来源
 * - Android 只负责输入适配、布局、绘制、触摸、IME
 * - 文本变化必须生成 EditorTransaction
 * - 动画事件统一走 Core EditorVisualTransaction
 * - 通过同一个 ViewModel 读写章节内容，不能绕过 Core
 *
 * ## 控制器层级
 * ```
 * SujianEditorView
 *   ├── SujianEditorBuffer       (文本缓冲区)
 *   ├── SujianEditorLayout       (布局引擎)
 *   ├── SujianEditorRenderer     (渲染器)
 *   ├── SujianInputConnection    (IME 连接)
 *   ├── SujianImeController      (IME 控制器)
 *   ├── SujianCursorController   (光标控制器)
 *   ├── SujianSelectionController(选区控制器)
 *   ├── SujianAnimationController(动画控制器)
 *   ├── SujianTouchController    (触摸控制器)
 *   └── SujianClipboardController(剪贴板控制器)
 * ```
 */
class SujianEditorView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private val TAG = "SujianEditorView"

    // ── 核心组件 ──
    val textPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
        textSize = 16f * resources.displayMetrics.scaledDensity
        typeface = Typeface.create("sans-serif", Typeface.NORMAL)
    }

    val buffer = SujianEditorBuffer()
    val layoutEngine = SujianEditorLayout(textPaint)
    val renderer = SujianEditorRenderer(textPaint, resources.displayMetrics.density)
    val cursorController = SujianCursorController(this, buffer, renderer)
    val animationController = SujianAnimationController(buffer, layoutEngine, renderer, cursorController)
    val imeController = SujianImeController(this, buffer, layoutEngine, renderer, animationController)
    val selectionController = SujianSelectionController(buffer, layoutEngine)
    val touchController = SujianTouchController(this, buffer, layoutEngine, selectionController, cursorController, animationController)
    val clipboardController = SujianClipboardController(context, buffer, animationController, layoutEngine, this)

    // ── 设置缓存 ──
    private var lastFontSize: Float = textPaint.textSize
    private var lastLineSpacingMultiplier: Float = 1.0f
    private var lastFirstLineIndentPx: Float = 0f
    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthChars: Float = 0f

    // ── Delete 场景复用 onBeforeDelete 已捕获的 oldCursorRect ──
    // runVisualEdit(Delete) 时优先使用此字段，避免重复查询 layout
    internal var preDeleteOldCursorRect: SujianCursorRectData? = null

    // ── 视觉事务标志 ──
    // 当 insideVisualEdit=true 时，buffer.onTextChanged 只做必要内容通知，
    // 不移动光标、不分发动画；光标和动画统一由 runVisualEdit → handleVisualEdit 收口
    internal var insideVisualEdit: Boolean = false
        private set

    // ── 方向键上下移动时记忆的 X 坐标 ──
    // 用于上下移动时保持水平位置不变
    private var rememberedCursorX: Float? = null

    // ── 内容变更监听 ──
    var onContentChanged: ((String) -> Unit)? = null

    // ── 初始化 ──
    init {
        // Buffer 变更监听
        buffer.onTextChanged = { result ->
            // 通知布局引擎文本已变
            layoutEngine.invalidate()

            if (insideVisualEdit) {
                // 视觉事务期间：只做必要内容通知，不移动光标、不分发动画
                // 光标、文字动画、ensureCursorVisible 统一由 runVisualEdit → handleVisualEdit 收口
                onContentChanged?.invoke(result.newText)
                invalidate()
            } else {
                // 非视觉事务（如 loadText）：正常更新光标
                val cursorRect = layoutEngine.getCursorRect(result.newText, buffer.selection.head)
                cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, true)
                touchController.ensureCursorVisible()
                onContentChanged?.invoke(result.newText)
                invalidate()
            }
        }

        // 可聚焦，可获取输入
        isFocusable = true
        isFocusableInTouchMode = true
        isClickable = true
        isLongClickable = true

        // 初始应用主题颜色
        applyThemeColors()
    }

    // ── 视觉事务提供者（Phase 2） ──
    internal var visualTransactionProvider: com.xiwei.sujian.ui.VisualTransactionProvider? = null

    /**
     * 注入 Core 视觉事务提供者（由 EditorFragment 调用）
     */
    fun setVisualTransactionProvider(provider: com.xiwei.sujian.ui.VisualTransactionProvider) {
        visualTransactionProvider = provider
    }

    /**
     * 视觉事务编辑包装器。
     *
     * 步骤：
     * 1. 捕获编辑前快照（oldText/oldSelection/oldCursorRect）
     * 2. 设置 insideVisualEdit=true，执行 edit() lambda
     * 3. 捕获编辑后快照（newText/newSelection/newCursorRect）
     * 4. 构造完整 SujianVisualEditContext 并分发动画
     *
     * insideVisualEdit 期间，buffer.onTextChanged 只做内容通知，不移动光标，
     * 光标和动画统一由本方法 → handleVisualEdit 收口。
     *
     * @param cause 编辑原因
     * @param edit 实际编辑操作（修改 buffer）
     * @return edit lambda 的返回值
     */
    fun <T> runVisualEdit(cause: SujianEditCauseData, edit: () -> T): T {
        // 步骤 1：捕获编辑前快照
        val oldText = buffer.text
        val oldSelection = buffer.selection  // SujianSelection(anchor, head)

        val oldCursorRect: SujianCursorRectData? = if (cause == SujianEditCauseData.Delete) {
            preDeleteOldCursorRect?.also { preDeleteOldCursorRect = null }
        } else {
            if (oldText.isNotEmpty()) {
                val cr = layoutEngine.getCursorRect(oldText, oldSelection.head)
                SujianCursorRectData(cr.x.toDouble(), cr.top.toDouble(), cr.bottom.toDouble(), cr.baselineY.toDouble())
            } else {
                null
            }
        }

        // 步骤 2：执行编辑
        insideVisualEdit = true
        try {
            val result = edit()

            // 步骤 3：捕获编辑后快照
            val newText = buffer.text
            val newSelection = buffer.selection

            val newCursorRect: SujianCursorRectData? = try {
                if (newText.isNotEmpty()) {
                    val cr = layoutEngine.getCursorRect(newText, newSelection.head)
                    SujianCursorRectData(cr.x.toDouble(), cr.top.toDouble(), cr.bottom.toDouble(), cr.baselineY.toDouble())
                } else {
                    null
                }
            } catch (e: Exception) {
                DiagnosticsLogger.d(TAG, "Failed to capture newCursorRect: ${e.message}")
                null
            }

            // 步骤 4：构造完整快照并分发动画
            val context = SujianVisualEditContext(
                oldText = oldText,
                newText = newText,
                oldSelectionAnchor = oldSelection.anchor,
                oldSelectionHead = oldSelection.head,
                newSelectionAnchor = newSelection.anchor,
                newSelectionHead = newSelection.head,
                oldCursorRect = oldCursorRect,
                newCursorRect = newCursorRect,
                cause = cause
            )
            animationController.handleVisualEdit(context, this)

            return result
        } finally {
            insideVisualEdit = false
        }
    }

    // ── 公共 API ──

    /**
     * 获取当前文本内容
     */
    fun getText(): String = buffer.text

    /**
     * 设置文本内容（加载章节），不触发动画
     */
    fun setText(text: String) {
        buffer.loadText(text)
        layoutEngine.invalidate()
        cursorController.onChapterLoaded()
        touchController.scrollTo(0, 0)
        invalidate()
    }

    /**
     * 设置字号
     */
    fun setFontSize(sizeSp: Float) {
        val sizePx = sizeSp * resources.displayMetrics.scaledDensity
        if (lastFontSize != sizePx) {
            lastFontSize = sizePx
            // 字号变化时：清动画、清 hidden range、snap 光标
            animationController.tick() // 先 tick 确保动画状态一致
            renderer.clearAnimations()
            renderer.setAnimatedInsertRange(null)
            textPaint.textSize = sizePx
            // 重算首行缩进像素值（字号变化后字符宽度变了）
            if (autoIndentEnabled && autoIndentWidthChars > 0) {
                val newIndentPx = textPaint.measureText("中") * autoIndentWidthChars
                lastFirstLineIndentPx = newIndentPx
            }
            // 更新 layout 参数
            layoutEngine.updateParams(
                width = width - paddingLeft - paddingRight,
                spacingMultiplier = lastLineSpacingMultiplier,
                spacingExtra = 0f,
                firstLineIndentPx = lastFirstLineIndentPx
            )
            cursorController.onFontSizeChanged()
            // 重算光标位置并 snap
            val cursorRect = layoutEngine.getCursorRect(buffer.text, buffer.selection.head)
            cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, false)
            invalidate()
        }
    }

    /**
     * 设置行距倍数
     */
    fun setLineSpacingMultiplier(multiplier: Float) {
        if (lastLineSpacingMultiplier != multiplier) {
            lastLineSpacingMultiplier = multiplier
            layoutEngine.updateParams(
                width = width - paddingLeft - paddingRight,
                spacingMultiplier = multiplier,
                spacingExtra = 0f,
                firstLineIndentPx = lastFirstLineIndentPx
            )
            // 重算光标位置并 snap
            val cursorRect = layoutEngine.getCursorRect(buffer.text, buffer.selection.head)
            cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, false)
            invalidate()
        }
    }

    /**
     * 设置首行缩进（像素）
     */
    fun setFirstLineIndentPx(px: Float) {
        if (lastFirstLineIndentPx != px) {
            lastFirstLineIndentPx = px
            layoutEngine.updateParams(
                width = width - paddingLeft - paddingRight,
                spacingMultiplier = lastLineSpacingMultiplier,
                spacingExtra = 0f,
                firstLineIndentPx = px
            )
            // 重算光标位置并 snap
            val cursorRect = layoutEngine.getCursorRect(buffer.text, buffer.selection.head)
            cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, false)
            invalidate()
        }
    }

    /**
     * 设置打字动画启用/禁用
     */
    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 160L) {
        val wasEnabled = animationController.animationEnabled
        animationController.animationEnabled = enabled
        animationController.animationDurationMs = durationMs
        buffer.maxAnimatedChars = 8
        buffer.animationDurationMs = durationMs
        // 关闭 typingAnimation 时清除 hidden range 和动画
        if (!enabled && wasEnabled) {
            renderer.clearAnimations()
            renderer.setAnimatedInsertRange(null)
        }
    }

    /**
     * 设置平滑光标启用/禁用
     */
    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        cursorController.setSmoothCursorEnabled(enabled, durationMs)
        renderer.smoothCursorEnabled = enabled
        // 关闭 smoothCursor 时恢复静态光标位置
        if (!enabled) {
            val cursorRect = layoutEngine.getCursorRect(buffer.text, buffer.selection.head)
            cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, false)
        }
    }

    /**
     * 设置自动缩进
     */
    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        autoIndentEnabled = enabled
        autoIndentWidthChars = widthChars
        val indentPx = if (enabled && widthChars > 0) {
            textPaint.measureText("中") * widthChars
        } else {
            0f
        }
        setFirstLineIndentPx(indentPx)
    }

    /**
     * 设置光标配合文字吞吐动画
     */
    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        animationController.coordinatedAnimationEnabled = enabled
    }

    /**
     * 获取光标位置（UTF-16 offset）
     */
    fun getSelectionStart(): Int = buffer.selection.start

    /**
     * 获取选区结束位置（UTF-16 offset）
     */
    fun getSelectionEnd(): Int = buffer.selection.end

    // ── View 生命周期 ──

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        layoutEngine.updateParams(
            width = w - paddingLeft - paddingRight,
            spacingMultiplier = lastLineSpacingMultiplier,
            spacingExtra = 0f,
            firstLineIndentPx = lastFirstLineIndentPx
        )
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        val text = buffer.text
        if (text.isEmpty() && !buffer.hasComposing) {
            // 空文本时只画光标（固定 1.5dp 宽度）
            if (cursorController.isCursorVisible) {
                val cursorWidth = 1.5f * resources.displayMetrics.density
                canvas.drawRect(
                    paddingLeft.toFloat(),
                    paddingTop.toFloat(),
                    paddingLeft + cursorWidth,
                    paddingTop + textPaint.textSize,
                    renderer.cursorPaint
                )
            }
            return
        }

        // 获取布局
        val staticLayout = layoutEngine.getLayout(text)

        // Tick 动画
        animationController.tick()

        // 绘制
        canvas.save()
        canvas.translate(paddingLeft.toFloat(), paddingTop.toFloat())

        renderer.draw(
            canvas = canvas,
            layout = staticLayout,
            text = text,
            scrollX = touchController.scrollX,
            scrollY = touchController.scrollY,
            selection = buffer.selection,
            composingStart = buffer.composingStart,
            composingEnd = buffer.composingEnd,
            viewportWidth = width - paddingLeft - paddingRight,
            viewportHeight = height - paddingTop - paddingBottom
        )

        canvas.restore()

        // 如果有活跃动画，继续重绘
        if (animationController.hasActiveAnimations()) {
            invalidate()
        }
    }

    // ── 方向键处理 ──

    /**
     * 获取 text 中 offset 前一个 grapheme boundary 的位置。
     * 使用 android.icu.text.BreakIterator.getCharacterInstance() 确保正确处理
     * surrogate pair、combining mark、ZWJ emoji 等复杂 grapheme。
     */
    fun prevGraphemeBoundary(text: String, offset: Int): Int {
        if (offset <= 0) return 0
        val bi = android.icu.text.BreakIterator.getCharacterInstance()
        bi.setText(text)
        return bi.preceding(offset.coerceIn(0, text.length)).coerceAtLeast(0)
    }

    /**
     * 获取 text 中 offset 后一个 grapheme boundary 的位置。
     * 使用 android.icu.text.BreakIterator.getCharacterInstance() 确保正确处理
     * surrogate pair、combining mark、ZWJ emoji 等复杂 grapheme。
     */
    fun nextGraphemeBoundary(text: String, offset: Int): Int {
        if (offset >= text.length) return text.length
        val bi = android.icu.text.BreakIterator.getCharacterInstance()
        bi.setText(text)
        return bi.following(offset.coerceIn(0, text.length)).coerceAtMost(text.length)
    }

    /**
     * 方向键移动后更新光标位置和选区。
     * 不触发 runVisualEdit（方向键不改变 buffer 内容）。
     */
    private fun updateCursorAfterMove(newOffset: Int, extendSelection: Boolean) {
        val clampedOffset = newOffset.coerceIn(0, buffer.text.length)
        if (extendSelection) {
            // Shift+方向键：扩展选区，anchor 不变，head 移动
            buffer.setSelection(buffer.selection.anchor, clampedOffset)
        } else {
            // 无 Shift：如果有选区，先折叠到 head 方向
            if (!buffer.selection.isCollapsed) {
                // 折叠到移动方向（左/上折叠到 start，右/下折叠到 end）
                // 但这里统一折叠到 newOffset
                buffer.setSelection(clampedOffset, clampedOffset)
            } else {
                buffer.setSelection(clampedOffset, clampedOffset)
            }
        }
        // 更新光标视觉位置
        val text = buffer.text
        val cursorRect = layoutEngine.getCursorRect(text, buffer.selection.head)
        cursorController.requestForceSnap()
        cursorController.updateCursorTarget(cursorRect.x, cursorRect.top, cursorRect.bottom, false)
        cursorController.onSelectionChanged()
        touchController.ensureCursorVisible()
        imeController.updateSelection()
        invalidate()
    }

    private fun handleDpadLeft(extendSelection: Boolean) {
        val text = buffer.text
        val currentHead = buffer.selection.head

        // 无 Shift 且有选区时：折叠到选区 start
        if (!extendSelection && !buffer.selection.isCollapsed) {
            updateCursorAfterMove(buffer.selection.start, false)
            return
        }

        val newOffset = prevGraphemeBoundary(text, currentHead)
        // 清除上下移动的 X 记忆
        rememberedCursorX = null
        updateCursorAfterMove(newOffset, extendSelection)
    }

    private fun handleDpadRight(extendSelection: Boolean) {
        val text = buffer.text
        val currentHead = buffer.selection.head

        // 无 Shift 且有选区时：折叠到选区 end
        if (!extendSelection && !buffer.selection.isCollapsed) {
            updateCursorAfterMove(buffer.selection.end, false)
            return
        }

        val newOffset = nextGraphemeBoundary(text, currentHead)
        // 清除上下移动的 X 记忆
        rememberedCursorX = null
        updateCursorAfterMove(newOffset, extendSelection)
    }

    private fun handleDpadUp(extendSelection: Boolean) {
        val text = buffer.text
        if (text.isEmpty()) return

        val currentHead = buffer.selection.head

        // 无 Shift 且有选区时：折叠到选区 start 所在行
        if (!extendSelection && !buffer.selection.isCollapsed) {
            val startLine = layoutEngine.getLineForOffset(text, buffer.selection.start)
            val currentLine = layoutEngine.getLineForOffset(text, currentHead)
            if (startLine < currentLine) {
                // 选区跨越多行，折叠到选区 start
                updateCursorAfterMove(buffer.selection.start, false)
                return
            }
        }

        val currentLine = layoutEngine.getLineForOffset(text, currentHead)
        if (currentLine <= 0) {
            // 已在第一行，移动到文本开头
            rememberedCursorX = null
            updateCursorAfterMove(0, extendSelection)
            return
        }

        // 记忆或使用 X 坐标
        val targetX = rememberedCursorX ?: layoutEngine.getCursorX(text, currentHead)
        rememberedCursorX = targetX

        val targetLine = currentLine - 1
        val newOffset = layoutEngine.getOffsetForHorizontal(text, targetLine, targetX)
        updateCursorAfterMove(newOffset, extendSelection)
    }

    private fun handleDpadDown(extendSelection: Boolean) {
        val text = buffer.text
        if (text.isEmpty()) return

        val currentHead = buffer.selection.head

        // 无 Shift 且有选区时：折叠到选区 end 所在行
        if (!extendSelection && !buffer.selection.isCollapsed) {
            val endLine = layoutEngine.getLineForOffset(text, buffer.selection.end)
            val currentLine = layoutEngine.getLineForOffset(text, currentHead)
            val lastLine = layoutEngine.getLineCount(text) - 1
            if (endLine > currentLine) {
                // 选区跨越多行，折叠到选区 end
                updateCursorAfterMove(buffer.selection.end, false)
                return
            }
        }

        val currentLine = layoutEngine.getLineForOffset(text, currentHead)
        val lastLine = layoutEngine.getLineCount(text) - 1
        if (currentLine >= lastLine) {
            // 已在最后一行，移动到文本末尾
            rememberedCursorX = null
            updateCursorAfterMove(text.length, extendSelection)
            return
        }

        // 记忆或使用 X 坐标
        val targetX = rememberedCursorX ?: layoutEngine.getCursorX(text, currentHead)
        rememberedCursorX = targetX

        val targetLine = currentLine + 1
        val newOffset = layoutEngine.getOffsetForHorizontal(text, targetLine, targetX)
        updateCursorAfterMove(newOffset, extendSelection)
    }

    // ── IME ──

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or
                android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_FULLSCREEN or
                android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI
        outAttrs.initialSelStart = buffer.selection.start
        outAttrs.initialSelEnd = buffer.selection.end
        outAttrs.initialCapsMode = 0

        return SujianInputConnection(this, buffer, imeController)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> {
                if (buffer.selection.isCollapsed) {
                    imeController.onBeforeDelete(1, 0)
                    runVisualEdit(SujianEditCauseData.Delete) {
                        val result = buffer.deleteSurrounding(1, 0)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                } else {
                    imeController.onBeforeDeleteSelection()
                    runVisualEdit(SujianEditCauseData.Delete) {
                        val result = buffer.commitText("", SujianEditCause.Delete)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                if (buffer.selection.isCollapsed) {
                    imeController.onBeforeDelete(0, 1)
                    runVisualEdit(SujianEditCauseData.Delete) {
                        val result = buffer.deleteSurrounding(0, 1)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                } else {
                    imeController.onBeforeDeleteSelection()
                    runVisualEdit(SujianEditCauseData.Delete) {
                        val result = buffer.commitText("", SujianEditCause.Delete)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_ENTER -> {
                runVisualEdit(SujianEditCauseData.Typing) {
                    val result = buffer.commitText("\n", SujianEditCause.Typing)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                }
                return true
            }
            KeyEvent.KEYCODE_DPAD_LEFT -> {
                handleDpadLeft(event.isShiftPressed)
                return true
            }
            KeyEvent.KEYCODE_DPAD_RIGHT -> {
                handleDpadRight(event.isShiftPressed)
                return true
            }
            KeyEvent.KEYCODE_DPAD_UP -> {
                handleDpadUp(event.isShiftPressed)
                return true
            }
            KeyEvent.KEYCODE_DPAD_DOWN -> {
                handleDpadDown(event.isShiftPressed)
                return true
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    // ── 触摸 ──

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val handled = touchController.onTouchEvent(event)
        if (handled) {
            requestFocus()
        }
        return handled || super.onTouchEvent(event)
    }

    // ── 焦点 ──

    override fun onFocusChanged(gained: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gained, direction, previouslyFocusedRect)
        cursorController.onFocusChanged(gained)
        if (gained) {
            imeController.showSoftInput()
        }
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (hasWindowFocus && isFocused) {
            cursorController.onFocusChanged(true)
        }
    }

    // ── 生命周期 ──

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        cursorController.onDetachedFromWindow()
        animationController.onDetachedFromWindow()
        touchController.onDetachedFromWindow()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        applyThemeColors()
        if (isFocused) {
            cursorController.onFocusChanged(true)
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration?) {
        super.onConfigurationChanged(newConfig)
        applyThemeColors()
    }

    /**
     * 从主题 attr 注入颜色到渲染器，避免硬编码颜色。
     * 必须在 attach 到 window 后调用（需要 context 取 theme attr）。
     */
    fun applyThemeColors() {
        val textColor = MaterialColors.getColor(
            context,
            com.google.android.material.R.attr.colorOnSurface,
            0
        )
        val primaryColor = MaterialColors.getColor(
            context,
            com.google.android.material.R.attr.colorPrimary,
            0
        )
        val surfaceColor = MaterialColors.getColor(
            context,
            com.google.android.material.R.attr.colorSurfaceContainerLowest,
            MaterialColors.getColor(
                context,
                com.google.android.material.R.attr.colorSurface,
                0
            )
        )

        // 编辑器背景
        setBackgroundColor(surfaceColor)

        // 正文颜色
        textPaint.color = textColor

        // 光标颜色 → colorPrimary
        val cursorColor = primaryColor

        // composing 下划线颜色 → colorPrimary 叠 alpha 180/255
        val composingColor = (primaryColor and 0x00FFFFFF) or (180 shl 24)

        // 选区高亮颜色 → colorPrimary 叠 alpha 60/255
        val selectionColor = (primaryColor and 0x00FFFFFF) or (60 shl 24)

        // 传递给渲染器（含 animTextPaint 同步）
        renderer.setThemeColors(textColor, cursorColor, composingColor, selectionColor)

        invalidate()
        layoutEngine.invalidate()
    }

    // ── Accessibility ──

    override fun onInitializeAccessibilityEvent(event: AccessibilityEvent) {
        super.onInitializeAccessibilityEvent(event)
        event.className = SujianEditorView::class.java.name
        event.text.add(buffer.text)
    }

    override fun performAccessibilityAction(action: Int, arguments: android.os.Bundle?): Boolean {
        when (action) {
            android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT -> {
                val text = arguments?.getCharSequence(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE)?.toString()
                if (text != null) {
                    setText(text)
                    return true
                }
            }
        }
        return super.performAccessibilityAction(action, arguments)
    }
}
