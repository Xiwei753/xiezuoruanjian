package com.xiwei.sujian.feature.project.ui

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.Locale

private const val WORDS_FORMAT_ZH = "%1\$d 字"
private const val WAN_FORMAT_ZH = "%2\$.1f 万字"
private const val WORDS_FORMAT_EN = "%1\$d words"

class ProjectNavigationRestoreTest {
    @Test
    fun initialHistory_noProject_projectListOnly() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ProjectList)
        assertEquals(1, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
    }

    @Test
    fun initialHistory_hasProjectNoChapter_projectListAndChapterTree() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ChapterTree("p1"))
        assertEquals(2, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
        assertEquals("p1", (chain[1] as WorkspacePaneKey.ChapterTree).projectId)
    }

    @Test
    fun initialHistory_hasProjectAndChapter_fullChain() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        assertEquals(3, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
        assertTrue(chain[2] is WorkspacePaneKey.Editor)
        val editor = chain[2] as WorkspacePaneKey.Editor
        assertEquals("p1", editor.projectId)
        assertEquals("v1", editor.volumeId)
        assertEquals("c1", editor.chapterId)
    }

    @Test
    fun deriveLocation_consistentWithPaneKey_chainRoundTrip() {
        val keys =
            listOf(
                WorkspacePaneKey.ProjectList,
                WorkspacePaneKey.ChapterTree("p1"),
                WorkspacePaneKey.Editor("p1", "v1", "c1"),
            )
        val locations = keys.map { deriveWorkspaceLocation(it) }
        assertTrue(locations[0] is WorkspaceLocation.ProjectList)
        assertTrue(locations[1] is WorkspaceLocation.ChapterTree)
        assertTrue(locations[2] is WorkspaceLocation.Editor)
    }
}

class SessionRestoreStateTest {
    @Test
    fun loadingState_isDistinctFromReady() {
        val loading: SessionRestoreState = SessionRestoreState.Loading
        val ready: SessionRestoreState = SessionRestoreState.Ready(SessionRestoreState.Destination.ProjectList)
        assert(loading != ready)
    }

    @Test
    fun readyEditor_holdsAllIds() {
        val ready = SessionRestoreState.Ready(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        val destination = (ready as SessionRestoreState.Ready).destination as SessionRestoreState.Destination.Editor
        assertEquals("p1", destination.projectId)
        assertEquals("v1", destination.volumeId)
        assertEquals("c1", destination.chapterId)
    }

    @Test
    fun readyProjectList_holdsNoIds() {
        val ready = SessionRestoreState.Ready(SessionRestoreState.Destination.ProjectList)
        assertTrue((ready as SessionRestoreState.Ready).destination is SessionRestoreState.Destination.ProjectList)
    }
}

class DeriveRestoreDestinationTest {
    @Test
    fun noProject_projectList() {
        assertEquals(
            SessionRestoreState.Destination.ProjectList,
            deriveRestoreDestination(null, null, null),
        )
    }

    @Test
    fun projectOnly_chapterTree() {
        val dest = deriveRestoreDestination("p1", null, null)
        assertTrue(dest is SessionRestoreState.Destination.ChapterTree)
        assertEquals("p1", (dest as SessionRestoreState.Destination.ChapterTree).projectId)
    }

    @Test
    fun projectAndVolumeAndChapter_editor() {
        val dest = deriveRestoreDestination("p1", "v1", "c1")
        assertTrue(dest is SessionRestoreState.Destination.Editor)
        val editor = dest as SessionRestoreState.Destination.Editor
        assertEquals("p1", editor.projectId)
        assertEquals("v1", editor.volumeId)
        assertEquals("c1", editor.chapterId)
    }
}

/**
 * #625 第二段：[WorkspaceNavigator] 业务行为单测 — 验证历史栈、canNavigateBack、back。
 */
class WorkspaceNavigatorTest {
    @Test
    fun replaceInitialHistory_emptyNavigator_appendsHistory() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            val initialHistory =
                listOf(
                    WorkspacePaneKey.ProjectList,
                    WorkspacePaneKey.ChapterTree("p1"),
                )
            navigator.replaceInitialHistory(initialHistory)
            assertEquals(2, navigator.history.size)
            assertEquals(WorkspaceLocation.ChapterTree("p1"), navigator.currentLocation)
            assertTrue(navigator.canNavigateBack)
        }

