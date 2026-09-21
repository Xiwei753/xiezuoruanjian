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
 * #732 评论第5节：首页契约 singular — recentEdit 只画一个卡片，key 为 `"recent:$projectId"`。
 * 本测试验证：
 * 1. 单个 recentEdit item key 不与同一作品的 project item key 冲突；
 * 2. header 常量不与任何 item key 冲突；
 * 3. 仅 projects 区块时 key 集合无重复。
 *
 * 本测试直接调用 production 入口（[projectItemKey] /
 * [RECENT_EDITS_HEADER_KEY] / [ALL_PROJECTS_HEADER_KEY]）。
 */
class ProjectListKeyUniquenessTest {
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

        // #732 评论第5节：recentEdit 单卡片 key 内联为 "recent:$projectId"。
        val recentKey = "recent:${edit.projectId}"
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

        val recentItemKey = "recent:${edit.projectId}"
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

        // #732 评论第5节：recentEdit 单值 — 只有一个 recent item。
        val recentEdit = makeRecentEdit(sharedProjectId)
        val projectSummaries =
            listOf(
                makeProjectSummary(sharedProjectId),
                makeProjectSummary("1111aaaa-2222-bbbb-3333-444444444444"),
                makeProjectSummary("55555555-6666-7777-8888-999999999999"),
            )

        val allKeys = mutableListOf<String>()
        allKeys.add(RECENT_EDITS_HEADER_KEY)
        allKeys.add("recent:${recentEdit.projectId}")
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
    fun allProjectsOnly_noRecentEdit_keySetHasNoDuplicates() {
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
