package com.xiwei.sujian.feature.editor.input

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #641 评论1 第2节：[EditorTextFieldStateBridge] 契约测试。
 *
 * 覆盖：
 * - IME composition 中间态不入 Core（不把 preedit 当正文事务保存）；
 * - [flushForClose] 切章节/返回时即使 IME 仍有 composition 也先把屏幕最终内容提交给 Core；
 * - 外部权威正文（同步/撤销/重载）经 [applyAuthoritativeText] 写回 [TextFieldState]；
 * - [computeSingleReplace] 共同前缀 + 共同后缀算一次连续 replace，offset 是 UTF-16，
 *   进入 Core 前由调用方转成 UTF-8 byte offset。
 *
 * 用 Robolectric 提供 [TextFieldState] 所需的 Compose/Android runtime。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorTextFieldStateBridgeTest {
    companion object {
        private const val AUTHORITATIVE_TEXT = "同步新正文"
        private const val UNDO_RESTORED_TEXT = "撤销后正文"
        private const val UNDO_RESTORED_SHORT = "撤销后"
        private const val MIRROR_FOLLOWS_AUTHORITY = "mirror 跟随权威正文"
    }

    @Test
    fun computeSingleReplace_pureInsert_hasEmptyCommonPrefixAndSuffix() {
        val edit =
            computeSingleReplace(
                oldText = "",
                newText = "abc",
                selection = TextRange(3, 3),
            )
        assertEquals(0, edit.replaceStart)
        assertEquals(0, edit.replaceEndExclusive)
        assertEquals("abc", edit.newText)
        assertEquals(TextRange(3, 3), edit.selection)
    }

    @Test
    fun computeSingleReplace_pureDelete_collapsesToDeleteRange() {
        val edit =
            computeSingleReplace(
                oldText = "abc",
                newText = "a",
                selection = TextRange(1, 1),
            )
        assertEquals(1, edit.replaceStart)
        assertEquals(3, edit.replaceEndExclusive)
        assertEquals("", edit.newText)
    }

    @Test
    fun computeSingleReplace_middleReplace_keepsCommonPrefixAndSuffix() {
        val edit =
            computeSingleReplace(
                oldText = "ABCDE",
                newText = "AXDE",
                selection = TextRange(2, 2),
            )
        assertEquals(1, edit.replaceStart)
        assertEquals(3, edit.replaceEndExclusive)
        assertEquals("X", edit.newText)
    }

    @Test
    fun computeSingleReplace_cjkOffsetsAreUtf16CodeUnits() {
        // "你好" = 2 UTF-16 code units, 6 UTF-8 bytes。
        val edit =
            computeSingleReplace(
                oldText = "你好",
                newText = "你世界",
                selection = TextRange(3, 3),
            )
        assertEquals(1, edit.replaceStart)
        assertEquals(2, edit.replaceEndExclusive)
        assertEquals("世界", edit.newText)
    }

    @Test
    fun onInputSnapshot_withActiveComposition_doesNotCommitToCore() {
        var commitCalls = 0
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "",
                initialSelection = TextRange(0, 0),
                commitToCore = { _ ->
                    commitCalls++
                    CommitResult.Accepted
                },
            )

        // IME preedit：composition != null，不应进入 Core 事务。
        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = "预",
                selection = TextRange(0, 0),
                composition = TextRange(0, 1),
            ),
        )

        assertEquals("composition 中间态不得入 Core", 0, commitCalls)
        assertEquals("mirror 仍为初始空串", "", bridge.mirroredText)
    }

    @Test
    fun onInputSnapshot_afterCompositionFinished_commitsCommittedText() {
        val committedEdits = mutableListOf<CommittedTextEdit>()
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "",
                initialSelection = TextRange(0, 0),
                commitToCore = { edit ->
                    committedEdits += edit
                    CommitResult.Accepted
                },
            )

        // composition 结束后（composition == null），最终正文入 Core。
        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = "测试",
                selection = TextRange(2, 2),
                composition = null,
            ),
        )

        assertEquals(1, committedEdits.size)
        assertEquals("测试", committedEdits[0].newText)
        assertEquals("测试", bridge.mirroredText)
    }

    @Test
    fun onInputSnapshot_unchangedText_doesNotCommitToCore() {
        var commitCalls = 0
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "已有",
                initialSelection = TextRange(2, 2),
                commitToCore = { _ ->
                    commitCalls++
                    CommitResult.Accepted
                },
            )

        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = "已有",
                selection = TextRange(2, 2),
                composition = null,
            ),
        )

        assertEquals("text 未变不得重复提交", 0, commitCalls)
    }

    @Test
    fun flushForClose_commitsEvenWhenCompositionActive() {
        val committedEdits = mutableListOf<CommittedTextEdit>()
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "",
                initialSelection = TextRange(0, 0),
                commitToCore = { edit ->
                    committedEdits += edit
                    CommitResult.Accepted
                },
            )

        // 模拟 IME 仍有 composition 上屏：state 已被 IME 改写，但 onInputSnapshot
        // 因 composition != null 被跳过，Core 仍未收到提交。
        bridge.state.edit {
            replace(0, length, "候选上屏")
            this.selection = TextRange(4, 4)
        }
        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = bridge.state.text.toString(),
                selection = bridge.state.selection,
                composition = TextRange(0, 4),
            ),
        )
        assertTrue("composition 期间不应提交", committedEdits.isEmpty())

        // flushForClose 必须无视 composition 提交屏幕最终内容。
        bridge.flushForClose()

        assertEquals(1, committedEdits.size)
        assertEquals("候选上屏", committedEdits[0].newText)
        assertEquals("候选上屏", bridge.mirroredText)
    }

    @Test
    fun applyAuthoritativeText_overwritesStateAndMirror() {
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "旧正文",
                initialSelection = TextRange(3, 3),
                commitToCore = { CommitResult.Accepted },
            )

        bridge.applyAuthoritativeText(AUTHORITATIVE_TEXT, TextRange(0, 0))

        assertEquals(AUTHORITATIVE_TEXT, bridge.state.text.toString())
        assertEquals(TextRange(0, 0), bridge.state.selection)
        assertEquals(MIRROR_FOLLOWS_AUTHORITY, AUTHORITATIVE_TEXT, bridge.mirroredText)
    }

    @Test
    fun rejectedCommit_appliesAuthoritativeTextFromCore() {
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "",
                initialSelection = TextRange(0, 0),
                commitToCore = { _ ->
                    // Core 拒绝并回退到权威正文（同步/撤销覆盖）。
                    CommitResult.Rejected(
                        text = "权威正文",
                        selection = TextRange(4, 4),
                    )
                },
            )

        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = "用户输入",
                selection = TextRange(4, 4),
                composition = null,
            ),
        )

        assertEquals("Core 拒绝后 state 回退到权威正文", "权威正文", bridge.state.text.toString())
        assertEquals(TextRange(4, 4), bridge.state.selection)
        assertEquals(MIRROR_FOLLOWS_AUTHORITY, "权威正文", bridge.mirroredText)
    }

    @Test
    fun commitToCore_receivesUtf16Offsets_callerConvertsToUtf8() {
        // 验证调用方在 commitToCore lambda 内把 UTF-16 offset 转成 UTF-8 byte offset。
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "你好",
                initialSelection = TextRange(2, 2),
                commitToCore = { edit ->
                    // replaceStart/replaceEndExclusive 是 UTF-16；进 Core 前转 UTF-8。
                    val utf8Start = TextOffsetUtils.utf8OffsetForCharIndex(edit.oldText, edit.replaceStart)
                    val utf8End = TextOffsetUtils.utf8OffsetForCharIndex(edit.oldText, edit.replaceEndExclusive)
                    assertEquals("UTF-16 offset 1 → UTF-8 byte 3", 3, utf8Start)
                    assertEquals("UTF-16 offset 2 → UTF-8 byte 6", 6, utf8End)
                    CommitResult.Accepted
                },
            )

        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = "你世界",
                selection = TextRange(3, 3),
                composition = null,
            ),
        )
    }

    @Test
    fun initialState_reflectsInitialTextAndSelection() {
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "初始",
                initialSelection = TextRange(1, 1),
                commitToCore = { CommitResult.Accepted },
            )

        assertEquals("初始", bridge.state.text.toString())
        assertEquals(TextRange(1, 1), bridge.state.selection)
        assertEquals("初始", bridge.mirroredText)
        assertNull("初始无 composition", bridge.state.composition)
    }

    /**
     * #641 评论 5458880786 问题3：flushForClose 必须先消费 pending authoritative —
     * Undo 在 composition 期间到达会暂存 pending，flushForClose 若直接 commitIfNeeded
     * 会把旧 composition 文本重新 commit 回 Core，覆盖 Undo。
     */
    @Test
    fun flushForClose_withPendingAuthoritative_appliesPendingInsteadOfComposing() {
        val committedEdits = mutableListOf<CommittedTextEdit>()
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "原始正文",
                initialSelection = TextRange(4, 4),
                commitToCore = { edit ->
                    committedEdits += edit
                    CommitResult.Accepted
                },
            )

        // 模拟 IME composition 活跃期间用户输入。
        bridge.state.edit {
            replace(0, length, "原始正文候选")
            this.selection = TextRange(6, 6)
        }
        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = bridge.state.text.toString(),
                selection = bridge.state.selection,
                composition = TextRange(4, 6),
            ),
        )
        assertTrue("composition 期间不应提交", committedEdits.isEmpty())

        // Undo 在 composition 期间到达，Core 权威正文变为"撤销后正文"。
        // bridge 暂存 pending（composition 仍活跃）。
        bridge.storePendingAuthoritative(UNDO_RESTORED_TEXT, TextRange(4, 4))
        assertTrue("pending 已存", bridge.hasPendingAuthoritative())

        // flushForClose 必须先消费 pending authoritative，不把旧 composition 文本提交回 Core。
        bridge.flushForClose()

        assertEquals("flushForClose 后正文应为撤销后正文", UNDO_RESTORED_TEXT, bridge.state.text.toString())
        assertEquals(MIRROR_FOLLOWS_AUTHORITY, UNDO_RESTORED_TEXT, bridge.mirroredText)
        // 不应有 committedEdits（pending authoritative 直接 apply，不走 commitToCore）。
        assertTrue("pending authoritative 不走 commitToCore", committedEdits.isEmpty())
        assertFalse("pending 已消费", bridge.hasPendingAuthoritative())
    }

    /**
     * #641 评论 5458880786 问题3：onInputSnapshot 也先消费 pending authoritative —
     * composition 结束后第一个 snapshot 应先应用 pending，不提交本地 diff。
     */
    @Test
    fun onInputSnapshot_afterCompositionEnd_withPending_appliesPendingFirst() {
        val committedEdits = mutableListOf<CommittedTextEdit>()
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "原始",
                initialSelection = TextRange(2, 2),
                commitToCore = { edit ->
                    committedEdits += edit
                    CommitResult.Accepted
                },
            )

        // composition 期间 Undo 到达，暂存 pending。
        bridge.storePendingAuthoritative(UNDO_RESTORED_SHORT, TextRange(3, 3))
        assertTrue(bridge.hasPendingAuthoritative())

        // composition 结束后的第一个 snapshot — 应先消费 pending，不提交本地 diff。
        bridge.onInputSnapshot(
            EditorInputSnapshot(
                text = UNDO_RESTORED_SHORT,
                selection = TextRange(3, 3),
                composition = null,
            ),
        )

        assertEquals("正文应为撤销后", UNDO_RESTORED_SHORT, bridge.state.text.toString())
        assertEquals(MIRROR_FOLLOWS_AUTHORITY, UNDO_RESTORED_SHORT, bridge.mirroredText)
        assertTrue("pending 消费后不走 commitToCore", committedEdits.isEmpty())
        assertFalse("pending 已消费", bridge.hasPendingAuthoritative())
    }
}
