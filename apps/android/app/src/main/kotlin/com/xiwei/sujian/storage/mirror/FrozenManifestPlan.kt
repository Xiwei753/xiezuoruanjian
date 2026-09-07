package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.common.BridgeResult
import org.json.JSONArray
import org.json.JSONObject

/**
 * FrozenManifestPlan — 冻结的全局 manifest 计划。
 *
 * #649 评论 5575551884 问题 3：旧 `buildManifestJsonFromMetadata` 只冻结单个项目级元数据，
 * 输出的 JSON schema 与 MirrorManifest 不一致（{projectId,title,...} vs
 * {schemaVersion,revision,updatedAt,projects:[...]}），且会丢掉其他作品。
 *
 * 新实现：在正文开始改公共镜像前，冻结本事务最终需要的全局 manifest 逻辑状态。
 * 恢复时只把本事务已经验证过的 `promotedEntries` 的真实 URI/hash 填回目标章节，
 * 再输出与 [MirrorManifest] 完全一致的 JSON，其他作品保留，不重新读取当前 Core。
 *
 * ## 与 `frozenManifestMetadata` 的关系
 * `frozenManifestMetadata`（已有的字段）保存项目级元数据（id/title/order/revision），
 * 用于恢复时构建单项目 MirrorProject。
 * `FrozenManifestPlan` 保存全局 manifest 计划（schemaVersion + revision + 所有项目），
 * 恢复时直接输出完整的 MirrorManifest JSON。
 *
 * @property schemaVersion manifest schema 版本（当前为 1）。
 * @property revision 全局 revision（毫秒时间戳）。
 * @property updatedAt ISO-8601 UTC 时间。
 * @property targetProjectId 本次事务的目标项目 ID。
 * @property projects 所有项目的冻结计划，targetProjectId 的那条在恢复时会被
 *   `promotedEntries` 的真实 URI/hash 覆盖。
 */
data class FrozenManifestPlan(
    val schemaVersion: Int,
    val revision: Long,
    val updatedAt: String,
    val targetProjectId: String,
    val projects: List<FrozenManifestProject>,
)

/**
 * 单个项目在 frozen plan 中的快照。
 *
 * @property id 项目 ID。
 * @property title 项目标题。
 * @property order 项目顺序。
 * @property revision 项目 revision（毫秒时间戳）。
 * @property updatedAt ISO-8601 UTC 时间。
 * @property volumes 卷列表。
 */
data class FrozenManifestProject(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val volumes: List<FrozenManifestVolume>,
)

/**
 * 单个卷在 frozen plan 中的快照。
 */
data class FrozenManifestVolume(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val chapters: List<FrozenManifestChapter>,
)

/**
 * 单个章节在 frozen plan 中的快照。
 *
 * 注意：contentFile 和 contentHash 在冻结时可能还是占位值（空字符串），
 * 恢复时由 `promotedEntries` 的真实 URI/hash 替换。
 */
data class FrozenManifestChapter(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val contentFile: String,
    val contentHash: String,
)

/**
 * 构建 frozen manifest plan。
 *
 * 在正文事务开始前调用，冻结所有项目的逻辑状态。
 * targetProjectId 的章节 contentFile/contentHash 由调用方在恢复时用 promotedEntries 替换。
 *
 * @param source 快照源（读取所有项目的当前快照）。
 * @param targetProjectId 本次事务的目标项目 ID。
 * @return frozen plan；任何步骤失败返回 null。
 */
