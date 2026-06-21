package com.xiwei.sujian.ui

import android.text.Editable
import android.view.inputmethod.BaseInputConnection

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
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"

    /**
     * Core 动画事件提供者。调用 Core 的 `editorAnimationEvents` 方法，
     * 传入 oldText/newText/oldCursorIndex/newCursorIndex/cause/maxAnimatedChars/animationDurationMs，
     * 返回 Core 计算出的动画事件列表（JSON 字符串）。
     *
     * 如果为 null，则回退到本地判断逻辑（兼容未初始化 Core 的场景）。
     * 通过 `setAnimationEventProvider()` 注入。
     */
    private var _animationEventProvider: AnimationEventProvider? = null

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
    private var oldCursorIndexBeforeChange: Int = 0

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
        // Clamp duration to 80~180ms
        typingAnimationDurationMs = durationMs.coerceIn(80L, 180L)
        if (!enabled) {
            clearPendingDelete()
            renderLayer.clear()
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
            if (DEBUG_ANIM) {
                android.util.Log.d(TAG, "beforeTextChanged - suppressed animation")
            }
            return
        }

        // 保存旧文本和光标位置，供 afterTextChanged 调用 Core 时使用
        oldTextBeforeChange = s?.toString() ?: ""
        oldCursorIndexBeforeChange = editText.selectionStart.coerceAtLeast(0)

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

        if (DEBUG_ANIM) {
            android.util.Log.d(TAG, "beforeTextChanged - replaced: $count, after: $after, cursor: ($cursorBeforeX, $cursorBeforeY)")
        }
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

        val newText = editable.toString()
        val newCursorIndex = editText.selectionStart.coerceAtLeast(0)

        // ── Core 驱动路径 ──
        // 如果有 animationEventProvider，Core 是唯一裁判
        if (_animationEventProvider != null && typingAnimationEnabled && editText.layout != null) {
            val cause = determineCause(oldTextBeforeChange, newText, lastAddedStart, lastAddedCount)

            try {
                val eventsJson = _animationEventProvider!!.provide(
                    oldText = oldTextBeforeChange,
                    newText = newText,
                    oldCursorIndex = oldCursorIndexBeforeChange.toUInt(),
                    newCursorIndex = newCursorIndex.toUInt(),
                    cause = cause,
                    maxAnimatedChars = MAX_ANIMATED_CHARS.toUInt(),
                    animationDurationMs = typingAnimationDurationMs.toULong()
                )

                val events = parseAnimationEvents(eventsJson)
                if (events.isEmpty()) {
                    clearPendingDelete()
                    lastAddedStart = -1
                    lastAddedCount = 0
                    return
                }

                // 遍历 Core 返回的事件，用 Android Layout 算坐标，提交到 renderLayer
                for (event in events) {
                    when (event.kind) {
                        "insert" -> {
                            lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                                kind = "insert",
                                start = event.rangeStart,
                                text = event.text,
                                durationMs = event.durationMs
                            )
                            val destX = editText.layout.getPrimaryHorizontal(event.rangeStart)
                            val line = editText.layout.getLineForOffset(event.rangeStart)
                            val destY = editText.layout.getLineBaseline(line).toFloat()
                            renderLayer.addTypingAnim(OverlayAnim(
                                insertedStart = event.rangeStart,
                                insertedText = event.text,
                                startX = cursorBeforeX,
                                startY = cursorBeforeY,
                                endX = destX,
                                endY = destY,
                                durationMs = event.durationMs,
                                isDeletion = false
                            ))
                        }
                        "delete" -> {
                            lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                                kind = "delete",
                                start = event.rangeStart,
                                text = event.text,
                                durationMs = event.durationMs
                            )
                            // 删除动画：起始位置是被删字符位置（在 beforeTextChanged 中记录），
                            // 目标位置是删除后的光标位置
                            if (pendingDeleteStart >= 0) {
                                val destX = editText.layout.getPrimaryHorizontal(newCursorIndex)
                                val destLine = editText.layout.getLineForOffset(newCursorIndex)
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
                        "cursor" -> {
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
                // 硬边界：Core 调用失败时，不 fallback 到本地逻辑，只跳过动画并打日志
                android.util.Log.w(TAG, "Core animation events failed, skipping animation (no local fallback)", e)
                clearPendingDelete()
                lastAddedStart = -1
                lastAddedCount = 0
                return
            }
        }

        // ── 本地回退路径（临时降级，仅 Core 未初始化时使用）──
        // ⚠️ 边界约束：
        // 1. 仅当 animationEventProvider 为 null 时使用（Core 未初始化的场景）
        // 2. Core provider 存在时，上面已经 return，不会走到这里
        // 3. Core 调用失败时，上面也已 return（跳过动画），不会走到这里
        // 4. 正式编辑页初始化后必须注入 provider；如果没注入，typing animation 应禁用
        // 5. 此路径将在 Core 初始化流程稳定后移除
        // 6. 如果 provider 仍为 null 但 typingAnimationEnabled 为 true，说明初始化未完成，
        //    此路径仅作为过渡，不应长期依赖

        // 提交待处理的删除动画
        if (pendingDeleteStart >= 0 && typingAnimationEnabled && editText.layout != null) {
            val newCursorOffset = editText.selectionStart
            if (newCursorOffset >= 0) {
                val newLine = editText.layout.getLineForOffset(newCursorOffset)
                val destX = editText.layout.getPrimaryHorizontal(newCursorOffset)
                val destY = editText.layout.getLineBaseline(newLine).toFloat()
                renderLayer.addTypingAnim(OverlayAnim(
                    insertedStart = pendingDeleteStart,
                    insertedText = pendingDeleteText,
                    startX = pendingDeleteStartX,
                    startY = pendingDeleteStartY,
                    endX = destX,
                    endY = destY,
                    durationMs = typingAnimationDurationMs,
                    isDeletion = true
                ))
            }
            clearPendingDelete()
        }

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        if (isComposing && lastAddedStart >= composingStart && lastAddedStart + lastAddedCount <= composingEnd) {
            if (DEBUG_ANIM) android.util.Log.d(TAG, "afterTextChanged - skipping animation for composing text.")
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }

        if (lastAddedCount in 1..3 && lastAddedStart >= 0) {
            val start = lastAddedStart
            val end = kotlin.math.min(start + lastAddedCount, editable.length)

            if (end > start && typingAnimationDurationMs > 0) {
                val insertedText = editable.subSequence(start, end).toString()
                if (insertedText.contains('\n') || insertedText.contains('\r')) {
                    lastAddedStart = -1
                    lastAddedCount = 0
                    return
                }

                lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                    kind = "insert",
                    start = start,
                    text = insertedText,
                    durationMs = typingAnimationDurationMs
                )

                if (typingAnimationEnabled && editText.layout != null) {
                    val animEnd = kotlin.math.min(start + lastAddedCount, editable.length)
                    if (animEnd > start) {
                        val destX = editText.layout.getPrimaryHorizontal(start)
                        val line = editText.layout.getLineForOffset(start)
                        val destY = editText.layout.getLineBaseline(line).toFloat()
                        renderLayer.addTypingAnim(OverlayAnim(
                            insertedStart = start,
                            insertedText = insertedText,
                            startX = cursorBeforeX,
                            startY = cursorBeforeY,
                            endX = destX,
                            endY = destY,
                            durationMs = typingAnimationDurationMs,
                            isDeletion = false
                        ))
                    }
                }

                if (DEBUG_ANIM) {
                    android.util.Log.d(TAG, "afterTextChanged - recorded event: start=$start, length=${insertedText.length}")
                }
            }
            lastAddedStart = -1
            lastAddedCount = 0
        }
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
     * @param oldCursorIndex 变化前的光标位置（UTF-8 byte offset，Android 端为 char offset）
     * @param newCursorIndex 变化后的光标位置
     * @param cause 变化原因（"Typing", "Delete", "ImeComposition", "Paste", "Load" 等）
     * @param maxAnimatedChars 最大动画字符数
     * @param animationDurationMs 动画时长（毫秒）
     * @return Core 返回的动画事件 JSON 字符串
     */
    fun provide(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): String
}

/**
 * Core 返回的动画事件（解析后的结构）。
 */
data class CoreAnimationEvent(
    val id: ULong,
    val kind: String,
    val rangeStart: Int,
    val rangeLen: Int,
    val text: String,
    val oldCursorIndex: Int,
    val newCursorIndex: Int,
    val durationMs: Long
)

/**
 * 解析 Core 返回的动画事件 JSON。
 *
 * Core 返回格式为 `EditorAnimationEventDto` 的 JSON 数组，
 * 字段名为 camelCase（id, kind, rangeStart, rangeLen, text,
 * oldCursorIndex, newCursorIndex, durationMs）。
 */
private fun parseAnimationEvents(json: String): List<CoreAnimationEvent> {
    if (json.isBlank() || json == "[]") return emptyList()

    return try {
        val events = mutableListOf<CoreAnimationEvent>()
        // 简单的 JSON 解析，避免引入 Gson 依赖
        // 格式：[{"id":1,"kind":"Insert","rangeStart":2,"rangeLen":1,"text":"c","oldCursorIndex":2,"newCursorIndex":3,"durationMs":120},...]
        val trimmed = json.trim()
        if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) return emptyList()

        val inner = trimmed.substring(1, trimmed.length - 1).trim()
        if (inner.isEmpty()) return emptyList()

        // 拆分顶层对象
        val objects = splitTopLevelObjects(inner)
        for (obj in objects) {
            val fields = parseJsonObject(obj)
            events.add(CoreAnimationEvent(
                id = (fields["id"]?.toULongOrNull() ?: 0u),
                kind = fields["kind"] ?: "Insert",
                rangeStart = fields["rangeStart"]?.toIntOrNull() ?: 0,
                rangeLen = fields["rangeLen"]?.toIntOrNull() ?: 0,
                text = fields["text"]?.unescapeJson() ?: "",
                oldCursorIndex = fields["oldCursorIndex"]?.toIntOrNull() ?: 0,
                newCursorIndex = fields["newCursorIndex"]?.toIntOrNull() ?: 0,
                durationMs = fields["durationMs"]?.toLongOrNull() ?: 0L
            ))
        }
        events
    } catch (e: Exception) {
        if (DEBUG_ANIM) {
            android.util.Log.w("WriterEditorAnim", "Failed to parse animation events JSON", e)
        }
        emptyList()
    }
}

