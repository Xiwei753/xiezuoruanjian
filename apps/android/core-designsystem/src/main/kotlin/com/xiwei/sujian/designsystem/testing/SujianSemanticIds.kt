package com.xiwei.sujian.designsystem.testing

object SujianSemanticIds {
    const val NavigationWorks = "navigation.works"
    const val NavigationSettings = "navigation.settings"

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

    const val WorkspaceVolumeList = "workspace.volume.list"
    const val WorkspaceCreateVolume = "workspace.volume.create"
    const val WorkspaceCreateChapter = "workspace.chapter.create"
    fun createChapter(volumeId: String) = "workspace.chapter.create.$volumeId"
    const val ChapterTitleInput = "workspace.chapter.title.input"
    const val DialogConfirm = "dialog.confirm"
    const val DialogCancel = "dialog.cancel"

    const val EditorContent = "editor.content"
    const val EditorSaveStatus = "editor.save_status"

    fun project(projectId: String) = "workspace.project.$projectId"
    fun volume(id: String) = "workspace.volume.$id"
    fun chapter(volumeId: String, chapterId: String) =
        "workspace.chapter.$volumeId.$chapterId"
}
