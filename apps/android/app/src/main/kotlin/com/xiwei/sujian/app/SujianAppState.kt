package com.xiwei.sujian.app

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import com.xiwei.sujian.feature.project.domain.ProjectUseCase
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

interface WorkspaceAppState {
    val projects: List<com.xiwei.sujian.feature.project.data.model.Project>
    val recentEdits: List<com.xiwei.sujian.feature.project.data.model.RecentEdit>
    val currentProjectId: String?
    val currentProjectTitle: String
    val currentVolumeId: String?
    val currentChapterId: String?
    val currentChapterTitle: String

    fun selectProject(
        projectId: String,
        projectTitle: String,
    )

    fun selectProject(projectId: String)

    fun selectChapter(
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
    )

    fun selectChapter(
        volumeId: String,
        chapterId: String,
    )

    fun clearChapterSelection()

    fun clearProjectSelection()

    fun refreshProjects()

    fun refreshRecentEdits()

    fun createProject(title: String)

    fun deleteProject(projectId: String)

    fun renameProject(
        projectId: String,
        newTitle: String,
    )
}

class SujianAppViewModel(
    private val savedStateHandle: SavedStateHandle,
) : ViewModel() {
    var projects by androidx.compose.runtime.mutableStateOf<List<Project>>(emptyList())
        private set

    var recentEdits by androidx.compose.runtime.mutableStateOf<List<RecentEdit>>(emptyList())
        private set

    var currentProjectId by androidx.compose.runtime.mutableStateOf<String?>(savedStateHandle["currentProjectId"])
        private set

    var currentProjectTitle by androidx.compose.runtime.mutableStateOf(savedStateHandle["currentProjectTitle"] ?: "")
        private set

    var currentVolumeId by androidx.compose.runtime.mutableStateOf<String?>(savedStateHandle["currentVolumeId"])
        private set

    var currentChapterId by androidx.compose.runtime.mutableStateOf<String?>(savedStateHandle["currentChapterId"])
        private set

    var currentChapterTitle by androidx.compose.runtime.mutableStateOf(savedStateHandle["currentChapterTitle"] ?: "")
        private set

    var isLoading by androidx.compose.runtime.mutableStateOf(false)
        private set

    private var projectUseCase: ProjectUseCase? = null
    private var settingsRepository: SettingsRepository? = null
    private var appContext: android.content.Context? = null

    fun initialize(
        projectRepo: com.xiwei.sujian.feature.project.data.ProjectRepository,
        projectUC: ProjectUseCase,
        settingsRepo: SettingsRepository,
        context: android.content.Context,
    ) {
        projectUseCase = projectUC
        settingsRepository = settingsRepo
        appContext = context.applicationContext
        // #600：从 SavedState 恢复当前作品 id 到进程级 gate（进程重启后同步层仍能读到）。
        com.xiwei.sujian.app.state.ActiveProjectGate.setCurrentProjectId(currentProjectId)
        refreshProjects()
        refreshRecentEdits()
    }

    fun selectProject(
        projectId: String,
        projectTitle: String,
    ) {
        currentProjectId = projectId
        currentProjectTitle = projectTitle
        savedStateHandle["currentProjectId"] = projectId
        savedStateHandle["currentProjectTitle"] = projectTitle
        com.xiwei.sujian.app.state.ActiveProjectGate.setCurrentProjectId(projectId)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
    }

    fun selectProject(projectId: String) {
        currentProjectId = projectId
        savedStateHandle["currentProjectId"] = projectId
        com.xiwei.sujian.app.state.ActiveProjectGate.setCurrentProjectId(projectId)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
        val cachedProject = projects.find { it.id == projectId }
        if (cachedProject != null) {
            currentProjectTitle = cachedProject.title
            savedStateHandle["currentProjectTitle"] = cachedProject.title
        } else {
            viewModelScope.launch {
                val title =
                    withContext(Dispatchers.IO) {
                        try {
                            projectUseCase?.getProjectTitle(projectId) ?: ""
                        } catch (_: Exception) {
                            ""
                        }
                    }
                currentProjectTitle = title
                savedStateHandle["currentProjectTitle"] = title
            }
        }
    }

    fun selectChapter(
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
    ) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        currentChapterTitle = chapterTitle
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        savedStateHandle["currentChapterTitle"] = chapterTitle
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
        updateProcessStateSummaryForEditor(true)
    }

    fun selectChapter(
        volumeId: String,
        chapterId: String,
    ) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
        updateProcessStateSummaryForEditor(true)
        viewModelScope.launch {
            val title =
                withContext(Dispatchers.IO) {
                    try {
                        projectUseCase?.getChapterTitle(chapterId) ?: ""
                    } catch (_: Exception) {
                        ""
                    }
                }
            currentChapterTitle = title
            savedStateHandle["currentChapterTitle"] = title
        }
    }

    fun clearChapterSelection() {
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
        savedStateHandle.remove<String>("currentVolumeId")
        savedStateHandle.remove<String>("currentChapterId")
        savedStateHandle["currentChapterTitle"] = ""
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceClear("chapter")
        updateProcessStateSummaryForEditor(false)
    }

    fun clearProjectSelection() {
        currentProjectId = null
        currentProjectTitle = ""
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
        savedStateHandle.remove<String>("currentProjectId")
        savedStateHandle["currentProjectTitle"] = ""
        savedStateHandle.remove<String>("currentVolumeId")
        savedStateHandle.remove<String>("currentChapterId")
        savedStateHandle["currentChapterTitle"] = ""
        com.xiwei.sujian.app.state.ActiveProjectGate.setCurrentProjectId(null)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceClear("project")
        updateProcessStateSummaryForEditor(false)
    }

    fun refreshProjects() {
        viewModelScope.launch {
            projects =
                withContext(Dispatchers.IO) {
                    try {
                        projectUseCase?.getProjects() ?: emptyList()
                    } catch (_: Exception) {
                        emptyList()
                    }
                }
        }
    }

    fun refreshRecentEdits() {
        viewModelScope.launch {
            recentEdits =
                withContext(Dispatchers.IO) {
                    try {
                        projectUseCase?.getRecentEdits(5) ?: emptyList()
                    } catch (_: Exception) {
                        emptyList()
                    }
                }
        }
    }

    fun createProject(title: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    projectUseCase?.createProject(title)
                } catch (_: Exception) {
                }
            }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    projectUseCase?.deleteProject(projectId)
                } catch (_: Exception) {
                }
            }
            refreshProjects()
        }
    }

    fun renameProject(
        projectId: String,
        newTitle: String,
    ) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    projectUseCase?.renameProject(projectId, newTitle)
                } catch (_: Exception) {
                }
            }
            refreshProjects()
        }
    }

    /**
     * Issue #612 三、3.2：进入/退出正文时更新进程状态摘要，
     * 让下次冷启动读取 ApplicationExitInfo 时知道进程死前是否在编辑正文。
     */
    private fun updateProcessStateSummaryForEditor(editorActive: Boolean) {
        val ctx = appContext ?: return
        com.xiwei.sujian.core.diagnostics.ProcessStateSummary.update(
            ctx,
            "Works",
            if (editorActive) "1" else "0",
            "idle",
        )
    }
}

