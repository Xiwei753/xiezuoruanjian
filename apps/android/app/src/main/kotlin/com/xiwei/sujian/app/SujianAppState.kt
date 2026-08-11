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
import com.xiwei.sujian.feature.project.domain.ProjectUseCasePort
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * #614：app 层 UI 事件流。ViewModel 抛出，[SujianNavigationSuite] 收集并展示。
 * 放在 app 层（不进 core/designsystem），避免 feature 各 data 层依赖 Compose。
 */
sealed interface WorkspaceUiEvent {
    data class Error(val message: String) : WorkspaceUiEvent
}

interface WorkspaceAppState {
    val projects: List<com.xiwei.sujian.feature.project.data.model.Project>
    val recentEdits: List<com.xiwei.sujian.feature.project.data.model.RecentEdit>
    val currentProjectId: String?
    val currentProjectTitle: String
    val currentVolumeId: String?
    val currentChapterId: String?
    val currentChapterTitle: String

    /** #614：首次加载失败时的错误文本，非 null 时列表显示错误态。 */
    val loadError: String?

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

    /** #614：UI 事件流 — 错误等不吞异常，发事件由 Snackbar 展示。 */
    private val _uiEvents = MutableSharedFlow<WorkspaceUiEvent>(extraBufferCapacity = 16)
    val uiEvents: SharedFlow<WorkspaceUiEvent> = _uiEvents.asSharedFlow()

    /** #614：首次加载失败时的错误文本；后续刷新失败保留上一份 projects，不覆盖。 */
    var loadError by androidx.compose.runtime.mutableStateOf<String?>(null)
        private set

    private var projectUseCase: ProjectUseCasePort? = null
    private var settingsRepository: SettingsRepository? = null
    private var appContext: android.content.Context? = null

    /**
     * #614 评论二：未初始化时显式抛异常，避免 safe-call 静默返回 null 被当成 Success(null)。
     * 抛出的 IllegalStateException 由各调用方的 runCatching/try-catch 捕获，走 errorMessage → WorkspaceUiEvent.Error。
     */
    private fun requireProjectUseCase(): ProjectUseCasePort = checkNotNull(projectUseCase) { "ProjectUseCase 尚未初始化" }

    /**
     * #614：仅用于单元测试注入 fake [ProjectUseCasePort]，绕过 [initialize] 的真实 Repository 构造。
     */
    @androidx.annotation.VisibleForTesting
    internal fun setProjectUseCaseForTesting(useCase: ProjectUseCasePort?) {
        projectUseCase = useCase
    }

    fun initialize(
        projectRepo: com.xiwei.sujian.feature.project.data.ProjectRepository,
        projectUC: ProjectUseCasePort,
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
                            requireProjectUseCase().getProjectTitle(projectId)
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
        viewModelScope.launch {
            val title =
                withContext(Dispatchers.IO) {
                    try {
                        requireProjectUseCase().getChapterTitle(chapterId)
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
    }

    fun refreshProjects() {
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { requireProjectUseCase().getProjects() }
                }
            result.onSuccess { list ->
                projects = list
                loadError = null
            }.onFailure { e ->
                // #614：保留上一份 projects，不覆盖为空；仅首次加载失败设 loadError。
                if (projects.isEmpty()) {
                    loadError = errorMessage(e)
                }
                _uiEvents.tryEmit(WorkspaceUiEvent.Error(errorMessage(e)))
            }
        }
    }

    fun refreshRecentEdits() {
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { requireProjectUseCase().getRecentEdits(5) }
                }
            result.onSuccess { list -> recentEdits = list }
                .onFailure { _uiEvents.tryEmit(WorkspaceUiEvent.Error(errorMessage(it))) }
        }
    }

    fun createProject(title: String) {
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { requireProjectUseCase().createProject(title) }
                }
            result.onFailure { _uiEvents.tryEmit(WorkspaceUiEvent.Error(errorMessage(it))) }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { requireProjectUseCase().deleteProject(projectId) }
                }
            result.onFailure { _uiEvents.tryEmit(WorkspaceUiEvent.Error(errorMessage(it))) }
            refreshProjects()
        }
    }

    fun renameProject(
        projectId: String,
        newTitle: String,
    ) {
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { requireProjectUseCase().renameProject(projectId, newTitle) }
                }
            result.onFailure { _uiEvents.tryEmit(WorkspaceUiEvent.Error(errorMessage(it))) }
            refreshProjects()
        }
    }

    /** #614：异常 → 本地化错误文本。RepositoryException.message 已是 context.getString 本地化串。 */
    private fun errorMessage(e: Throwable): String =
        when (e) {
            is com.xiwei.sujian.core.interop.common.RepositoryException ->
                e.message ?: (appContext?.getString(com.xiwei.sujian.R.string.error_internal) ?: "操作失败")
            else -> appContext?.getString(com.xiwei.sujian.R.string.error_internal) ?: "操作失败"
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
    override val loadError: String? get() = viewModel.loadError
    val isLoading: Boolean get() = viewModel.isLoading
    val uiEvents: SharedFlow<WorkspaceUiEvent> get() = viewModel.uiEvents

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