/**
 * 拆分顶层 JSON 对象（以 },{ 分隔）。
 */
private fun splitTopLevelObjects(json: String): List<String> {
    val result = mutableListOf<String>()
    var depth = 0
    var start = 0
    for (i in json.indices) {
        when (json[i]) {
            '{' -> depth++
            '}' -> {
                depth--
                if (depth == 0) {
                    result.add(json.substring(start, i + 1).trim())
                    start = i + 1
                    // 跳过逗号和空白
                    while (start < json.length && (json[start] == ',' || json[start].isWhitespace())) {
                        start++
                    }
                }
            }
        }
    }
    return result
}

/**
 * 简单的 JSON 对象解析器，返回 key→value 的 Map。
 * 只处理顶层字符串和数字值，不处理嵌套对象。
 */
private fun parseJsonObject(json: String): Map<String, String> {
    val result = mutableMapOf<String, String>()
    val trimmed = json.trim()
    if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return result

    val inner = trimmed.substring(1, trimmed.length - 1).trim()
    if (inner.isEmpty()) return result

    var i = 0
    while (i < inner.length) {
        // 跳过空白和逗号
        while (i < inner.length && (inner[i].isWhitespace() || inner[i] == ',')) i++
        if (i >= inner.length) break

        // 解析 key（必须是字符串）
        if (inner[i] != '"') break
        val keyStart = i + 1
        val keyEnd = findStringEnd(inner, keyStart)
        if (keyEnd < 0) break
        val key = inner.substring(keyStart, keyEnd).unescapeJson()
        i = keyEnd + 1

        // 跳过冒号和空白
        while (i < inner.length && (inner[i].isWhitespace() || inner[i] == ':')) i++
        if (i >= inner.length) break

        // 解析 value
        val value: String
        if (inner[i] == '"') {
            val valStart = i + 1
            val valEnd = findStringEnd(inner, valStart)
            if (valEnd < 0) break
            value = inner.substring(valStart, valEnd)
            i = valEnd + 1
        } else {
            // 数字或枚举值
            val valStart = i
            while (i < inner.length && inner[i] != ',' && inner[i] != '}') i++
            value = inner.substring(valStart, i).trim()
        }

        result[key] = value
    }

    return result
}

