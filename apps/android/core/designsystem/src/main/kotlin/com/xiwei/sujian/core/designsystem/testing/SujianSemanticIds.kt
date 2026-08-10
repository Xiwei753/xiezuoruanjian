package com.xiwei.sujian.core.designsystem.testing

/**
 * 当前真实 UI 的稳定语义 ID（#597 八）。
 *
 * 页面测试一律通过这些 ID 定位控件，不靠中文文本找按钮。
 * 只保留当前已实现界面的 ID；已删除页面（旧 Workbench）与未实现功能
 * （星图完整页面）的 ID 不得保留。
 */
object SujianSemanticIds {
    // ---- 一级导航（底栏/侧栏）----
    const val NavigationWorks = "navigation.works"
    const val NavigationStarMap = "navigation.starmap"
    const val NavigationStats = "navigation.stats"
    const val NavigationBar = "navigation.bar"
    const val NavigationRail = "navigation.rail"

    // ---- 顶栏操作 ----
    const val NavigationSettings = "navigation.settings"
    const val NavigationSearch = "navigation.search"
    const val NavigationSync = "navigation.sync"

    // ---- 设置页 ----
    const val SettingsScreen = "settings.screen"
    const val SettingsNavAppearance = "settings.nav.appearance"
    const val SettingsNavEditor = "settings.nav.editor"
    const val SettingsNavSave = "settings.nav.save"
    const val SettingsNavSync = "settings.nav.sync"
    const val SettingsNavAi = "settings.nav.ai"
    const val SettingsNavDiagnostics = "settings.nav.diagnostics"
    const val SettingsNavLaboratory = "settings.nav.laboratory"
    const val SettingsNavAbout = "settings.nav.about"
    const val SettingsEditorSection = "settings.editor.section"
    const val SettingsFontSize = "settings.editor.font_size"
    const val SettingsTypingAnimation = "settings.editor.typing_animation"

    // ---- 作品工作区 ----
    const val WorkspaceVolumeList = "workspace.volume.list"
    const val WorkspaceCreateVolume = "workspace.volume.create"
    const val WorkspaceCreateChapter = "workspace.chapter.create"
    fun createChapter(volumeId: String) = "workspace.chapter.create.$volumeId"
    /** 空态新建章节（#610 评论四：EmptyState 槽位的真实消费点，与行尾 ItemTrailing 区分）。 */
    fun createChapterInEmpty(volumeId: String) = "workspace.chapter.create.empty.$volumeId"
    const val ChapterTitleInput = "workspace.chapter.title.input"
    const val DialogConfirm = "dialog.confirm"
    const val DialogCancel = "dialog.cancel"

    // ---- 编辑器 ----
    const val EditorContent = "editor.content"
    const val EditorSaveStatus = "editor.save_status"

    // ---- 星图占位页（功能未实现，仅保留页面级 ID）----
    const val StarMapScreen = "starmap.screen"

    fun project(projectId: String) = "workspace.project.$projectId"
    fun volume(id: String) = "workspace.volume.$id"
    fun chapter(volumeId: String, chapterId: String) =
        "workspace.chapter.$volumeId.$chapterId"
}
