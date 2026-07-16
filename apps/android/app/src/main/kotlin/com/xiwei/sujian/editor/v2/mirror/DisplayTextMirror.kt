package com.xiwei.sujian.editor.v2.mirror

import android.text.SpannableStringBuilder
import android.text.style.UnderlineSpan
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import org.json.JSONArray
import org.json.JSONObject

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
                    baseRevision = obj.getLong("baseRevision"),
                    newRevision = obj.getLong("newRevision"),
                    replaceByteStart = obj.getInt("replaceByteStart"),
                    replaceByteEndExclusive = obj.getInt("replaceByteEndExclusive"),
                    insertedText = obj.getString("insertedText"),
                    resultingSelectionStart = obj.getInt("resultingSelectionStart"),
                    resultingSelectionEnd = obj.getInt("resultingSelectionEnd")
                ))
            }
            return patches
        }
    }
}

data class VisualIntent(
    val cause: String,
    val operationKind: String,
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    val newAffectedByteRanges: List<Pair<Int, Int>>,
    val animationMode: String,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor
) {
    companion object {
        fun fromJson(json: JSONObject): VisualIntent {
            val cursorObj = json.getJSONObject("coordinatedCursor")
            val oldRanges = parseByteRanges(json.optJSONArray("oldAffectedByteRanges"))
            val newRanges = parseByteRanges(json.optJSONArray("newAffectedByteRanges"))
            return VisualIntent(
                cause = json.getString("cause"),
                operationKind = json.getString("operationKind"),
                oldAffectedByteRanges = oldRanges,
                newAffectedByteRanges = newRanges,
                animationMode = json.getString("animationMode"),
                durationMs = json.getLong("durationMs"),
                coordinatedCursor = CoordinatedCursor(
                    oldByteOffset = cursorObj.getInt("oldByteOffset"),
                    newByteOffset = cursorObj.getInt("newByteOffset"),
                    shouldAnimate = cursorObj.getBoolean("shouldAnimate")
                )
            )
        }

        private fun parseByteRanges(arr: JSONArray?): List<Pair<Int, Int>> {
            if (arr == null) return emptyList()
            val ranges = mutableListOf<Pair<Int, Int>>()
            for (i in 0 until arr.length()) {
                val obj = arr.getJSONObject(i)
                val start = obj.getInt("start")
                val endExclusive = obj.getInt("endExclusive")
                ranges.add(Pair(start, endExclusive))
            }
            return ranges
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
            val obj = JSONObject(json)
            val patchesArray = obj.getJSONArray("displayPatches")
            val intentObj = obj.getJSONObject("visualIntent")

            return EditResult(
                transactionId = obj.getLong("transactionId"),
                baseRevision = obj.getLong("baseRevision"),
                newRevision = obj.getLong("newRevision"),
                displayPatches = DisplayPatch.fromJsonArray(patchesArray),
                oldSelectionStart = obj.getInt("oldSelectionStart"),
                oldSelectionEnd = obj.getInt("oldSelectionEnd"),
                newSelectionStart = obj.getInt("newSelectionStart"),
                newSelectionEnd = obj.getInt("newSelectionEnd"),
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

    fun getLengthUtf16(): Int = buffer.length

    fun applyPatches(patches: List<DisplayPatch>) {
        if (patches.isEmpty()) return

        val hadComposition = compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16
        val savedCompositionText = if (hadComposition) {
            buffer.substring(compositionStartUtf16, compositionEndUtf16)
        } else ""
        val savedCompositionStartUtf8 = cursorUtf8

        if (hadComposition) {
            buffer.replace(compositionStartUtf16, compositionEndUtf16, "")
            compositionStartUtf16 = -1
            compositionEndUtf16 = -1
        }

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
            UnderlineSpan(),
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

    fun getCompositionRangeUtf16(): Pair<Int, Int>? {
        if (compositionStartUtf16 < 0) return null
        return Pair(compositionStartUtf16, compositionEndUtf16)
    }

    private fun clearCompositionSpans() {
        val spans = buffer.getSpans(0, buffer.length, UnderlineSpan::class.java)
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
