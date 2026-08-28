package com.xiwei.sujian.feature.editor.input

import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.runtime.Stable
import androidx.compose.ui.text.TextRange

/**
 * #641 评论1 第2节：IME 已提交的文本快照 — 从 [TextFieldState] 读出的
 * 不可变快照，供 [EditorTextFieldStateBridge.onInputSnapshot] 判断是否需要提交。
 */
data class EditorInputSnapshot(
    val text: String,
    val selection: TextRange,
    val composition: TextRange?,
)

/**
 * #641 评论1 第2节：一次连续替换 — 共同前缀 + 共同后缀算出最小 replace range。
 * offset 是 UTF-16，进入 Rust/Core bridge 前由调用方转成 UTF-8 byte offset。
 */
data class CommittedTextEdit(
    val oldText: String,
    val replaceStart: Int,
    val replaceEndExclusive: Int,
    val newText: String,
    val selection: TextRange,
)

/**
 * #641 评论1 第2节：Core 提交结果 — [Accepted] 表示 Core 已采纳，
 * [Rejected] 表示 Core 拒绝并要求回退到权威正文（同步/撤销/重载覆盖）。
 */
sealed interface CommitResult {
    data object Accepted : CommitResult

    data class Rejected(
        val text: String,
        val selection: TextRange,
    ) : CommitResult
}

/**
 * #641 评论1 第2节：让 [TextFieldState] 成为 Android 实时输入状态。
 *
 * 职责只有：
 * - 持有 [TextFieldState]；
 * - 把 Android 已提交的文本变化转成现有 Core 事务（经 [commitToCore]）；
 * - 把同步/撤销/重载等外部正文事实写回 [TextFieldState]。
 *
 * IME composing/preedit 仍交给 Android 自己管理，不把 composing 中间态当正文事务保存。
 * 同步事实在 `state.composition != null` 时到达，由调用方走现有
 * `storePendingExternalFact` / 冲突链，等 composition 结束再应用。
 */
@Stable
class EditorTextFieldStateBridge(
    initialText: String,
    initialSelection: TextRange,
    private val commitToCore: (CommittedTextEdit) -> CommitResult,
) {
    val state =
        TextFieldState(
            initialText = initialText,
            initialSelection = initialSelection,
        )

    private var committedMirror: String = initialText

    fun onInputSnapshot(snapshot: EditorInputSnapshot) {
        if (snapshot.composition != null) return
        commitIfNeeded(snapshot.text, snapshot.selection)
    }

    /**
     * 切章节/返回时，即使 IME 仍有 composition，也先把屏幕上的最终内容提交给 Core，
     * 再走现有 save/close，不能丢掉候选上屏前的内容。
     */
    fun flushForClose() {
        commitIfNeeded(state.text.toString(), state.selection)
    }

    private fun commitIfNeeded(
        text: String,
        selection: TextRange,
    ) {
        if (text == committedMirror) return

        val edit =
            computeSingleReplace(
                oldText = committedMirror,
                newText = text,
                selection = selection,
            )

        when (val result = commitToCore(edit)) {
            is CommitResult.Accepted -> committedMirror = text
            is CommitResult.Rejected ->
                applyAuthoritativeText(
                    result.text,
                    result.selection,
                )
        }
    }

    /**
     * 外部权威正文（同步/撤销/重载）写回 [TextFieldState]。
     * 同时更新 [committedMirror]，使后续输入 diff 从权威正文开始。
     */
    fun applyAuthoritativeText(
        text: String,
        selection: TextRange,
    ) {
        committedMirror = text
        state.edit {
            replace(0, length, text)
            this.selection = selection
        }
    }

    /** 当前已提交给 Core 的正文镜像 — 供调用方判断 dirty。 */
    val mirroredText: String get() = committedMirror
}

/**
 * #641 评论1 第2节：共同前缀 + 共同后缀算一次连续 replace。
 * offset 是 UTF-16。
 *
 * #641 评论 问题1b：共同前缀/后缀与 replace 端点都按 Unicode code point 边界对齐，
 * 不把 emoji/CJK 之外的 supplementary plane 字符（surrogate pair）拆在 high/low 中间。
 */
internal fun computeSingleReplace(
    oldText: String,
    newText: String,
    selection: TextRange,
): CommittedTextEdit {
    val commonPrefix = commonPrefixLength(oldText, newText)
    val commonSuffix = commonSuffixLength(oldText, newText, commonPrefix)
    val replaceStart = commonPrefix
    val replaceEndExclusive = oldText.length - commonSuffix
    // 确保 replaceStart 和 replaceEndExclusive 不落在 surrogate pair 中间
    val safeStart = safeCodePointBoundary(oldText, replaceStart)
    val safeEnd = safeCodePointBoundary(oldText, replaceEndExclusive)
    val insertedText = newText.substring(commonPrefix, newText.length - commonSuffix)
    return CommittedTextEdit(
        oldText = oldText,
        replaceStart = safeStart,
        replaceEndExclusive = safeEnd,
        newText = insertedText,
        selection = selection,
    )
}

/**
 * #641 评论 问题1b：把 [index] 校正到 Unicode code point 边界。
 * 若 [index] 落在 low surrogate 上（即 surrogate pair 中间），回退到 high surrogate。
 * 边界 0 / text.length 直接返回。
 */
private fun safeCodePointBoundary(
    text: String,
    index: Int,
): Int {
    if (index <= 0 || index >= text.length) return index
    // 如果 index 落在 low surrogate 上，回退到 high surrogate
    return if (text[index].isLowSurrogate()) index - 1 else index
}

private fun commonPrefixLength(
    a: String,
    b: String,
): Int {
    val min = minOf(a.length, b.length)
    var i = 0
    while (i < min && a[i] == b[i]) i++
    // 校正到 code point 边界：如果 i 落在 low surrogate 上，回退到 high surrogate
    if (i > 0 && i < a.length && a[i].isLowSurrogate()) i--
    return i
}

private fun commonSuffixLength(
    a: String,
    b: String,
    prefixLen: Int,
): Int {
    val aRemain = a.length - prefixLen
    val bRemain = b.length - prefixLen
    val min = minOf(aRemain, bRemain)
    var i = 0
    while (i < min && a[a.length - 1 - i] == b[b.length - 1 - i]) i++
    // 校正到 code point 边界：如果 a[a.length - i] 是 high surrogate
    // （即 i 落在 surrogate pair 中间），回退
    if (i > 0 && a.length - i > 0 && a[a.length - i].isHighSurrogate()) i--
    return i
}
