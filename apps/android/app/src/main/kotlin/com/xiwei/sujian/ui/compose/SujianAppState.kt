package com.xiwei.sujian.ui.compose

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import com.xiwei.sujian.data.LayoutPolicyRepositoryProvider
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.FoldFeatureInfo
import com.xiwei.sujian.model.FoldState
import com.xiwei.sujian.model.FoldOrientation
import com.xiwei.sujian.model.FoldOcclusion
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.platform.api.FoldPosture
import com.xiwei.sujian.platform.api.FoldOrientation as PlatformFoldOrientation
import com.xiwei.sujian.platform.api.OcclusionType
import com.xiwei.sujian.platform.window.WindowFoldFeatureCollector
import androidx.window.layout.FoldingFeature
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import com.xiwei.sujian.ui.phone.portrait.WorkspaceSessionViewModel

interface WorkspaceAppState {
    val projects: List<com.xiwei.sujian.model.Project>
    val recentEdits: List<com.xiwei.sujian.model.RecentEdit>
    val currentProjectId: String?
    val currentProjectTitle: String
    val currentVolumeId: String?
    val currentChapterId: String?
    val currentChapterTitle: String
    fun selectProject(projectId: String, projectTitle: String)
    fun selectProject(projectId: String)
    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String)
    fun selectChapter(volumeId: String, chapterId: String)
    fun clearChapterSelection()
    fun clearProjectSelection()
    fun refreshProjects()
    fun refreshRecentEdits()
    fun createProject(title: String)
    fun deleteProject(projectId: String)
    fun renameProject(projectId: String, newTitle: String)
}

class SujianAppViewModel(
    private val savedStateHandle: SavedStateHandle
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

    var currentLayoutPlan by androidx.compose.runtime.mutableStateOf<LayoutPlan?>(null)
        private set

    var foldFeatureInfo by androidx.compose.runtime.mutableStateOf<FoldFeatureInfo>(FoldFeatureInfo())
        private set

    var isLoading by androidx.compose.runtime.mutableStateOf(false)
        private set

    private var workspaceUseCase: WorkspaceUseCase? = null
    private var settingsRepository: SettingsRepository? = null
    private var appContext: android.content.Context? = null

    fun initialize(
        workspaceRepo: com.xiwei.sujian.data.WorkspaceRepository,
        workspaceUC: WorkspaceUseCase,
        settingsRepo: SettingsRepository,
        context: android.content.Context
    ) {
        workspaceUseCase = workspaceUC
        settingsRepository = settingsRepo
        appContext = context.applicationContext
        refreshProjects()
        refreshRecentEdits()
    }

    fun selectProject(projectId: String, projectTitle: String) {
        currentProjectId = projectId
        currentProjectTitle = projectTitle
        savedStateHandle["currentProjectId"] = projectId
        savedStateHandle["currentProjectTitle"] = projectTitle
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
    }

    fun selectProject(projectId: String) {
        currentProjectId = projectId
        savedStateHandle["currentProjectId"] = projectId
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
        val cachedProject = projects.find { it.id == projectId }
        if (cachedProject != null) {
            currentProjectTitle = cachedProject.title
            savedStateHandle["currentProjectTitle"] = cachedProject.title
        } else {
            viewModelScope.launch {
                val title = withContext(Dispatchers.IO) {
                    try {
                        workspaceUseCase?.getProjectTitle(projectId) ?: ""
                    } catch (_: Exception) {
                        ""
                    }
                }
                currentProjectTitle = title
                savedStateHandle["currentProjectTitle"] = title
            }
        }
    }

    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        currentChapterTitle = chapterTitle
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        savedStateHandle["currentChapterTitle"] = chapterTitle
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
    }

    fun selectChapter(volumeId: String, chapterId: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
        viewModelScope.launch {
            val title = withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.getChapterTitle(chapterId) ?: ""
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
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceClear("chapter")
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
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceClear("project")
    }

    fun updateFoldFeature(info: FoldFeatureInfo) {
        foldFeatureInfo = info
    }

    fun updateFoldFeaturesFromAdaptive(features: List<FoldingFeature>, density: Float = 1f) {
        val coreFoldInfo = if (features.isNotEmpty()) {
            val feature = features.first()
            val info = WindowFoldFeatureCollector.toFoldFeatureInfo(feature)
            FoldFeatureInfo(
                state = when (info.state) {
                    FoldPosture.Flat -> FoldState.Flat
                    FoldPosture.HalfOpened -> FoldState.HalfOpened
                    else -> FoldState.None
                },
                orientation = if (info.orientation == PlatformFoldOrientation.Horizontal) FoldOrientation.Horizontal else FoldOrientation.Vertical,
                isSeparating = info.isSeparating,
                occlusion = when (info.occlusionType) {
                    OcclusionType.Full -> FoldOcclusion.Full
                    else -> FoldOcclusion.None
                },
                boundsLeftVp = info.boundsLeft.toFloat() / density,
                boundsTopVp = info.boundsTop.toFloat() / density,
                boundsRightVp = info.boundsRight.toFloat() / density,
                boundsBottomVp = info.boundsBottom.toFloat() / density
            )
        } else {
            FoldFeatureInfo()
        }
        foldFeatureInfo = coreFoldInfo
    }

    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? {
        val ctx = appContext ?: return null
        val bridge = LayoutPolicyRepositoryProvider.getLayoutPolicyBridge(ctx)
        val plan = bridge.resolveLayout(metrics)
        currentLayoutPlan = plan
        return plan
    }

    fun refreshProjects() {
        viewModelScope.launch {
            projects = withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.getProjects() ?: emptyList()
                } catch (_: Exception) {
                    emptyList()
                }
            }
        }
    }

    fun refreshRecentEdits() {
        viewModelScope.launch {
            recentEdits = withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.getRecentEdits(5) ?: emptyList()
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
                    workspaceUseCase?.createProject(title)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.deleteProject(projectId)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.renameProject(projectId, newTitle)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }
}