/**
 * 找到 JSON 字符串的结束引号位置（处理转义）。
 * @param start 开始搜索的位置（引号后第一个字符）
 * @return 结束引号的位置，-1 表示未找到
 */
private fun findStringEnd(s: String, start: Int): Int {
    var i = start
    while (i < s.length) {
        when (s[i]) {
            '\\' -> i += 2  // 跳过转义字符
            '"' -> return i
            else -> i++
        }
    }
    return -1
}

/**
 * 简单的 JSON 字符串反转义。
 */
private fun String.unescapeJson(): String {
    val sb = StringBuilder(length)
    var i = 0
    while (i < length) {
        if (this[i] == '\\' && i + 1 < length) {
            when (this[i + 1]) {
                '"' -> { sb.append('"'); i += 2 }
                '\\' -> { sb.append('\\'); i += 2 }
                'n' -> { sb.append('\n'); i += 2 }
                'r' -> { sb.append('\r'); i += 2 }
                't' -> { sb.append('\t'); i += 2 }
                '/' -> { sb.append('/'); i += 2 }
                else -> { sb.append(this[i]); i++ }
            }
        } else {
            sb.append(this[i])
            i++
        }
    }
    return sb.toString()
}

private fun String.toULongOrNull(): ULong? = try { toULong() } catch (_: Exception) { null }

data class AndroidEditorAnimationEvent(
    val kind: String,
    val start: Int,
    val text: String,
    val durationMs: Long
)
