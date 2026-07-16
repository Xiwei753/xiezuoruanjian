package com.xiwei.sujian.editor.v2.mirror

import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorVisualIntentDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorByteRangeDto

object EditResultDtoMapper {

    fun dtoToJson(dto: EditorEditResultDto): String {
        val sb = StringBuilder()
        sb.append("{")
        sb.append("\"transactionId\":${dto.transactionId},")
        sb.append("\"baseRevision\":${dto.baseRevision},")
        sb.append("\"newRevision\":${dto.newRevision},")
        sb.append("\"displayPatches\":[")
        dto.displayPatches.forEachIndexed { i, patch ->
            if (i > 0) sb.append(",")
            sb.append("{")
            sb.append("\"baseRevision\":${patch.baseRevision},")
            sb.append("\"newRevision\":${patch.newRevision},")
            sb.append("\"replaceByteStart\":${patch.replaceByteStart},")
            sb.append("\"replaceByteEndExclusive\":${patch.replaceByteEndExclusive},")
            sb.append("\"insertedText\":${escapeJson(patch.insertedText)},")
            sb.append("\"resultingSelectionStart\":${patch.resultingSelectionStart},")
            sb.append("\"resultingSelectionEnd\":${patch.resultingSelectionEnd}")
            sb.append("}")
        }
        sb.append("],")
        sb.append("\"oldSelectionStart\":${dto.oldSelectionStart},")
        sb.append("\"oldSelectionEnd\":${dto.oldSelectionEnd},")
        sb.append("\"newSelectionStart\":${dto.newSelectionStart},")
        sb.append("\"newSelectionEnd\":${dto.newSelectionEnd},")
        sb.append("\"visualIntent\":${visualIntentToJson(dto.visualIntent)}")
        sb.append("}")
        return sb.toString()
    }

    private fun visualIntentToJson(vi: EditorVisualIntentDto): String {
        val sb = StringBuilder()
        sb.append("{")
        sb.append("\"cause\":\"${vi.cause.name.replace("_", "")}\",")
        sb.append("\"operationKind\":\"${vi.operationKind.name.replace("_", "")}\",")
        sb.append("\"oldAffectedByteRanges\":[")
        vi.oldAffectedByteRanges.forEachIndexed { i, range ->
            if (i > 0) sb.append(",")
            sb.append("{\"start\":${range.start},\"endExclusive\":${range.endExclusive}}")
        }
        sb.append("],")
        sb.append("\"newAffectedByteRanges\":[")
        vi.newAffectedByteRanges.forEachIndexed { i, range ->
            if (i > 0) sb.append(",")
            sb.append("{\"start\":${range.start},\"endExclusive\":${range.endExclusive}}")
        }
        sb.append("],")
        sb.append("\"animationMode\":\"${vi.animationMode.name.replace("_", "")}\",")
        sb.append("\"durationMs\":${vi.durationMs},")
        sb.append("\"coordinatedCursor\":${coordinatedCursorToJson(vi.coordinatedCursor)}")
        sb.append("}")
        return sb.toString()
    }

    private fun coordinatedCursorToJson(cc: CoordinatedCursorDto): String {
        return "{\"oldByteOffset\":${cc.oldByteOffset},\"newByteOffset\":${cc.newByteOffset},\"shouldAnimate\":${cc.shouldAnimate}}"
    }

    private fun escapeJson(s: String): String {
        val escaped = s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
        return "\"$escaped\""
    }
}