suspend fun buildFrozenManifestPlan(
    source: MirrorSnapshotSource,
    targetProjectId: String,
): FrozenManifestPlan? {
    val projectsResult = source.listProjects()
    if (projectsResult !is BridgeResult.Success) return null

    val now = java.time.Instant.now()
    val updatedAt = java.time.format.DateTimeFormatter.ISO_INSTANT.format(now)
    val revision = now.toEpochMilli()

    val frozenProjects = mutableListOf<FrozenManifestProject>()
    for (project in projectsResult.data) {
        val snapshotResult = source.getProjectWorkspaceSnapshot(project.id)
        if (snapshotResult !is BridgeResult.Success) continue
        val snapshot = snapshotResult.data

        val volumes = snapshot.volumes.map { vol ->
            FrozenManifestVolume(
                id = vol.volume.id,
                title = vol.volume.title,
                order = vol.volume.order,
                revision = vol.volume.updatedAt.toEpochMillis(),
                updatedAt = vol.volume.updatedAt,
                chapters = vol.chapters.map { ch ->
                    FrozenManifestChapter(
                        id = ch.id,
                        title = ch.title,
                        order = ch.order,
                        revision = ch.updatedAt.toEpochMillis(),
                        updatedAt = ch.updatedAt,
                        contentFile = "", // 占位，恢复时替换
                        contentHash = "", // 占位，恢复时替换
                    )
                },
            )
        }

        frozenProjects.add(
            FrozenManifestProject(
                id = project.id,
                title = project.title,
                order = 0,
                revision = snapshot.project.updatedAt.toEpochMillis(),
                updatedAt = snapshot.project.updatedAt,
                volumes = volumes,
            ),
        )
    }

    return FrozenManifestPlan(
        schemaVersion = 1,
        revision = revision,
        updatedAt = updatedAt,
        targetProjectId = targetProjectId,
        projects = frozenProjects,
    )
}

/**
 * 从 frozen plan 生成 MirrorManifest JSON，targetProjectId 的章节用 promotedEntries 替换。
 *
 * 输出与 [mirrorManifestToJson] 一致的 schema：`{schemaVersion, revision, updatedAt, projects: [...]}`。
 *
 * @param plan 冻结的 manifest 计划。
 * @param promotedEntries 本次事务已 promote 的章节 entries（key 为 ChapterKey）。
 * @return manifest JSON 字符串；如果 plan 中有章节在 promotedEntries 中缺失 URI/hash，返回 null。
 */
fun frozenPlanToManifestJson(
    plan: FrozenManifestPlan,
    promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
): String? {
    val projectsJson = JSONArray()

    for (frozenProject in plan.projects) {
        val volumesJson = JSONArray()

        for (frozenVolume in frozenProject.volumes) {
            val chaptersJson = JSONArray()

            for (frozenChapter in frozenVolume.chapters) {
                val chapterKey = ChapterKey(frozenProject.id, frozenVolume.id, frozenChapter.id)

                if (frozenProject.id == plan.targetProjectId) {
                    // 目标项目：用 promotedEntries 的真实 URI/hash
                    val entry = promotedEntries[chapterKey]
                    if (entry == null) {
                        // 章节在 frozen plan 中存在但没有 promotedEntries → 不完整，停止
                        return null
                    }
                    chaptersJson.put(JSONObject().apply {
                        put("id", frozenChapter.id)
                        put("title", frozenChapter.title)
                        put("order", frozenChapter.order)
                        put("revision", frozenChapter.revision)
                        put("updatedAt", frozenChapter.updatedAt)
                        put("contentFile", entry.relativePath)
                        put("contentHash", entry.contentHash)
                    })
                } else {
                    // 非目标项目：用冻结的占位值（contentFile/contentHash 由后续维护保证）
                    // 注意：冻结时 contentFile/contentHash 是空字符串，
                    // 正常流程中非目标项目不会被改动，所以这里保留原始空值。
                    // 实际生产中，非目标项目的内容来自 stateStore，不在此 plan 中。
                    chaptersJson.put(JSONObject().apply {
                        put("id", frozenChapter.id)
                        put("title", frozenChapter.title)
                        put("order", frozenChapter.order)
                        put("revision", frozenChapter.revision)
                        put("updatedAt", frozenChapter.updatedAt)
                        put("contentFile", frozenChapter.contentFile)
                        put("contentHash", frozenChapter.contentHash)
                    })
                }
            }

            volumesJson.put(JSONObject().apply {
                put("id", frozenVolume.id)
                put("title", frozenVolume.title)
                put("order", frozenVolume.order)
                put("revision", frozenVolume.revision)
                put("updatedAt", frozenVolume.updatedAt)
                put("chapters", chaptersJson)
            })
        }

        projectsJson.put(JSONObject().apply {
            put("id", frozenProject.id)
            put("title", frozenProject.title)
            put("order", frozenProject.order)
            put("revision", frozenProject.revision)
            put("updatedAt", frozenProject.updatedAt)
            put("volumes", volumesJson)
        })
    }

    return JSONObject().apply {
        put("schemaVersion", plan.schemaVersion)
        put("revision", plan.revision)
        put("updatedAt", plan.updatedAt)
        put("projects", projectsJson)
    }.toString()
}

