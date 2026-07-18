package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputAdapter(
    context: Context,
    private val mirror: DisplayTextMirror,
    private val editorView: SujianEditorView
) : View(context) {

    private var currentCompositionText: String = ""
    private var previousCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        if (outAttrs != null) {
            outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or
                    android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            outAttrs.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION or
                    android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            return AndroidInputConnection(this, mirror, editorView)
        }
        return null
    }

    override fun onCheckIsTextEditor(): Boolean = true

    fun sendInsertToKernel(byteOffset: Int, text: String, cause: EditorTransactionCauseDto) {
        editorView.insertText(byteOffset, text, cause)
    }

    fun sendDeleteToKernel(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        editorView.deleteRange(byteStart, byteEndExclusive, cause)
    }

    fun sendReplaceToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto) {
        editorView.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)
    }

    fun sendSetSelectionToKernel(anchorByteOffset: Int, headByteOffset: Int) {
        editorView.setSelectionTyped(anchorByteOffset, headByteOffset)
    }

    fun handleCompositionUpdate(preeditText: String, newCursorPosition: Int) {
        if (!isComposing) {
            val selStart = mirror.getSelectionStartUtf8()
            val selEnd = mirror.getSelectionEndUtf8()
            if (selStart != selEnd) {
                compositionReplaceStartUtf8 = selStart
                compositionReplaceEndUtf8 = selEnd
            } else {
                compositionReplaceStartUtf8 = mirror.getCursorUtf8()
                compositionReplaceEndUtf8 = compositionReplaceStartUtf8
            }
            isComposing = true
        }
        previousCompositionText = currentCompositionText
        currentCompositionText = preeditText

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val intentDto = bridge.compositionUpdateVisualIntent(
                compositionReplaceStartUtf8.toUInt(),
                compositionReplaceEndUtf8.toUInt(),
                previousCompositionText,
                preeditText,
            )
            if (intentDto != null) {
                val visualIntent = com.xiwei.sujian.editor.v2.mirror.VisualIntent.fromDto(intentDto)
                editorView.applyCompositionUpdate(visualIntent) {
                    mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
                }
                applyNewCursorPositionInComposition(newCursorPosition, preeditText)
                return
            }
        }

        mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        applyNewCursorPositionInComposition(newCursorPosition, preeditText)
        editorView.onCompositionUpdated()
    }

    private fun applyNewCursorPositionInComposition(newCursorPosition: Int, preeditText: String) {
        val compositionRangeUtf16 = mirror.getCompositionRangeUtf16() ?: return
        val targetUtf16: Int
        if (newCursorPosition > 0) {
            targetUtf16 = (compositionRangeUtf16.first + (newCursorPosition - 1)).coerceIn(compositionRangeUtf16.first, compositionRangeUtf16.second)
        } else if (newCursorPosition == 0) {
            targetUtf16 = compositionRangeUtf16.first
        } else {
            targetUtf16 = (compositionRangeUtf16.first + newCursorPosition).coerceAtLeast(0)
        }
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val targetUtf8 = indexMap.utf16ToUtf8(targetUtf16)
        mirror.setSelectionInternal(targetUtf8, targetUtf8)
    }

    fun applyNewCursorPosition(newCursorPosition: Int) {
        val committedCursorUtf8 = mirror.getCommittedCursorUtf8()
        val committedBytes = mirror.getCommittedText().toByteArray(Charsets.UTF_8)
        val cursorByteOffset = committedCursorUtf8.coerceIn(0, committedBytes.size)
        val targetUtf8: Int
        if (newCursorPosition > 0) {
            var pos = cursorByteOffset
            var remaining = newCursorPosition - 1
            while (remaining > 0 && pos < committedBytes.size) {
                pos++
                while (pos < committedBytes.size && (committedBytes[pos].toInt() and 0xC0) == 0x80) {
                    pos++
                }
                remaining--
            }
            targetUtf8 = pos
        } else if (newCursorPosition == 0) {
            targetUtf8 = cursorByteOffset
        } else {
            var pos = cursorByteOffset
            var remaining = -newCursorPosition
            while (remaining > 0 && pos > 0) {
                pos--
                while (pos > 0 && (committedBytes[pos].toInt() and 0xC0) == 0x80) {
                    pos--
                }
                remaining--
            }
            targetUtf8 = pos
        }
        if (isComposing) {
            mirror.setSelectionInternal(targetUtf8, targetUtf8)
        } else {
            editorView.setSelectionTyped(targetUtf8, targetUtf8)
        }
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        val committedText = currentCompositionText
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val dto = bridge.compositionCommit(replaceStart, replaceEnd, committedText, "")
            if (dto != null) {
                editorView.applyCompositionCommit(dto)
                return
            }
        }

        editorView.clearCompositionAndReplace(replaceStart, replaceEnd, committedText, "", EditorTransactionCauseDto.TYPING_COMMIT)
    }

    fun handleCompositionCommitWithText(finalText: String, newCursorPosition: Int) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val dto = bridge.compositionCommit(replaceStart, replaceEnd, finalText, "")
            if (dto != null) {
                editorView.applyCompositionCommit(dto)
                if (newCursorPosition != 0 && newCursorPosition != 1) {
                    applyNewCursorPosition(newCursorPosition)
                }
                return
            }
        }

        editorView.clearCompositionAndReplace(replaceStart, replaceEnd, finalText, "", EditorTransactionCauseDto.TYPING_COMMIT)
        if (newCursorPosition != 0 && newCursorPosition != 1) {
            applyNewCursorPosition(newCursorPosition)
        }
    }

    fun handleCompositionCancel() {
        if (!isComposing) return
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        editorView.applyCompositionUpdate(
            com.xiwei.sujian.editor.v2.mirror.VisualIntent(
                cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
                operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0L,
                coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, false)
            )
        ) {
            mirror.clearComposition()
        }
    }

    fun startComposingRegion(byteStart: Int, byteEnd: Int, selectedText: String) {
        compositionReplaceStartUtf8 = byteStart
        compositionReplaceEndUtf8 = byteEnd
        currentCompositionText = selectedText
        previousCompositionText = ""
        isComposing = true
    }

    fun isComposing(): Boolean = isComposing

    fun getCompositionText(): String = currentCompositionText

    fun getCompositionCursorOffset(): Int? {
        if (!isComposing) return null
        return currentCompositionText.length
    }

    fun getCompositionRangeUtf8(): Pair<Int, Int>? {
        if (!isComposing) return null
        return Pair(compositionReplaceStartUtf8, compositionReplaceEndUtf8)
    }
}
