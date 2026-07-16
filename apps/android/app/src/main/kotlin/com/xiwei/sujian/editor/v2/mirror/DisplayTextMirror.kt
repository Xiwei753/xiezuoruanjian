package com.xiwei.sujian.editor.v2.mirror

import android.text.SpannableStringBuilder
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import org.json.JSONArray

data class DisplayPatch(
    val baseRevision: Long,
    val newRevision: Long,
    val replaceByteStart: Int,
    val replaceByteEndExclusive: Int,
    val insertedText: String,
    val resultingSelectionStart: Int,
    val resultingSelectionEnd: Int
) {
    companion object {
        fun fromJsonArray(jsonArray: JSONArray): List<DisplayPatch> {
            val patches = mutableListOf<DisplayPatch>()
            for (i in 0 until jsonArray.length()) {
                val obj = jsonArray.getJSONObject(i)
                patches.add(DisplayPatch(
                    baseRevision = obj.getLong("base_revision"),
                    newRevision = obj.getLong("new_revision"),
                    replaceByteStart = obj.getInt("replace_byte_start"),
                    replaceByteEndExclusive = obj.getInt("replace_byte_end_exclusive"),
                    insertedText = obj.getString("inserted_text"),
                    resultingSelectionStart = obj.getInt("resulting_selection_start"),
                    resultingSelectionEnd = obj.getInt("resulting_selection_end")
                ))
            }
            return patches
        }
    }
}

data class VisualIntent(
    val cause: String,
    val operationKind: String,
    val animationMode: String,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor
) {
    companion object {
        fun fromJson(json: org.json.JSONObject): VisualIntent {
            val cursorObj = json.getJSONObject("coordinated_cursor")
            return VisualIntent(
                cause = json.getString("cause"),
                operationKind = json.getString("operation_kind"),
                animationMode = json.getString("animation_mode"),
                durationMs = json.getLong("duration_ms"),
                coordinatedCursor = CoordinatedCursor(
                    oldByteOffset = cursorObj.getInt("old_byte_offset"),
                    newByteOffset = cursorObj.getInt("new_byte_offset"),
                    shouldAnimate = cursorObj.getBoolean("should_animate")
                )
            )
        }
    }
}

data class CoordinatedCursor(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val shouldAnimate: Boolean
)

data class EditResult(
    val transactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val displayPatches: List<DisplayPatch>,
    val oldSelectionStart: Int,
    val oldSelectionEnd: Int,
    val newSelectionStart: Int,
    val newSelectionEnd: Int,
    val visualIntent: VisualIntent
) {
    companion object {
        fun fromJson(json: String): EditResult {
            val obj = org.json.JSONObject(json)
            val patchesArray = obj.getJSONArray("display_patches")
            val intentObj = obj.getJSONObject("visual_intent")

            return EditResult(
                transactionId = obj.getLong("transaction_id"),
                baseRevision = obj.getLong("base_revision"),
                newRevision = obj.getLong("new_revision"),
                displayPatches = DisplayPatch.fromJsonArray(patchesArray),
                oldSelectionStart = obj.getInt("old_selection_start"),
                oldSelectionEnd = obj.getInt("old_selection_end"),
                newSelectionStart = obj.getInt("new_selection_start"),
                newSelectionEnd = obj.getInt("new_selection_end"),
                visualIntent = VisualIntent.fromJson(intentObj)
            )
        }
    }
}

class DisplayTextMirror {
    private val buffer = SpannableStringBuilder()
    private var currentRevision: Long = 0
    private var cursorUtf8: Int = 0
    private var cursorUtf16: Int = 0
    private var compositionStartUtf16: Int = -1
    private var compositionEndUtf16: Int = -1

    fun getText(): String = buffer.toString()

    fun getCursorUtf8(): Int = cursorUtf8

    fun getCursorUtf16(): Int = cursorUtf16

    fun getRevision(): Long = currentRevision

    fun getSpannable(): SpannableStringBuilder = buffer

    fun applyPatches(patches: List<DisplayPatch>) {
        val indexMap = AndroidTextIndexMap(this)
        for (patch in patches) {
            if (patch.newRevision <= currentRevision) continue

            val replaceStartUtf16 = indexMap.utf8ToUtf16(patch.replaceByteStart)
            val replaceEndUtf16 = indexMap.utf8ToUtf16(patch.replaceByteEndExclusive)

            buffer.replace(replaceStartUtf16, replaceEndUtf16, patch.insertedText)

            currentRevision = patch.newRevision
            cursorUtf8 = patch.resultingSelectionEnd
            cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
        }
    }

    fun updateComposition(replaceStartUtf8: Int, replaceEndUtf8: Int, preeditText: String) {
        val indexMap = AndroidTextIndexMap(this)
        clearCompositionSpans()

        if (compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16) {
            buffer.replace(compositionStartUtf16, compositionEndUtf16, preeditText)
        } else {
            val insertPos = indexMap.utf8ToUtf16(replaceStartUtf8)
            buffer.insert(insertPos, preeditText)
            compositionStartUtf16 = insertPos
        }
        compositionEndUtf16 = compositionStartUtf16 + preeditText.length

        buffer.setSpan(
            android.text.style.UnderlineSpan(),
            compositionStartUtf16,
            compositionEndUtf16,
            SpannableStringBuilder.SPAN_EXCLUSIVE_EXCLUSIVE
        )
    }

    fun clearComposition() {
        clearCompositionSpans()
        compositionStartUtf16 = -1
        compositionEndUtf16 = -1
    }

    private fun clearCompositionSpans() {
        val spans = buffer.getSpans(0, buffer.length, android.text.style.UnderlineSpan::class.java)
        for (span in spans) {
            buffer.removeSpan(span)
        }
    }

    fun loadText(text: String, cursorUtf8: Int) {
        buffer.clear()
        buffer.append(text)
        this.cursorUtf8 = cursorUtf8
        this.currentRevision = 0
        this.compositionStartUtf16 = -1
        this.compositionEndUtf16 = -1
        val indexMap = AndroidTextIndexMap(this)
        this.cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
    }
}