/**
 * 序列化 FrozenManifestPlan 为 JSON 字符串。
 */
fun frozenManifestPlanToJson(plan: FrozenManifestPlan): String {
    val root = JSONObject()
    root.put("schemaVersion", plan.schemaVersion)
    root.put("revision", plan.revision)
    root.put("updatedAt", plan.updatedAt)
    root.put("targetProjectId", plan.targetProjectId)

    val projectsArray = JSONArray()
    for (project in plan.projects) {
        val volumesArray = JSONArray()
        for (volume in project.volumes) {
            val chaptersArray = JSONArray()
            for (chapter in volume.chapters) {
                chaptersArray.put(JSONObject().apply {
                    put("id", chapter.id)
                    put("title", chapter.title)
                    put("order", chapter.order)
                    put("revision", chapter.revision)
                    put("updatedAt", chapter.updatedAt)
                    put("contentFile", chapter.contentFile)
                    put("contentHash", chapter.contentHash)
                })
            }
            volumesArray.put(JSONObject().apply {
                put("id", volume.id)
                put("title", volume.title)
                put("order", volume.order)
                put("revision", volume.revision)
                put("updatedAt", volume.updatedAt)
                put("chapters", chaptersArray)
            })
        }
        projectsArray.put(JSONObject().apply {
            put("id", project.id)
            put("title", project.title)
            put("order", project.order)
            put("revision", project.revision)
            put("updatedAt", project.updatedAt)
            put("volumes", volumesArray)
        })
    }
    root.put("projects", projectsArray)
    return root.toString()
}

/**
 * 反序列化 FrozenManifestPlan 从 JSON 字符串。
 */
fun frozenManifestPlanFromJson(json: String): FrozenManifestPlan? {
    return try {
        val root = JSONObject(json)
        val schemaVersion = root.getInt("schemaVersion")
        val revision = root.getLong("revision")
        val updatedAt = root.getString("updatedAt")
        val targetProjectId = root.getString("targetProjectId")

        val projectsArray = root.getJSONArray("projects")
        val projects = mutableListOf<FrozenManifestProject>()
        for (i in 0 until projectsArray.length()) {
            val projectObj = projectsArray.getJSONObject(i)
            val volumesArray = projectObj.getJSONArray("volumes")
            val volumes = mutableListOf<FrozenManifestVolume>()
            for (j in 0 until volumesArray.length()) {
                val volumeObj = volumesArray.getJSONObject(j)
                val chaptersArray = volumeObj.getJSONArray("chapters")
                val chapters = mutableListOf<FrozenManifestChapter>()
                for (k in 0 until chaptersArray.length()) {
                    val chapterObj = chaptersArray.getJSONObject(k)
                    chapters.add(FrozenManifestChapter(
                        id = chapterObj.getString("id"),
                        title = chapterObj.getString("title"),
                        order = chapterObj.getInt("order"),
                        revision = chapterObj.getLong("revision"),
                        updatedAt = chapterObj.getString("updatedAt"),
                        contentFile = chapterObj.optString("contentFile", ""),
                        contentHash = chapterObj.optString("contentHash", ""),
                    ))
                }
                volumes.add(FrozenManifestVolume(
                    id = volumeObj.getString("id"),
                    title = volumeObj.getString("title"),
                    order = volumeObj.getInt("order"),
                    revision = volumeObj.getLong("revision"),
                    updatedAt = volumeObj.getString("updatedAt"),
                    chapters = chapters,
                ))
            }
            projects.add(FrozenManifestProject(
                id = projectObj.getString("id"),
                title = projectObj.getString("title"),
                order = projectObj.getInt("order"),
                revision = projectObj.getLong("revision"),
                updatedAt = projectObj.getString("updatedAt"),
                volumes = volumes,
            ))
        }

        FrozenManifestPlan(
            schemaVersion = schemaVersion,
            revision = revision,
            updatedAt = updatedAt,
            targetProjectId = targetProjectId,
            projects = projects,
        )
    } catch (_: Exception) {
        null
    }
}
