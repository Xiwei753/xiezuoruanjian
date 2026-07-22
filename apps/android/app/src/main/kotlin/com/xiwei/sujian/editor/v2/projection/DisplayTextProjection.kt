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

        fun maskedWithComposition(
            text: String,
            compStartUtf16: Int,
            compEndUtf16: Int,
            compText: String,
            maskChar: String = "\u2022"
        ): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val utf16Len = text.length
            val safeCompStart = compStartUtf16.coerceIn(0, utf16Len)
            val safeCompEnd = compEndUtf16.coerceIn(safeCompStart, utf16Len)
            val maskedText = buildString {
                for (i in 0 until safeCompStart) {
                    if (text[i] == '\n') append('\n')
                    else append(maskChar)
                }
                append(compText)
                for (i in safeCompEnd until utf16Len) {
                    if (text[i] == '\n') append('\n')
                    else append(maskChar)
                }
            }
            val displayLen = maskedText.length
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
                        val realIdx = charIdx.coerceIn(0, utf16Len)
                        if (realIdx < safeCompStart) {
                            realIdx
                        } else if (realIdx >= safeCompEnd) {
                            realIdx + (compText.length - (safeCompEnd - safeCompStart))
                        } else {
                            safeCompStart + ((realIdx - safeCompStart) * compText.length / (safeCompEnd - safeCompStart).coerceAtLeast(1))
                        }.coerceIn(0, displayLen)
                    },
                    displayUtf16ToRealUtf8 = { utf16 ->
                        val safeUtf16 = utf16.coerceIn(0, displayLen)
                        val realIdx = when {
                            safeUtf16 < safeCompStart -> safeUtf16
                            safeUtf16 < safeCompStart + compText.length -> {
                                safeCompStart + ((safeUtf16 - safeCompStart) * (safeCompEnd - safeCompStart) / compText.length.coerceAtLeast(1))
                            }
                            else -> safeUtf16 - compText.length + (safeCompEnd - safeCompStart)
                        }.coerceIn(0, utf16Len)
                        text.substring(0, realIdx).toByteArray(Charsets.UTF_8).size
                    },
                    displayLengthUtf16 = displayLen,
                    realLengthUtf8 = bytes.size
                )
            )
        }

        fun masked(text: String, maskChar: String = "\u2022"): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val utf16Len = text.length
            val maskedText = buildString {
                for (char in text) {
                    if (char == '\n') append(char)
                    else append(maskChar)
                }
            }
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
