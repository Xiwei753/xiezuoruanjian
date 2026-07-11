package com.xiwei.sujian.ui.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.FoldFeatureInfo
import com.xiwei.sujian.model.FoldState
import com.xiwei.sujian.model.FoldOrientation
import com.xiwei.sujian.model.FoldOcclusion
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.PointerKind
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.model.ShellMode
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.ui.compose.adaptive.AndroidAdaptiveWindowAdapter
import com.xiwei.sujian.ui.compose.adaptive.FoldState as AdaptiveFoldState
import com.xiwei.sujian.ui.compose.adaptive.FoldOrientation as AdaptiveFoldOrientation
import com.xiwei.sujian.ui.compose.adaptive.FoldOcclusionType
import com.xiwei.sujian.ui.compose.navigation.SujianDestination
import androidx.window.layout.FoldingFeature
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class SujianAppViewModel(
    private val savedStateHandle: SavedStateHandle
) : ViewModel() {

    var currentDestination by mutableStateOf(
        savedStateHandle["currentDestination"] ?: SujianDestination.Works.name
    )
        private set

    var projects by mutableStateOf<List<Project>>(emptyList())
        private set

    var recentEdits by mutableStateOf<List<RecentEdit>>(emptyList())
        private set

    var currentProjectId by mutableStateOf<String?>(savedStateHandle["currentProjectId"])
        internal set

    var currentProjectTitle by mutableStateOf(savedStateHandle["currentProjectTitle"] ?: "")
        internal set

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

    private var workspaceRepository: WorkspaceRepository? = null
    private var workspaceUseCase: WorkspaceUseCase? = null
    private var settingsRepository: SettingsRepository? = null
    var coroutineScope: CoroutineScope? = null

    fun initialize(
        workspaceRepo: WorkspaceRepository,
        workspaceUC: WorkspaceUseCase,
        settingsRepo: SettingsRepository,
        scope: CoroutineScope
    ) {
        workspaceRepository = workspaceRepo
        workspaceUseCase = workspaceUC
        settingsRepository = settingsRepo
        coroutineScope = scope
        refreshProjects()
        refreshRecentEdits()
    }

    fun navigateTo(destination: SujianDestination) {
        currentDestination = destination.name
        savedStateHandle["currentDestination"] = destination.name
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

    fun updateFoldFeature(info: FoldFeatureInfo) {
        foldFeatureInfo = info
    }

    fun updateFoldFeaturesFromAdaptive(features: List<FoldingFeature>, density: Float = 1f) {
        val coreFoldInfo = if (features.isNotEmpty()) {
            val feature = features.first()
            val info = AndroidAdaptiveWindowAdapter.toFoldFeatureInfo(feature)
            FoldFeatureInfo(
                state = when (info.state) {
                    AdaptiveFoldState.Flat -> FoldState.Flat
                    AdaptiveFoldState.HalfOpened -> FoldState.HalfOpened
                    else -> FoldState.None
                },
                orientation = if (info.orientation == AdaptiveFoldOrientation.Horizontal) FoldOrientation.Horizontal else FoldOrientation.Vertical,
                isSeparating = info.isSeparating,
                occlusion = if (info.occlusionType == FoldOcclusionType.Full) FoldOcclusion.Full else FoldOcclusion.None,
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
        val bridge = workspaceRepository?.let { BridgeProvider.getLayoutPolicyBridge(it) } ?: return null
        val plan = bridge.resolveLayout(metrics)
        currentLayoutPlan = plan
        return plan
    }

    fun refreshProjects() {
        coroutineScope?.launch {
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
        coroutineScope?.launch {
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
        coroutineScope?.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.createProject(title)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        coroutineScope?.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.deleteProject(projectId)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        coroutineScope?.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.renameProject(projectId, newTitle)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun getCurrentDestination(): SujianDestination {
        return try {
            SujianDestination.valueOf(currentDestination)
        } catch (_: Exception) {
            SujianDestination.Works
        }
    }
}

@Stable
class SujianAppState(
    val viewModel: SujianAppViewModel
) {
    val currentDestination: SujianDestination get() = viewModel.getCurrentDestination()
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

    fun navigateTo(destination: SujianDestination) = viewModel.navigateTo(destination)
    fun selectProject(projectId: String, projectTitle: String) = viewModel.selectProject(projectId, projectTitle)
    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) = viewModel.selectChapter(volumeId, chapterId, chapterTitle)
    fun clearChapterSelection() = viewModel.clearChapterSelection()
    fun updateFoldFeaturesFromAdaptive(features: List<FoldingFeature>, density: Float = 1f) = viewModel.updateFoldFeaturesFromAdaptive(features, density)
    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? = viewModel.resolveLayout(metrics)
    fun refreshProjects() = viewModel.refreshProjects()
    fun refreshRecentEdits() = viewModel.refreshRecentEdits()
    fun createProject(title: String) = viewModel.createProject(title)
    fun deleteProject(projectId: String) = viewModel.deleteProject(projectId)
    fun renameProject(projectId: String, newTitle: String) = viewModel.renameProject(projectId, newTitle)
}
