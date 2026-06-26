package com.xiwei.sujian.ui

import android.text.Editable
import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.model.EditorAnimationEventData
import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.ui.UtfOffsetConverter
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * TypingAnimationController — 打字动画控制器
 *
 * 监听 EditText 的文本变化，通过 Core EditorEngine 判断是否播放动画，
 * 然后用 Android Layout 算坐标，提交到 EditorRenderLayer。
 *
 * ## 架构定位
 * - Core 负责 `should_animate` 和事件语义（Insert/Delete/Cursor）
 * - Android 负责坐标计算和渲染（Layout 算 glyph/caret 坐标，EditorRenderLayer 画动画）
 * - 流程：TextWatcher → 构造 oldText/newText/selection/cause → 调 Core 得 animation_events → Android 用 Layout 算坐标 → EditorRenderLayer 画动画
 *
 * ## 职责边界
 * - **做**：文本变化监听、调用 Core 判断动画、坐标计算、提交动画到 renderLayer
 * - **不做**：自己判断 count<=3、自己检测 composing、自己生成 AndroidEditorAnimationEvent
 * - **禁止**：向正文 Editable 注入透明 ForegroundColorSpan 隐藏文字
 *
 * ## 动画裁判硬边界
 * - Core provider 存在时，Core 是唯一裁判，本地 fallback 不运行
 * - Core provider 不存在时，本地 fallback 才允许运行
 * - Core 调用失败时，不 fallback 动画，只跳过动画并打日志
 *
 * ## 使用场景
 * - 用户输入/删除字符时通过 Core 判断是否播放逐字动画
 * - Core 返回空事件列表时跳过动画（粘贴/大段替换/加载等场景）
 */