@Stable
class SujianAppState(
    val viewModel: SujianAppViewModel,
) : WorkspaceAppState {
    override val projects: List<Project> get() = viewModel.projects
    override val recentEdits: List<RecentEdit> get() = viewModel.recentEdits
    override val currentProjectId: String? get() = viewModel.currentProjectId
    override val currentProjectTitle: String get() = viewModel.currentProjectTitle
    override val currentVolumeId: String? get() = viewModel.currentVolumeId
    override val currentChapterId: String? get() = viewModel.currentChapterId
    override val currentChapterTitle: String get() = viewModel.currentChapterTitle
    val isLoading: Boolean get() = viewModel.isLoading

    override fun selectProject(
        projectId: String,
        projectTitle: String,
    ) = viewModel.selectProject(
        projectId,
        projectTitle,
    )

    override fun selectProject(projectId: String) = viewModel.selectProject(projectId)

    override fun selectChapter(
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
    ) = viewModel.selectChapter(
        volumeId,
        chapterId,
        chapterTitle,
    )

    override fun selectChapter(
        volumeId: String,
        chapterId: String,
    ) = viewModel.selectChapter(volumeId, chapterId)

    override fun clearChapterSelection() = viewModel.clearChapterSelection()

    override fun clearProjectSelection() = viewModel.clearProjectSelection()

    override fun refreshProjects() = viewModel.refreshProjects()

    override fun refreshRecentEdits() = viewModel.refreshRecentEdits()

    override fun createProject(title: String) = viewModel.createProject(title)

    override fun deleteProject(projectId: String) = viewModel.deleteProject(projectId)

    override fun renameProject(
        projectId: String,
        newTitle: String,
    ) = viewModel.renameProject(projectId, newTitle)
}