@Stable
class SujianAppState(
    val viewModel: SujianAppViewModel
) : WorkspaceAppState {
    override val projects: List<Project> get() = viewModel.projects
    override val recentEdits: List<RecentEdit> get() = viewModel.recentEdits
    override val currentProjectId: String? get() = viewModel.currentProjectId
    override val currentProjectTitle: String get() = viewModel.currentProjectTitle
    override val currentVolumeId: String? get() = viewModel.currentVolumeId
    override val currentChapterId: String? get() = viewModel.currentChapterId
    override val currentChapterTitle: String get() = viewModel.currentChapterTitle
    val currentLayoutPlan: LayoutPlan? get() = viewModel.currentLayoutPlan
    val foldFeatureInfo: FoldFeatureInfo get() = viewModel.foldFeatureInfo
    val isLoading: Boolean get() = viewModel.isLoading

    override fun selectProject(projectId: String, projectTitle: String) = viewModel.selectProject(projectId, projectTitle)
    override fun selectProject(projectId: String) = viewModel.selectProject(projectId)
    override fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) = viewModel.selectChapter(volumeId, chapterId, chapterTitle)
    override fun selectChapter(volumeId: String, chapterId: String) = viewModel.selectChapter(volumeId, chapterId)
    override fun clearChapterSelection() = viewModel.clearChapterSelection()
    override fun clearProjectSelection() = viewModel.clearProjectSelection()
    fun updateFoldFeaturesFromAdaptive(features: List<FoldingFeature>, density: Float = 1f) = viewModel.updateFoldFeaturesFromAdaptive(features, density)
    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? = viewModel.resolveLayout(metrics)
    override fun refreshProjects() = viewModel.refreshProjects()
    override fun refreshRecentEdits() = viewModel.refreshRecentEdits()
    override fun createProject(title: String) = viewModel.createProject(title)
    override fun deleteProject(projectId: String) = viewModel.deleteProject(projectId)
    override fun renameProject(projectId: String, newTitle: String) = viewModel.renameProject(projectId, newTitle)
}

@Stable
class SujianSessionAppState(
    val sessionViewModel: WorkspaceSessionViewModel
) : WorkspaceAppState {
    override val projects: List<Project> get() = sessionViewModel.projects
    override val recentEdits: List<RecentEdit> get() = sessionViewModel.recentEdits
    override val currentProjectId: String? get() = sessionViewModel.currentProjectId
    override val currentProjectTitle: String get() = sessionViewModel.currentProjectTitle
    override val currentVolumeId: String? get() = sessionViewModel.currentVolumeId
    override val currentChapterId: String? get() = sessionViewModel.currentChapterId
    override val currentChapterTitle: String get() = sessionViewModel.currentChapterTitle
    val currentLayoutPlan: LayoutPlan? get() = null
    val foldFeatureInfo: FoldFeatureInfo get() = FoldFeatureInfo()
    val isLoading: Boolean get() = false

    override fun selectProject(projectId: String, projectTitle: String) = sessionViewModel.selectProject(projectId, projectTitle)
    override fun selectProject(projectId: String) = sessionViewModel.selectProject(projectId)
    override fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) = sessionViewModel.selectChapter(volumeId, chapterId, chapterTitle)
    override fun selectChapter(volumeId: String, chapterId: String) = sessionViewModel.selectChapter(volumeId, chapterId)
    override fun clearChapterSelection() = sessionViewModel.clearChapterSelection()
    override fun clearProjectSelection() = sessionViewModel.clearProjectSelection()
    override fun refreshProjects() = sessionViewModel.refreshProjects()
    override fun refreshRecentEdits() = sessionViewModel.refreshRecentEdits()
    override fun createProject(title: String) = sessionViewModel.createProject(title)
    override fun deleteProject(projectId: String) = sessionViewModel.deleteProject(projectId)
    override fun renameProject(projectId: String, newTitle: String) = sessionViewModel.renameProject(projectId, newTitle)
}
