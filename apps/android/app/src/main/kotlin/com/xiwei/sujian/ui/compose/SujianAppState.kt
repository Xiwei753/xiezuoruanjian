package com.xiwei.sujian.ui.compose

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import com.xiwei.sujian.data.BridgeProvider
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
import com.xiwei.sujian.platform.window.AospFoldFeatureInfo
import androidx.window.layout.FoldingFeature
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class SujianAppViewModel(
    private val savedStateHandle: SavedStateHandle
) : ViewModel() {

    var projects by mutableStateOf<List<Project>>(emptyList())
        private set

    var recentEdits by mutableStateOf<List<RecentEdit>>(emptyList())
        private set

    var currentProjectId by mutableStateOf<String?>(savedStateHandle["currentProjectId"])
        private set

    var currentProjectTitle by mutableStateOf(savedStateHandle["currentProjectTitle"] ?: "")
        private set

    var currentVolumeId by mutableStateOf<String?>(savedStateHandle["currentVolumeId"])
        private set

    var currentChapterId by mutableStateOf<String?>(savedStateHandle["currentChapterId"])
        private set

    var currentChapterTitle by mutableStateOf(savedStateHandle["currentChapterTitle"] ?: "")
        private set

    var currentLayoutPlan by mutableStateOf<LayoutPlan?>(null)
        private set

    var foldFeatureInfo by mutableStateOf<FoldFeatureInfo>(FoldFeatureInfo())
        private set

    var isLoading by mutableStateOf(false)
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
    }

    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        currentChapterTitle = chapterTitle
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        savedStateHandle["currentChapterTitle"] = chapterTitle
    }

    fun clearChapterSelection() {
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
        savedStateHandle.remove<String>("currentVolumeId")
        savedStateHandle.remove<String>("currentChapterId")
        savedStateHandle["currentChapterTitle"] = ""
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
        val bridge = BridgeProvider.getLayoutPolicyBridge(ctx)
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
) {
    val projects: List<Project> get() = viewModel.projects
    val recentEdits: List<RecentEdit> get() = viewModel.recentEdits
    val currentProjectId: String? get() = viewModel.currentProjectId
    val currentProjectTitle: String get() = viewModel.currentProjectTitle
    val currentVolumeId: String? get() = viewModel.currentVolumeId
    val currentChapterId: String? get() = viewModel.currentChapterId
    val currentChapterTitle: String get() = viewModel.currentChapterTitle
    val currentLayoutPlan: LayoutPlan? get() = viewModel.currentLayoutPlan
    val foldFeatureInfo: FoldFeatureInfo get() = viewModel.foldFeatureInfo
    val isLoading: Boolean get() = viewModel.isLoading

    fun selectProject(projectId: String, projectTitle: String) = viewModel.selectProject(projectId, projectTitle)
    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) = viewModel.selectChapter(volumeId, chapterId, chapterTitle)
    fun clearChapterSelection() = viewModel.clearChapterSelection()
    fun clearProjectSelection() = viewModel.clearProjectSelection()
    fun updateFoldFeaturesFromAdaptive(features: List<FoldingFeature>, density: Float = 1f) = viewModel.updateFoldFeaturesFromAdaptive(features, density)
    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? = viewModel.resolveLayout(metrics)
    fun refreshProjects() = viewModel.refreshProjects()
    fun refreshRecentEdits() = viewModel.refreshRecentEdits()
    fun createProject(title: String) = viewModel.createProject(title)
    fun deleteProject(projectId: String) = viewModel.deleteProject(projectId)
    fun renameProject(projectId: String, newTitle: String) = viewModel.renameProject(projectId, newTitle)
}
