package com.xiwei.sujian.editor.selfrender

import android.content.Context
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

/**
 * SujianEditorView — Android 自研写作区核心 View
 *
 * 继承 View（不继承 EditText），整合所有控制器。
 * 只替换写作正文区域，不重写整个 Android UI。
 *
 * ## 架构原则
 * - Core 仍唯一业务语义来源
 * - Android 只负责输入适配、布局、绘制、触摸、IME
 * - 文本变化必须生成 EditorTransaction
 * - 动画事件继续复用 Core EditorAnimationEvent
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
    val renderer = SujianEditorRenderer(textPaint)
    val animationController = SujianAnimationController(buffer, layoutEngine, renderer)
    val imeController = SujianImeController(this, buffer, layoutEngine, renderer, animationController)
    val cursorController = SujianCursorController(this, buffer, renderer)
    val selectionController = SujianSelectionController(buffer, layoutEngine)
    val touchController = SujianTouchController(this, buffer, layoutEngine, selectionController, cursorController, animationController)
    val clipboardController = SujianClipboardController(context, buffer, animationController, layoutEngine)

    // ── 设置缓存 ──
    private var lastFontSize: Float = textPaint.textSize
    private var lastLineSpacingMultiplier: Float = 1.0f
    private var lastFirstLineIndentPx: Float = 0f

    // ── 内容变更监听 ──
    var onContentChanged: ((String) -> Unit)? = null

    // ── 初始化 ──
    init {
        // Buffer 变更监听
        buffer.onTextChanged = { result ->
            // 通知布局引擎文本已变
            layoutEngine.invalidate()
            // 通知动画控制器处理事件
            animationController.handleAnimationEvents(result.animationEvents, result.cause)
            // 确保光标可见
            touchController.ensureCursorVisible()
            // 通知外部内容变更
            onContentChanged?.invoke(result.newText)
            // 重绘
            invalidate()
        }

        // 设置动画事件提供者桥接
        // SujianAnimationEventProvider 使用 Int/Long，
        // AnimationEventProvider 使用 UInt/ULong，
        // 这里做类型转换桥接
        buffer.setAnimationEventProvider(object : SujianAnimationEventProvider {
            override fun provide(
                oldText: String,
                newText: String,
                oldCursorUtf8: Int,
                newCursorUtf8: Int,
                cause: String,
                maxAnimatedChars: Int,
                animationDurationMs: Long
            ): List<com.xiwei.sujian.model.EditorAnimationEventData> {
                val provider = animationEventProvider ?: return emptyList()
                return provider.provide(
                    oldText,
                    newText,
                    oldCursorUtf8.toUInt(),
                    newCursorUtf8.toUInt(),
                    cause,
                    maxAnimatedChars.toUInt(),
                    animationDurationMs.toULong()
                )
            }
        })

        // 可聚焦，可获取输入
        isFocusable = true
        isFocusableInTouchMode = true
        isClickable = true
        isLongClickable = true
    }

    // ── 外部注入的动画事件提供者 ──
    private var animationEventProvider: com.xiwei.sujian.ui.AnimationEventProvider? = null

    /**
     * 注入 Core 动画事件提供者（由 EditorFragment 调用）
     */
    fun setAnimationEventProvider(provider: com.xiwei.sujian.ui.AnimationEventProvider) {
        animationEventProvider = provider
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
            textPaint.textSize = sizePx
            layoutEngine.invalidate()
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
            invalidate()
        }
    }

    /**
     * 设置打字动画启用/禁用
     */
    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 160L) {
        animationController.animationEnabled = enabled
        animationController.animationDurationMs = durationMs
        buffer.maxAnimatedChars = 8
        buffer.animationDurationMs = durationMs
    }

    /**
     * 设置平滑光标启用/禁用
     */
    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        cursorController.setSmoothCursorEnabled(enabled, durationMs)
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
            // 空文本时只画光标
            if (cursorController.isCursorVisible) {
                canvas.drawRect(
                    paddingLeft.toFloat(),
                    paddingTop.toFloat(),
                    paddingLeft + 2.5f,
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
                    val result = buffer.deleteSurrounding(1, 0)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                } else {
                    imeController.onBeforeDeleteSelection()
                    val result = buffer.commitText("", SujianEditCause.Delete)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                if (buffer.selection.isCollapsed) {
                    imeController.onBeforeDelete(0, 1)
                    val result = buffer.deleteSurrounding(0, 1)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                } else {
                    imeController.onBeforeDeleteSelection()
                    val result = buffer.commitText("", SujianEditCause.Delete)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                }
                return true
            }
            KeyEvent.KEYCODE_ENTER -> {
                val result = buffer.commitText("\n", SujianEditCause.Typing)
                imeController.onEditResult(result)
                imeController.updateSelection()
                return true
            }
            KeyEvent.KEYCODE_DPAD_LEFT,
            KeyEvent.KEYCODE_DPAD_RIGHT,
            KeyEvent.KEYCODE_DPAD_UP,
            KeyEvent.KEYCODE_DPAD_DOWN -> {
                // TODO: 光标移动
                return super.onKeyDown(keyCode, event)
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
        if (isFocused) {
            cursorController.onFocusChanged(true)
        }
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
