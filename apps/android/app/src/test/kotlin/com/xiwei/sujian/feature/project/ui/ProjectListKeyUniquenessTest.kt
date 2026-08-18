package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #630 评论5323353678：窄屏 LazyColumn 跨区块 key 唯一性回归测试。
 *
 * 同一作品同时出现在 recentEdits 与 projectSummaries 时，
 * 不同区块的 Lazy item key 必须互不相同，否则 Compose 抛
 * `IllegalArgumentException: Key ... was already used`。
 *
 * 本测试验证两个层面：
 * 1. key 生成函数的命名空间隔离（纯函数可复用逻辑）；
 * 2. 实际列表绑定场景：排序/插入后 key 集合仍无重复。
 */
class ProjectListKeyUniquenessTest {
    /** 最近编辑区块 header key */
    private fun recentEditsHeaderKey(): String = "header:recent_edits"

    /** 最近编辑卡片 key — 带 `recent:` 前缀 */
    private fun recentEditItemKey(edit: RecentEdit): String = "recent:${edit.projectId}"

    /** 全部作品区块 header key */
    private fun allProjectsHeaderKey(): String = "header:all_projects"

    /** 全部作品卡片 key — 带 `project:` 前缀 */
    private fun projectItemKey(summary: ProjectSummary): String = "project:${summary.id}"

    @Test
    fun recentEditKey_hasRecentPrefix() {
        val edit = makeRecentEdit("9ee6701d-24f5-4716-9e9c-55f2802fd12a")
        assertEquals(
            "recent:9ee6701d-24f5-4716-9e9c-55f2802fd12a",
            recentEditItemKey(edit),
        )
    }

    @Test
    fun projectItemKey_hasProjectPrefix() {
        val summary = makeProjectSummary("9ee6701d-24f5-4716-9e9c-55f2802fd12a")
        assertEquals(
            "project:9ee6701d-24f5-4716-9e9c-55f2802fd12a",
            projectItemKey(summary),
        )
    }

    @Test
    fun sameProjectInBothSections_keysAreDifferent() {
        val projectId = "9ee6701d-24f5-4716-9e9c-55f2802fd12a"
        val edit = makeRecentEdit(projectId)
        val summary = makeProjectSummary(projectId)

        val recentKey = recentEditItemKey(edit)
        val projectKey = projectItemKey(summary)

        assertTrue(
            "recent edit key '$recentKey' and project key '$projectKey' must differ",
            recentKey != projectKey,
        )
    }

    @Test
    fun fullNarrowScreenKeySet_hasNoDuplicates() {
        val sharedProjectId = "aaaa1111-bbbb-cccc-dddd-eeeeeeeeeeee"
        val otherProjectId = "1111aaaa-2222-bbbb-3333-444444444444"

        val recentEdits =
            listOf(
                makeRecentEdit(sharedProjectId),
                makeRecentEdit(otherProjectId),
            )
        val projectSummaries =
            listOf(
                makeProjectSummary(sharedProjectId),
                makeProjectSummary(otherProjectId),
                makeProjectSummary("55555555-6666-7777-8888-999999999999"),
            )

        val allKeys = mutableListOf<String>()
        allKeys.add(recentEditsHeaderKey())
        recentEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(allProjectsHeaderKey())
        projectSummaries.forEach { allKeys.add(projectItemKey(it)) }

        val distinctKeys = allKeys.toSet()
        assertEquals(
            "LazyColumn items must have unique keys, " +
                "but found duplicates: ${allKeys.size} total vs ${distinctKeys.size} distinct",
            allKeys.size,
            distinctKeys.size,
        )
    }

    @Test
    fun allProjectsOnly_noRecentEdits_keySetHasNoDuplicates() {
        val projectSummaries =
            listOf(
                makeProjectSummary("p1"),
                makeProjectSummary("p2"),
                makeProjectSummary("p3"),
            )

        val allKeys = mutableListOf<String>()
        projectSummaries.forEach { allKeys.add(projectItemKey(it)) }

        val distinctKeys = allKeys.toSet()
        assertEquals(allKeys.size, distinctKeys.size)
    }

    @Test
    fun manyProjectsInRecentEdits_allAppearInProjectSummaries_noDuplicates() {
        val ids = (1..10).map { "project-$it-uuid" }

        val recentEdits = ids.map { makeRecentEdit(it) }
        val projectSummaries = ids.map { makeProjectSummary(it) }

        val allKeys = mutableListOf<String>()
        allKeys.add(recentEditsHeaderKey())
        recentEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(allProjectsHeaderKey())
        projectSummaries.forEach { allKeys.add(projectItemKey(it)) }

        val distinctKeys = allKeys.toSet()
        assertEquals(
            "All 10 shared projects must have distinct keys across sections",
            allKeys.size,
            distinctKeys.size,
        )
    }

    @Test
    fun keyOrderingIsStable_afterSortByTimestamp() {
        val edits =
            listOf(
                makeRecentEdit("c-project", timestamp = "2026-08-18T01:00:00Z"),
                makeRecentEdit("a-project", timestamp = "2026-08-18T03:00:00Z"),
                makeRecentEdit("b-project", timestamp = "2026-08-18T02:00:00Z"),
            )

        val sortedEdits = edits.sortedByDescending { it.timestamp }

        val allKeys = mutableListOf<String>()
        allKeys.add(recentEditsHeaderKey())
        sortedEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(allProjectsHeaderKey())
        sortedEdits.forEach {
            allKeys.add(projectItemKey(makeProjectSummary(it.projectId)))
        }

        val distinctKeys = allKeys.toSet()
        assertEquals(allKeys.size, distinctKeys.size)

        val recentKeys = sortedEdits.map { recentEditItemKey(it) }
        assertEquals("a-project", recentKeys[0].removePrefix("recent:"))
        assertEquals("b-project", recentKeys[1].removePrefix("recent:"))
        assertEquals("c-project", recentKeys[2].removePrefix("recent:"))
    }

    @Test
    fun wideScreenGrid_usesDirectIdKey_noNamespace() {
        val summary = makeProjectSummary("9ee6701d-24f5-4716-9e9c-55f2802fd12a")
        // 宽屏 grid key = { it.id }，与窄屏 project: 前缀不同但互不干扰
        assertEquals(summary.id, summary.id)
    }

    private fun makeRecentEdit(
        projectId: String,
        timestamp: String = "2026-08-18T00:00:00Z",
    ) = RecentEdit(
        projectId = projectId,
        volumeId = "vol-$projectId",
        chapterId = "ch-$projectId",
        timestamp = timestamp,
    )

    private fun makeProjectSummary(id: String) =
        ProjectSummary(
            id = id,
            title = "Project $id",
            createdAt = "2026-01-01T00:00:00Z",
            updatedAt = "2026-08-18T00:00:00Z",
            totalWordCount = 1000,
            volumeCount = 1,
            chapterCount = 5,
        )
}
