@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.layout

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #717 评论 5743443030 修复2：EditorSoftBreakProjection 单元测试。
 *
 * 覆盖：
 * - code point 判断（补充平面 Latin、非 BMP 数字、普通拉丁扩展）
 * - 连接符（'、’、_）并入 word run 的各种边界情况
 * - 回归（CJK 不插入、空格断开、纯 ASCII、空字符串、单字符）
 * - displayLength / rawToDisplay / displayToRaw / toDisplayRange 互逆与正确性
 */
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class EditorSoftBreakProjectionTest {
    // ===== code point 测试 =====

    /**
     * 补充平面 Latin 字符（Latin Extended-F U+10760）是 surrogate pair。
     * 构造 raw = latinF + "bc"（3 个 grapheme），fromRawText 应在内部 boundary 插入 U+200B。
     *
     * Latin Extended-F 是 Unicode 14.0 加入的；JDK 17 只支持 Unicode 13.0，
     * 若运行环境不识别该字符的 Latin script，用 [assumeTrue] 跳过而非失败。
     */
    @Test
    fun supplementaryPlaneLatinDoesNotCrash() {
        // Latin Extended-F: U+10760 LATIN SMALL LETTER OPEN O WITH RETROFLEX HOOK
        // Script = Latin，补充平面（surrogate pair）
        val codePoint = 0x10760
        assumeTrue(
            "JDK 须识别 U+10760 为 Latin script（需要 Unicode 14.0+）",
            Character.UnicodeScript.of(codePoint) == Character.UnicodeScript.LATIN,
        )
        val latinF = String(Character.toChars(codePoint))
        val raw = latinF + "bc" // 3 grapheme: latinF, b, c
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(raw.length, projection.rawLength)
        assertTrue(projection.insertPoints.isNotEmpty())
        assertEquals(raw.length + projection.insertPoints.size, projection.displayLength)
        // latinF 占 2 UTF-16 unit，b 在 offset 2，c 在 offset 3
        // 内部 boundary 在 2（latinF|b）、3（b|c）
        assertEquals(listOf(2, 3), projection.insertPoints)
    }

    /**
     * 非 BMP 数字（Brahmi 数字 U+11066，General Category = Nd），Character.isDigit 返回 true。
     * 构造含此字符的 word run，验证不断行（word run 不被切断）。
     */
    @Test
    fun supplementaryPlaneDigitIsWordChar() {
        val codePoint = 0x11066
        val brahmiDigit = String(Character.toChars(codePoint))
        // 验证这个 code point 确实是数字
        assertTrue(Character.isDigit(codePoint))
        val raw = brahmiDigit + "1" // 2 grapheme: brahmiDigit, 1
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(raw.length, projection.rawLength)
        // 作为一个 word run，内部 boundary 在 2
        assertEquals(listOf(2), projection.insertPoints)
    }

    /** 普通拉丁扩展（é, ñ, ü）仍正常。 */
    @Test
    fun latinExtendedCharsAreWordChars() {
        val raw = "éñü"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2), projection.insertPoints)
    }

    // ===== 连接符测试 =====

    /** "I'm" → ' 左右都是 Latin → 并入 run，insertPoints 包含 1 和 2。 */
    @Test
    fun connectorApostropheJoinsRun() {
        val raw = "I'm"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2), projection.insertPoints)
    }

    /** "a_b" → _ 左右都是 Latin → 并入 run。 */
    @Test
    fun connectorUnderscoreJoinsRun() {
        val raw = "a_b"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2), projection.insertPoints)
    }

    /** "don't" → ' 并入，整个是一个 run，内部 boundary 1, 2, 3, 4。 */
    @Test
    fun connectorInLongerWord() {
        val raw = "don't"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2, 3, 4), projection.insertPoints)
    }

    /** 连接符在开头 "'abc" → ' 左边无 Latin → 不并入，abc 是一个 run。 */
    @Test
    fun connectorAtStartDoesNotJoin() {
        val raw = "'abc"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        // abc 是一个 run [1, 4)，内部 boundary 2, 3
        assertEquals(listOf(2, 3), projection.insertPoints)
    }

    /** 连接符在结尾 "abc'" → ' 右边无 Latin → 不并入。 */
    @Test
    fun connectorAtEndDoesNotJoin() {
        val raw = "abc'"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        // abc 是一个 run [0, 3)，内部 boundary 1, 2
        assertEquals(listOf(1, 2), projection.insertPoints)
    }

    /** "ab-cd" → - 是连字符不处理，ab 和 cd 分成两个 run。 */
    @Test
    fun hyphenIsNotConnector() {
        val raw = "ab-cd"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        // ab 一个 run（insertPoint 在 1），cd 一个 run（insertPoint 在 4）
        assertEquals(listOf(1, 4), projection.insertPoints)
    }

    /** "a'_b" → ' 右边是 _（非 Latin/数字），不并入；_ 左边无 Latin run，不并入。无 insertPoint。 */
    @Test
    fun connectorBetweenNonLatinDoesNotJoin() {
        val raw = "a'_b"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(emptyList<Int>(), projection.insertPoints)
    }

    /** 右单引号 ’（U+2019）也作为连接符。 */
    @Test
    fun rightSingleQuotationMarkJoinsRun() {
        val raw = "I\u2019m" // I 'm 使用 U+2019
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2), projection.insertPoints)
    }

    /**
     * 补充平面 Latin + 连接符组合不崩。
     *
     * 同 [supplementaryPlaneLatinDoesNotCrash]，需要 Unicode 14.0+ 环境。
     */
    @Test
    fun supplementaryPlaneLatinWithConnector() {
        val codePoint = 0x10760
        assumeTrue(
            "JDK 须识别 U+10760 为 Latin script（需要 Unicode 14.0+）",
            Character.UnicodeScript.of(codePoint) == Character.UnicodeScript.LATIN,
        )
        val latinF = String(Character.toChars(codePoint))
        val raw = latinF + "'b" // latinF, ', b
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        // ' 左右都是 Latin → 并入，run [0, 4)，内部 boundary 2, 3
        assertEquals(listOf(2, 3), projection.insertPoints)
    }

    // ===== 回归测试 =====

    /** CJK 字符间不插入。 */
    @Test
    fun cjkCharsHaveNoInsertPoints() {
        val raw = "你好世界"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(emptyList<Int>(), projection.insertPoints)
    }

    /** 空格断开："hello world" → 两个 run，各自内部插入。 */
    @Test
    fun spaceBreaksWordRuns() {
        val raw = "hello world"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2, 3, 4, 7, 8, 9, 10), projection.insertPoints)
    }

    /** 纯 ASCII "abcdef" → insertPoints 在每个内部 boundary。 */
    @Test
    fun pureAsciiWord() {
        val raw = "abcdef"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(listOf(1, 2, 3, 4, 5), projection.insertPoints)
    }

    /** 空字符串 → identity。 */
    @Test
    fun emptyStringReturnsIdentity() {
        val projection = EditorSoftBreakProjection.fromRawText("")
        assertEquals(0, projection.rawLength)
        assertEquals(emptyList<Int>(), projection.insertPoints)
        assertEquals(0, projection.displayLength)
    }

    /** 单字符 "a" → 无 insertPoint。 */
    @Test
    fun singleCharHasNoInsertPoint() {
        val raw = "a"
        val projection = EditorSoftBreakProjection.fromRawText(raw)
        assertEquals(emptyList<Int>(), projection.insertPoints)
    }

    // ===== displayLength / rawToDisplay / displayToRaw / toDisplayRange 测试 =====

    /** rawToDisplay 和 displayToRaw 互逆。 */
    @Test
    fun rawToDisplayAndDisplayToRawAreInverse() {
        val projection = EditorSoftBreakProjection.fromRawText("don't")
        assertEquals(listOf(1, 2, 3, 4), projection.insertPoints)
        assertEquals(5, projection.rawLength)
        assertEquals(9, projection.displayLength)
        // 对每个 raw offset，displayToRaw(rawToDisplay(x)) == x
        for (rawOffset in 0..projection.rawLength) {
            val displayOffset = projection.rawToDisplay(rawOffset)
            val backToRaw = projection.displayToRaw(displayOffset)
            assertEquals(rawOffset, backToRaw)
        }
    }

    /** 已知 projection 的 rawToDisplay 精确值。 */
    @Test
    fun rawToDisplayExactValues() {
        val projection = EditorSoftBreakProjection.fromRawText("don't")
        // insertPoints = [1, 2, 3, 4]
        assertEquals(0, projection.rawToDisplay(0))
        assertEquals(2, projection.rawToDisplay(1))
        assertEquals(4, projection.rawToDisplay(2))
        assertEquals(6, projection.rawToDisplay(3))
        assertEquals(8, projection.rawToDisplay(4))
        assertEquals(9, projection.rawToDisplay(5))
    }

    /** 已知 projection 的 displayToRaw 精确值。 */
    @Test
    fun displayToRawExactValues() {
        val projection = EditorSoftBreakProjection.fromRawText("don't")
        // insertPoints = [1, 2, 3, 4], displayLength = 9
        assertEquals(0, projection.displayToRaw(0))
        assertEquals(1, projection.displayToRaw(2))
        assertEquals(2, projection.displayToRaw(4))
        assertEquals(3, projection.displayToRaw(6))
        assertEquals(4, projection.displayToRaw(8))
        assertEquals(5, projection.displayToRaw(9))
    }

    /** 含连接符的文本 toDisplayRange 正确。 */
    @Test
    fun toDisplayRangeWithConnector() {
        val projection = EditorSoftBreakProjection.fromRawText("don't")
        // 整个 range
        assertEquals(TextRange(0, 9), projection.toDisplayRange(TextRange(0, 5)))
        // d 的 range
        assertEquals(TextRange(0, 2), projection.toDisplayRange(TextRange(0, 1)))
        // ' 的 range (raw 3..4 → display 6..8)
        assertEquals(TextRange(6, 8), projection.toDisplayRange(TextRange(3, 4)))
    }

    /** identity projection 的 rawToDisplay / displayToRaw 是 identity。 */
    @Test
    fun identityProjectionIsIdentity() {
        val identity = EditorSoftBreakProjection.identity()
        assertEquals(0, identity.rawLength)
        assertEquals(0, identity.displayLength)
        assertEquals(0, identity.rawToDisplay(0))
        assertEquals(0, identity.displayToRaw(0))
    }
}