class TypingAnimationController(
    private val editText: WriterEditText,
    private val renderLayer: EditorRenderLayer
) {
    private val TAG = "WriterEditorAnim"

    /**
     * Core 动画事件提供者。调用 Core 的 `editorAnimationEvents` 方法，
     * 传入 oldText/newText/oldCursorIndex/newCursorIndex/cause/maxAnimatedChars/animationDurationMs，
     * 返回 Core 计算出的 typed 动画事件列表。
     *
     * 如果为 null，则回退到本地判断逻辑（兼容未初始化 Core 的场景）。
     * 通过 `setAnimationEventProvider()` 注入。
     */
    private var _animationEventProvider: AnimationEventProvider? = null
    val hasProvider: Boolean get() = _animationEventProvider != null

    var providerUnavailable: Boolean = false
    var providerFailedLastTime: Boolean = false

    var typingAnimationEnabled = false
        private set
    var typingAnimationDurationMs: Long = 100L
        private set

    var isSuppressAnimations = false
    var isScrollAnimationsSuppressed = false
        set(value) {
            field = value
            if (value) {
                lastAddedStart = -1
                lastAddedCount = 0
                clearPendingDelete()
                renderLayer.clear()
            }
        }

    private var lastAddedStart = -1
    private var lastAddedCount = 0
    private var cursorBeforeX = -1f
    private var cursorBeforeY = -1f

    var lastEditorAnimationEvent: AndroidEditorAnimationEvent? = null
        private set

    // 保存 beforeTextChanged 时的旧文本和光标位置，供 afterTextChanged 使用
    private var oldTextBeforeChange: String = ""
    private var oldCursorIndexBeforeChange: Int = 0  // UTF-8 byte offset
    private var oldCursorIndexBeforeChangeUtf16: Int = 0  // UTF-16 code unit offset，供本地 Layout 使用

    // 删除动画待提交信息：在 beforeTextChanged 记录起始位置，在 afterTextChanged 提交完整动画
    private var pendingDeleteStartX = -1f
    private var pendingDeleteStartY = -1f
    private var pendingDeleteText = ""
    private var pendingDeleteStart = -1

    private fun clearPendingDelete() {
        pendingDeleteStart = -1
        pendingDeleteText = ""
        pendingDeleteStartX = -1f
        pendingDeleteStartY = -1f
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs.coerceIn(80L, 180L)
        val providerAvailable = _animationEventProvider != null
        val actualPath = when {
            !enabled -> "disabled"
            providerAvailable && !providerFailedLastTime -> "core"
            providerAvailable && providerFailedLastTime -> "core(failed,skip)"
            else -> "no-provider(disabled)"
        }
        DiagnosticsLogger.d(TAG, "setTypingAnimationEnabled: enabled=$enabled, durationMs=${typingAnimationDurationMs}, providerAvailable=$providerAvailable, providerFailed=$providerFailedLastTime, actualPath=$actualPath")
        if (!enabled) {
            clearPendingDelete()
            renderLayer.clear()
        }
        // 如果 provider 不存在且 enabled=true，记录警告
        if (enabled && _animationEventProvider == null) {
            com.xiwei.sujian.diagnostics.DiagnosticsLogger.w(
                TAG,
                "setTypingAnimationEnabled: enabled=true but no provider injected. " +
                "Animation will be disabled until provider is set."
            )
        }
    }

    /**
     * 设置 Core 动画事件提供者。
     *
     * 调用此方法后，TypingAnimationController 将通过 Core 的 EditorEngine
     * 判断 should_animate 和事件语义，而非本地 count<=3 判断。
     */
    fun setAnimationEventProvider(provider: AnimationEventProvider) {
        _animationEventProvider = provider
        providerUnavailable = false
        providerFailedLastTime = false
    }

    fun recordCursorBeforeChange(x: Float, y: Float) {
        cursorBeforeX = x
        cursorBeforeY = y
    }

    fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
        if (isScrollAnimationsSuppressed) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }
        if (isSuppressAnimations) {
            clearPendingDelete()
            renderLayer.clear()
            return
        }

        DiagnosticsLogger.d(TAG, "beforeTextChanged: start=$start, count=$count, after=$after")

        oldTextBeforeChange = s?.toString() ?: ""
        val oldCursorIndexUtf16 = editText.selectionStart.coerceAtLeast(0)
        oldCursorIndexBeforeChange = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(s?.toString() ?: "", oldCursorIndexUtf16)
        oldCursorIndexBeforeChangeUtf16 = oldCursorIndexUtf16

        if ((after > 0 || count > 0) && editText.layout != null) {
            val line = editText.layout.getLineForOffset(start)
            cursorBeforeX = editText.layout.getPrimaryHorizontal(start)
            cursorBeforeY = editText.layout.getLineBaseline(line).toFloat()
        }

        // 对于删除操作，在 beforeTextChanged 中记录被删文本的坐标
        // （此时 layout 还未更新，可以拿到被删字符的精确位置）
        if (count > 0 && after == 0 && s != null && typingAnimationEnabled && editText.layout != null) {
            val deletedText = s.subSequence(start, start + count).toString()
            if (!deletedText.contains('\n') && !deletedText.contains('\r')) {
                pendingDeleteStartX = editText.layout.getPrimaryHorizontal(start)
                pendingDeleteStartY = editText.layout.getLineBaseline(editText.layout.getLineForOffset(start)).toFloat()
                pendingDeleteText = deletedText
                pendingDeleteStart = start
            }
        }

        DiagnosticsLogger.d(TAG, "beforeTextChanged: cursor=($cursorBeforeX, $cursorBeforeY)")
    }

    fun onTextChanged(start: Int, count: Int) {
        if (isSuppressAnimations || isScrollAnimationsSuppressed) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }

        // 只记录位置，判断逻辑交给 afterTextChanged 中的 Core
        lastAddedStart = start
        lastAddedCount = count
    }

    fun afterTextChanged(editable: Editable?) {
        if (editable == null) return
        if (isSuppressAnimations || isScrollAnimationsSuppressed) {
            clearPendingDelete()
            return
        }

        DiagnosticsLogger.d(TAG, "afterTextChanged: lastStart=$lastAddedStart, lastCount=$lastAddedCount, typingEnabled=$typingAnimationEnabled")

        val newText = editable.toString()
        val newCursorIndexUtf16 = editText.selectionStart.coerceAtLeast(0)
        val newCursorIndexByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorIndexUtf16)

        // ── Core 驱动路径 ──
        // 如果有 animationEventProvider，Core 是唯一裁判
        if (_animationEventProvider != null && typingAnimationEnabled && editText.layout != null) {
            val cause = determineCause(oldTextBeforeChange, newText, lastAddedStart, lastAddedCount)

            try {
                val events = _animationEventProvider!!.provide(
                    oldText = oldTextBeforeChange,
                    newText = newText,
                    oldCursorIndex = oldCursorIndexBeforeChange.toUInt(),  // UTF-8 byte offset
                    newCursorIndex = newCursorIndexByte.toUInt(),           // UTF-8 byte offset
                    cause = cause,
                    maxAnimatedChars = MAX_ANIMATED_CHARS.toUInt(),
                    animationDurationMs = typingAnimationDurationMs.toULong()
                )

                if (events.isEmpty()) {
                    clearPendingDelete()
                    lastAddedStart = -1
                    lastAddedCount = 0
                    return
                }

                // 遍历 Core 返回的事件，用 Android Layout 算坐标，提交到 renderLayer
                for (event in events) {
                    when (event.kind) {
                        EditorAnimationKindData.Insert -> {
                            val rangeStartUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, event.rangeStart)
                            lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                                kind = "insert",
                                start = rangeStartUtf16,
                                text = event.text,
                                durationMs = event.durationMs
                            )
                            val destX = editText.layout.getPrimaryHorizontal(rangeStartUtf16)
                            val line = editText.layout.getLineForOffset(rangeStartUtf16)
                            val destY = editText.layout.getLineBaseline(line).toFloat()
                            renderLayer.addTypingAnim(OverlayAnim(
                                insertedStart = rangeStartUtf16,
                                insertedText = event.text,
                                startX = cursorBeforeX,
                                startY = cursorBeforeY,
                                endX = destX,
                                endY = destY,
                                durationMs = event.durationMs,
                                isDeletion = false
                            ))
                        }
                        EditorAnimationKindData.Delete -> {
                            val rangeStartUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, event.rangeStart)
                            lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                                kind = "delete",
                                start = rangeStartUtf16,
                                text = event.text,
                                durationMs = event.durationMs
                            )
                            // 删除动画：起始位置是被删字符位置（在 beforeTextChanged 中记录），
                            // 目标位置是删除后的光标位置
                            if (pendingDeleteStart >= 0) {
                                val destX = editText.layout.getPrimaryHorizontal(newCursorIndexUtf16)
                                val destLine = editText.layout.getLineForOffset(newCursorIndexUtf16)
                                val destY = editText.layout.getLineBaseline(destLine).toFloat()
                                renderLayer.addTypingAnim(OverlayAnim(
                                    insertedStart = pendingDeleteStart,
                                    insertedText = event.text,
                                    startX = pendingDeleteStartX,
                                    startY = pendingDeleteStartY,
                                    endX = destX,
                                    endY = destY,
                                    durationMs = event.durationMs,
                                    isDeletion = true
                                ))
                            }
                        }
                        EditorAnimationKindData.Cursor -> {
                            // Cursor 动画由 SmoothCursorRenderer 处理，此处不额外提交
                            // Core 返回 cursor 事件仅作为语义标记
                        }
                    }
                }

                clearPendingDelete()
                lastAddedStart = -1
                lastAddedCount = 0
                return
            } catch (e: Exception) {
                providerFailedLastTime = true
                DiagnosticsLogger.w(TAG, "Core animation events failed: typingEnabled=$typingAnimationEnabled, providerInjected=true, providerFailed=true, skipping animation", e)
                clearPendingDelete()
                lastAddedStart = -1
                lastAddedCount = 0
                return
            }
        }

        // ── 无 Core provider 路径 ──
        // Core provider 为 null：正式编辑页不应走到这里。
        // 如果走到，说明 provider 未注入，typing animation 应禁用。
        // 记录 diagnostics，不执行本地 fallback 动画。
        if (typingAnimationEnabled && _animationEventProvider == null) {
            com.xiwei.sujian.diagnostics.DiagnosticsLogger.w(
                TAG,
                "afterTextChanged: provider is null, typing animation disabled. " +
                "EditorFragment must inject AnimationEventProvider before enabling typing animation."
            )
        }
        clearPendingDelete()
        lastAddedStart = -1
        lastAddedCount = 0
    }

    fun onDetachedFromWindow() {
        clearPendingDelete()
        renderLayer.clear()
    }

    // ── 内部辅助 ──

    /**
     * 根据文本变化推断 EditorTransactionCause。
     *
     * 这个推断逻辑与 Core 端的 `should_animate` 配合：
     * - 只有 Typing 和 Delete 会触发文本动画
     * - Paste/Load/Format 等不会触发
     *
     * Android 端无法 100% 精确判断 cause（比如区分 Typing vs ImeComposition），
     * 但 Core 端的 `should_animate_changes` 会做最终判断，所以这里只需要
     * 大致分类即可。
     */
    private fun determineCause(oldText: String, newText: String, changeStart: Int, changeCount: Int): String {
        val oldLen = oldText.length
        val newLen = newText.length

        return when {
            // 文本变长 → 可能是插入
            newLen > oldLen -> {
                val diff = newLen - oldLen
                // 大段增长 → 粘贴
                if (diff > MAX_ANIMATED_CHARS) "Paste"
                // 有 composing span → IME 组合
                else {
                    val editable = editText.text
                    if (editable != null) {
                        val compStart = BaseInputConnection.getComposingSpanStart(editable)
                        val compEnd = BaseInputConnection.getComposingSpanEnd(editable)
                        if (compStart != -1 && compEnd != -1 && changeStart >= compStart && changeStart + changeCount <= compEnd) {
                            "ImeComposition"
                        } else {
                            "Typing"
                        }
                    } else {
                        "Typing"
                    }
                }
            }
            // 文本变短 → 删除
            newLen < oldLen -> "Delete"
            // 长度不变 → 替换（如 IME 组合替换）
            else -> "ImeComposition"
        }
    }

    companion object {
        /** 与 Core EditorEngine 默认 max_animated_chars 对齐 */
        private const val MAX_ANIMATED_CHARS = 8
    }
}

/**
 * Core 动画事件提供者接口。
 *
 * Android 端通过此接口调用 Core 的 `editorAnimationEvents` 方法。
 * 实现类通常包装 `uniffi.writer_core.WriterAppService.editorAnimationEvents()`。
 */
fun interface AnimationEventProvider {
    /**
     * 调用 Core 的 EditorEngine 计算动画事件。
     *
     * @param oldText 变化前的文本
     * @param newText 变化后的文本
     * @param oldCursorIndex 变化前的光标位置（UTF-8 byte offset，调用方需从 UTF-16 转换）
     * @param newCursorIndex 变化后的光标位置（UTF-8 byte offset，调用方需从 UTF-16 转换）
     * @param cause 变化原因（"Typing", "Delete", "ImeComposition", "Paste", "Load" 等）
     * @param maxAnimatedChars 最大动画字符数
     * @param animationDurationMs 动画时长（毫秒）
     * @return Core 返回的 typed 动画事件列表
     */
    fun provide(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): List<EditorAnimationEventData>
}

data class AndroidEditorAnimationEvent(
    val kind: String,
    val start: Int,
    val text: String,
    val durationMs: Long
)
