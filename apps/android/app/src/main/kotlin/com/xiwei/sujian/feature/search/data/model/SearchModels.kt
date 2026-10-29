package com.xiwei.sujian.feature.search.data.model

import com.google.gson.Gson

/**
 * 全局搜索范围 — 与 Core `SearchScope` 的 camelCase 字符串一一对应。
 *
 * UI 不自行解析/过滤搜索结果，范围只作为 Core 查询参数。
 */
object SearchScopes {
    const val ChapterBody = "chapterBody"
    const val ChapterTitle = "chapterTitle"
    const val ChapterNote = "chapterNote"
    const val ProjectTitle = "projectTitle"
    const val VolumeTitle = "volumeTitle"
    const val StarmapTitle = "starmapTitle"
    const val StarmapNode = "starmapNode"
    const val StarmapEdgeLabel = "starmapEdgeLabel"
    const val StarmapLink = "starmapLink"
    const val StarmapEmbed = "starmapEmbed"
    const val Setting = "setting"
    const val All = "all"
}

/** 搜索来源屏幕 — 与 Core `SearchOriginScreen` 的 camelCase 字符串一一对应。 */
object SearchOriginScreens {
    const val General = "general"
    const val Home = "home"
    const val Project = "project"
    const val Writing = "writing"
    const val Starmap = "starmap"
    const val Settings = "settings"
}

/**
 * 发起搜索时的页面上下文 — 只参与 Core 排序加权，不过滤结果。
 *
 * 字段名使用 camelCase，与 Core `SearchContext` 的 serde 契约一致。
 */
data class SearchContext(
    val originScreen: String = SearchOriginScreens.General,
    val currentProjectId: String? = null,
    val currentVolumeId: String? = null,
    val currentChapterId: String? = null,
    val currentStarmapId: String? = null,
    val currentSettingsSection: String? = null,
) {
    fun toJson(): String = gson.toJson(this)

    private companion object {
        val gson = Gson()
    }
}

/** Core `SearchTarget` 的 JSON 映射。 */
data class SearchTarget(
    val projectId: String? = null,
    val volumeId: String? = null,
    val chapterId: String? = null,
    val starmapId: String? = null,
    val nodeId: String? = null,
    val edgeId: String? = null,
    val linkId: String? = null,
    val embedId: String? = null,
    val settingKey: String? = null,
    val settingSection: String? = null,
)

/** Core `SearchResult` 的 JSON 映射。 */
data class SearchHit(
    val objectId: String = "",
    val title: String = "",
    val path: String = "",
    val summary: String = "",
    val byteStart: Int = 0,
    val byteEnd: Int = 0,
    val summaryMatchRanges: List<List<Int>> = emptyList(),
    val field: String = "",
    val score: Double = 0.0,
    val scope: String = "",
    val target: SearchTarget = SearchTarget(),
) {
    /**
     * 同一章节的多个正文命中必须逐条显示；objectId 相同但 byteStart/byteEnd 不同。
     * 用作 LazyColumn key 与游标翻页去重键。
     */
    val stableKey: String
        get() = "$objectId:$scope:${this.field}:$byteStart:$byteEnd"

    val isBodyHit: Boolean
        get() = this.field == "body"

    val isTitleHit: Boolean
        get() = this.field == "title"
}

/** Core `SearchResultPage` 的 JSON 映射。 */
data class SearchPage(
    val items: List<SearchHit> = emptyList(),
    val nextCursor: String? = null,
)
