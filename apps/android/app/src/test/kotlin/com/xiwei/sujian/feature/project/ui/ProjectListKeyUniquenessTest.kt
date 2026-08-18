package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

// detekt StringLiteralDuplication：测试夹具 UUID 在多处复用，提取为文件级常量。
private const val FIXTURE_PROJECT_UUID_9EE6701D = "9ee6701d-24f5-4716-9e9c-55f2802fd12a"

/**
 * #630 评论5323353678：窄屏 LazyColumn 跨区块 key 唯一性回归测试。
 *
 * 同一作品同时出现在 recentEdits 与 projectSummaries 时，
 * 不同区块的 Lazy item key 必须互不相同，否则 Compose 抛
 * `IllegalArgumentException: Key ... was already used`。
 *
 * 本测试直接调用 production 入口（[recentEditItemKey] / [projectItemKey] /
 * [RECENT_EDITS_HEADER_KEY] / [ALL_PROJECTS_HEADER_KEY]），验证：
 * 1. 同一 UUID 在 recent/all 两区块的 key 唯一；
 * 2. header 常量不与任何 item key 冲突；
 * 3. 排序/插入后 key 集合仍无重复。
 *
 * **不**为测试重复实现 key 逻辑；若生产 key 回退成裸 UUID，本测试会正确失败。
 */
class ProjectListKeyUniquenessTest {
    @Test
    fun recentEditKey_hasRecentPrefix() {
        val edit = makeRecentEdit(FIXTURE_PROJECT_UUID_9EE6701D)
        assertEquals(
            "recent:9ee6701d-24f5-4716-9e9c-55f2802fd12a",
            recentEditItemKey(edit),
        )
    }

    @Test
    fun projectItemKey_hasProjectPrefix() {
        val summary = makeProjectSummary(FIXTURE_PROJECT_UUID_9EE6701D)
        assertEquals(
            "project:9ee6701d-24f5-4716-9e9c-55f2802fd12a",
            projectItemKey(summary),
        )
    }

    @Test
    fun sameProjectInBothSections_keysAreDifferent() {
        val projectId = FIXTURE_PROJECT_UUID_9EE6701D
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
    fun headerKeys_doNotCollideWithItemKeys() {
        val edit = makeRecentEdit(RECENT_EDITS_HEADER_KEY)
        val summary = makeProjectSummary(ALL_PROJECTS_HEADER_KEY)

        val recentItemKey = recentEditItemKey(edit)
        val projectItemK = projectItemKey(summary)

        assertTrue(
            "header key '$RECENT_EDITS_HEADER_KEY' must not equal item key '$recentItemKey'",
            RECENT_EDITS_HEADER_KEY != recentItemKey,
        )
        assertTrue(
            "header key '$ALL_PROJECTS_HEADER_KEY' must not equal item key '$projectItemK'",
            ALL_PROJECTS_HEADER_KEY != projectItemK,
        )
        assertTrue(
            "header keys must differ from each other",
            RECENT_EDITS_HEADER_KEY != ALL_PROJECTS_HEADER_KEY,
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
        allKeys.add(RECENT_EDITS_HEADER_KEY)
        recentEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(ALL_PROJECTS_HEADER_KEY)
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
        allKeys.add(RECENT_EDITS_HEADER_KEY)
        recentEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(ALL_PROJECTS_HEADER_KEY)
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
        allKeys.add(RECENT_EDITS_HEADER_KEY)
        sortedEdits.forEach { allKeys.add(recentEditItemKey(it)) }
        allKeys.add(ALL_PROJECTS_HEADER_KEY)
        sortedEdits.forEach {
            allKeys.add(projectItemKey(makeProjectSummary(it.projectId)))
        }

        val distinctKeys = allKeys.toSet()
        assertEquals(allKeys.size, distinctKeys.size)

        val recentKeys = sortedEdits.map { recentEditItemKey(it) }
        assertEquals("recent:a-project", recentKeys[0])
        assertEquals("recent:b-project", recentKeys[1])
        assertEquals("recent:c-project", recentKeys[2])
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