    @Test
    fun replaceInitialHistory_nonEmptyNavigator_doesNotOverwrite() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            navigator.replaceInitialHistory(listOf(WorkspacePaneKey.ProjectList))
            // 第二次调用不应覆盖已有历史。
            navigator.replaceInitialHistory(
                listOf(WorkspacePaneKey.ProjectList, WorkspacePaneKey.ChapterTree("p1")),
            )
            assertEquals(1, navigator.history.size)
        }

    @Test
    fun navigateTo_appendsKey() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            navigator.replaceInitialHistory(listOf(WorkspacePaneKey.ProjectList))
            navigator.navigateTo(WorkspacePaneKey.ChapterTree("p1"))
            assertEquals(2, navigator.history.size)
            assertTrue(navigator.currentLocation is WorkspaceLocation.ChapterTree)
        }

    @Test
    fun back_atRoot_returnsFalse() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            navigator.replaceInitialHistory(listOf(WorkspacePaneKey.ProjectList))
            assertFalse(navigator.back())
        }

    @Test
    fun back_withHistory_popsAndReturnsTrue() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            navigator.replaceInitialHistory(
                listOf(WorkspacePaneKey.ProjectList, WorkspacePaneKey.ChapterTree("p1")),
            )
            assertTrue(navigator.back())
            assertEquals(1, navigator.history.size)
            assertTrue(navigator.currentLocation is WorkspaceLocation.ProjectList)
        }

    @Test
    fun seekBack_isNoOp_doesNotThrow() =
        kotlinx.coroutines.test.runTest {
            val navigator = WorkspaceNavigator()
            navigator.replaceInitialHistory(
                listOf(WorkspacePaneKey.ProjectList, WorkspacePaneKey.ChapterTree("p1")),
            )
            // seekBack 是空实现 — 不应抛异常，不应改变历史。
            navigator.seekBack(0.5f)
            assertEquals(2, navigator.history.size)
            navigator.seekBack(0f)
            assertEquals(2, navigator.history.size)
        }

    private fun assertFalse(actual: Boolean) = org.junit.Assert.assertFalse(actual)
}

/**
 * #625 项7：[formatProjectWordCount] 字数格式化单测。
 *
 * [formatProjectWordCount] 接收 i18n 格式串，测试传入中文格式串验证格式化逻辑。
 * 固定 Locale.US 保证 `%.1f` 小数点为 `.`，不受 CI 默认 locale 影响。
 */
class FormatProjectWordCountTest {
    private val savedLocale = Locale.getDefault()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
    }

    @After
    fun tearDown() {
        Locale.setDefault(savedLocale)
    }

    @Test
    fun lessThanTenThousand_showsRawCount() {
        assertEquals("0 字", formatProjectWordCount(0, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
        assertEquals("1234 字", formatProjectWordCount(1234, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
        assertEquals("9999 字", formatProjectWordCount(9999, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
    }

    @Test
    fun tenThousandOrMore_showsWanUnit() {
        assertEquals("1.0 万字", formatProjectWordCount(10_000, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
        assertEquals("1.2 万字", formatProjectWordCount(12_000, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
        assertEquals("10.0 万字", formatProjectWordCount(100_000, WORDS_FORMAT_ZH, WAN_FORMAT_ZH))
    }

    @Test
    fun englishLocale_doesNotUseWanUnit() {
        // 英文不区分万字：wanFormat 也用 "%1$d words"，始终显示原数。
        assertEquals("0 words", formatProjectWordCount(0, WORDS_FORMAT_EN, WORDS_FORMAT_EN))
        assertEquals("1234 words", formatProjectWordCount(1234, WORDS_FORMAT_EN, WORDS_FORMAT_EN))
        assertEquals("10000 words", formatProjectWordCount(10_000, WORDS_FORMAT_EN, WORDS_FORMAT_EN))
        assertEquals("100000 words", formatProjectWordCount(100_000, WORDS_FORMAT_EN, WORDS_FORMAT_EN))
    }
}
