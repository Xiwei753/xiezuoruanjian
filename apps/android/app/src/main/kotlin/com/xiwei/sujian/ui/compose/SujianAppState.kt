package com.xiwei.sujian.ui.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
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

@Stable
class SujianAppState(
    val coroutineScope: CoroutineScope
) {
    var currentDestination by mutableStateOf(SujianDestination.Works)
        private set

    var projects by mutableStateOf<List<Project>>(emptyList())
        private set

    var recentEdits by mutableStateOf<List<RecentEdit>>(emptyList())
        private set

    var currentProjectId by mutableStateOf<String?>(null)
        internal set

    var currentProjectTitle by mutableStateOf("")
        internal set

    var currentVolumeId by mutableStateOf<String?>(null)
        private set

    var currentChapterId by mutableStateOf<String?>(null)
        private set

    var currentChapterTitle by mutableStateOf("")
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

    fun initialize(
        workspaceRepo: WorkspaceRepository,
        workspaceUC: WorkspaceUseCase,
        settingsRepo: SettingsRepository
    ) {
        workspaceRepository = workspaceRepo
        workspaceUseCase = workspaceUC
        settingsRepository = settingsRepo
        refreshProjects()
        refreshRecentEdits()
    }

    fun navigateTo(destination: SujianDestination) {
        currentDestination = destination
    }

    fun selectProject(projectId: String, projectTitle: String) {
        currentProjectId = projectId
        currentProjectTitle = projectTitle
    }

    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        currentChapterTitle = chapterTitle
    }

    fun clearChapterSelection() {
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
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
        coroutineScope.launch {
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
        coroutineScope.launch {
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
        coroutineScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.createProject(title)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        coroutineScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.deleteProject(projectId)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        coroutineScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    workspaceUseCase?.renameProject(projectId, newTitle)
                } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }
}

@Composable
fun rememberSujianAppState(): SujianAppState {
    val coroutineScope = rememberCoroutineScope()
    return remember { SujianAppState(coroutineScope) }
}
