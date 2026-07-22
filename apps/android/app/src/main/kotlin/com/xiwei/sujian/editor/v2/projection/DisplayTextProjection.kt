package com.xiwei.sujian.editor.v2.projection

data class OffsetMapping(
    val realUtf8ToDisplayUtf16: (Int) -> Int,
    val displayUtf16ToRealUtf8: (Int) -> Int,
    val displayLengthUtf16: Int,
    val realLengthUtf8: Int
)

class DisplayTextProjection(
    val realText: String,
    val displayText: String,
    val isMasked: Boolean,
    private val offsetMapping: OffsetMapping
) {
    val realLengthUtf8: Int get() = offsetMapping.realLengthUtf8
    val displayLengthUtf16: Int get() = offsetMapping.displayLengthUtf16

    fun realUtf8ToDisplayUtf16(utf8: Int): Int =
        offsetMapping.realUtf8ToDisplayUtf16(utf8)

    fun displayUtf16ToRealUtf8(utf16: Int): Int =
        offsetMapping.displayUtf16ToRealUtf8(utf16)

    fun getDisplaySpannable(): android.text.SpannableStringBuilder =
        android.text.SpannableStringBuilder(displayText)

    companion object {
        fun identity(text: String): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val utf16Len = text.length
            return DisplayTextProjection(
                realText = text,
                displayText = text,
                isMasked = false,
                offsetMapping = OffsetMapping(
                    realUtf8ToDisplayUtf16 = { utf8 -> utf8.coerceIn(0, utf16Len) },
                    displayUtf16ToRealUtf8 = { utf16 ->
                        val chars = text.substring(0, utf16.coerceIn(0, utf16Len))
                        chars.toByteArray(Charsets.UTF_8).size
                    },
                    displayLengthUtf16 = utf16Len,
                    realLengthUtf8 = bytes.size
                )
            )
        }

        fun masked(text: String, maskChar: String = "\u2022"): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val utf16Len = text.length
            val maskedText = maskChar.repeat(utf16Len)
            return DisplayTextProjection(
                realText = text,
                displayText = maskedText,
                isMasked = true,
                offsetMapping = OffsetMapping(
                    realUtf8ToDisplayUtf16 = { utf8 ->
                        var bytePos = 0
                        var charIdx = 0
                        val chars = text.toCharArray()
                        while (charIdx < chars.size && bytePos < utf8) {
                            val charBytes = chars[charIdx].toString().toByteArray(Charsets.UTF_8)
                            bytePos += charBytes.size
                            if (bytePos <= utf8) charIdx++
                        }
                        charIdx.coerceIn(0, utf16Len)
                    },
                    displayUtf16ToRealUtf8 = { utf16 ->
                        val safeUtf16 = utf16.coerceIn(0, utf16Len)
                        text.substring(0, safeUtf16).toByteArray(Charsets.UTF_8).size
                    },
                    displayLengthUtf16 = utf16Len,
                    realLengthUtf8 = bytes.size
                )
            )
        }
    }
}
