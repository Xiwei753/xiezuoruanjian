package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 会话恢复 → navigator 初始历史契约：
 * 只有 [SessionRestoreState.Ready] 后才构建历史；每个目的地映射唯一固定链，
 * 与 PhonePortraitShell 共用生产实现 [buildInitialHistory]，测试不复制第二份逻辑。
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
class SessionRestoreNavigatorContractTest {

    @Test
    fun loadingState_isNotReady() {
        val state: SessionRestoreState = SessionRestoreState.Loading
        assertFalse(state is SessionRestoreState.Ready)
    }

    @Test
    fun readyProjectList_buildsSingleDestinationHistory() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ProjectList)
        assertEquals(1, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
    }

    @Test
    fun readyChapterTree_buildsProjectListAndChapterTree() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ChapterTree("p1"))
        assertEquals(2, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
        assertTrue(chain[1].contentKey is WorkspacePaneKey.ChapterTree)
        assertEquals("p1", (chain[1].contentKey as WorkspacePaneKey.ChapterTree).projectId)
    }

    @Test
    fun readyEditor_buildsFullHistory() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        assertEquals(3, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
        assertEquals(WorkspacePaneKey.ChapterTree("p1"), chain[1].contentKey)
        assertEquals(WorkspacePaneKey.Editor("p1", "v1", "c1"), chain[2].contentKey)
    }

    @Test
    fun loadingToReady_transitionRebuildsHistory() {
        val loading: SessionRestoreState = SessionRestoreState.Loading
        assertFalse(loading is SessionRestoreState.Ready)
        val ready: SessionRestoreState = SessionRestoreState.Ready(
            SessionRestoreState.Destination.Editor("p1", "v1", "c1"),
        )
        assertTrue(ready is SessionRestoreState.Ready)
        val chain = buildInitialHistory((ready as SessionRestoreState.Ready).destination)
        assertEquals(3, chain.size)
    }

    @Test
    fun historyRoles_followPaneScaffoldRoles() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        assertEquals(WorkspacePaneKey.ProjectList.role, chain[0].pane)
        assertEquals(WorkspacePaneKey.ChapterTree("p1").role, chain[1].pane)
        assertEquals(WorkspacePaneKey.Editor("p1", "v1", "c1").role, chain[2].pane)
    }
}
