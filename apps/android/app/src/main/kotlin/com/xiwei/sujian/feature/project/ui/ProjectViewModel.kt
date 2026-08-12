package com.xiwei.sujian.feature.project.ui

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.feature.project.data.ProjectRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class VolumeChapterUiState(
    val volumes: List<VolumeUiModel> = emptyList(),
    val expandedVolumeIds: Set<String> = emptySet(),
    val selectedChapterId: String? = null,
    val projectStats: ProjectStatsUiModel? = null,
    val isLoading: Boolean = false,
    /** #617 评论九：首次加载失败时的错误文本，非 null 时章节树显示"加载失败"而非"暂无卷"。 */
    val loadError: String? = null,
)

/** #617 评论七：一次 refresh 读取的完整快照 — 卷列表与统计原子提交，避免分两次写回。 */
private data class ProjectSnapshot(
    val volumes: List<VolumeUiModel>,
    val projectStats: ProjectStatsUiModel?,
)

/**
 * #617 评论九：章节树 UI 事件 — 错误等不吞异常，发事件由 [ChapterTreeContent] 收集
 * 并通过 [onError] 转交全局 Snackbar（不在 feature 里另起 Toast/Snackbar 系统）。
 */
sealed interface ProjectTreeUiEvent {
    data class Error(val message: String) : ProjectTreeUiEvent
}

// #617 评论一：ProjectViewModel 不依赖 Application — Navigation 3 的 NavEntry 级
// ViewModelStoreOwner 的 CreationExtras 只保证提供 SavedStateHandle（由
// SaveableStateHolderNavEntryDecorator + ViewModelStoreNavEntryDecorator 提供），
// 不保证 APPLICATION_KEY；此前继承 AndroidViewModel 会在进入章节树时崩溃。
class ProjectViewModel(
    private val savedStateHandle: SavedStateHandle,
) : ViewModel() {
    private var currentProjectId: String? = savedStateHandle["currentProjectId"]

    private val _uiState =
        MutableStateFlow(
            VolumeChapterUiState(
                // #617 评论七：展开状态按 projectId 分 key 保存，进程重建后也只恢复本作品的展开。
                expandedVolumeIds =
                    savedStateHandle[expandedVolumeKey(currentProjectId ?: "")] ?: emptySet(),
            ),
        )
    val uiState: StateFlow<VolumeChapterUiState> = _uiState.asStateFlow()

    /** #617 评论九：章节树 UI 事件流 — 错误等不吞异常，由 [ChapterTreeContent] 收集。 */
    private val _uiEvents = MutableSharedFlow<ProjectTreeUiEvent>(extraBufferCapacity = 8)
    val uiEvents: SharedFlow<ProjectTreeUiEvent> = _uiEvents.asSharedFlow()

    private var projectRepository: ProjectRepository? = null
    private var isInitialized: Boolean = false

    // #617 评论七：作品读取收成一条可取消的刷新链 — 作品切换/变更刷新都会取消在途
    // refresh job 并递增纪元；迟到的旧纪元/旧作品结果在写回前被丢弃，防止慢设备上
    // 旧作品的卷/统计覆盖新作品的章节树。纪元在作品切换时递增，同作品变更刷新
    // 靠 refreshJob.cancel() + 写回前校验（generation/pid）双保险。
    private var projectGeneration = 0L
    private var refreshJob: Job? = null

    /** 当前 _uiState 数据所属的作品；用于区分“作品切换”与“同作品变更刷新”。 */
    private var loadedProjectId: String? = null

    private fun expandedVolumeKey(projectId: String) = "expandedVolumeIds:$projectId"

    fun initialize(
        projectId: String,
        projectRepo: ProjectRepository,
    ) {
        val projectChanged = currentProjectId != projectId
        val repoChanged = projectRepository !== projectRepo
        val needsReload =
            !isInitialized || projectChanged || repoChanged ||
                _uiState.value.volumes.isEmpty()

        currentProjectId = projectId
        savedStateHandle["currentProjectId"] = projectId
        projectRepository = projectRepo

        if (needsReload) {
            isInitialized = true
            projectGeneration++
            refreshProject()
        }
    }

    /** 取消在途刷新并重新读取当前作品的卷/章节/统计 — 数据刷新的唯一入口。 */
    private fun refreshProject() {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val generation = projectGeneration

        refreshJob?.cancel()
        refreshJob =
            viewModelScope.launch {
                if (loadedProjectId != pid) {
                    enterProject(pid)
                } else {
                    _uiState.value = _uiState.value.copy(isLoading = true)
                }

                // #617 评论九：loadProjectSnapshot 不再吞异常 — 任一必要数据失败
                // 让异常回到这里统一处理：保留旧数据、首次加载设 loadError、发错误事件，
                // 不再把"读取失败"伪装成"作品真的是空的"。try-catch 在 withContext(IO)
                // 内部捕获，避免跨线程异常传播问题。
                val result = loadProjectSnapshot(repo, pid)
                if (generation != projectGeneration || pid != currentProjectId) return@launch
                result
                    .onSuccess { snapshot ->
                        loadedProjectId = pid
                        // #617 评论八：写回只替换卷/章节数据与统计 — 展开状态不在 UI 模型里，
                        // 只存在于 expandedVolumeIds（写回不触碰），加载在途期间的展开切换
                        // 永远不会被刷新链覆盖。
                        _uiState.value =
                            _uiState.value.copy(
                                volumes = snapshot.volumes,
                                projectStats = snapshot.projectStats,
                                isLoading = false,
                                loadError = null,
                            )
                    }.onFailure { e ->
                        // #617 评论九：保留上一次成功的 volumes/projectStats，不要写
                        // emptyList()/null 覆盖；首次加载没有旧数据时记录 loadError，
                        // 让 UI 显示"加载失败"不要显示"暂无卷"。
                        val firstLoad = _uiState.value.volumes.isEmpty()
                        _uiState.value =
                            _uiState.value.copy(
                                isLoading = false,
                                loadError = if (firstLoad) errorMessage(e) else _uiState.value.loadError,
                            )
                        _uiEvents.tryEmit(ProjectTreeUiEvent.Error(errorMessage(e)))
                    }
            }
    }

    /** 作品切换：立即清掉只属于旧作品的 UI 状态（卷/选中章节/统计），展开按 projectId 恢复。 */
    private fun enterProject(pid: String) {
        loadedProjectId = pid
        _uiState.value =
            VolumeChapterUiState(
                expandedVolumeIds = savedStateHandle[expandedVolumeKey(pid)] ?: emptySet(),
                isLoading = true,
            )
    }

    /** 在 IO 线程读取当前作品快照（卷 + 章节 + 统计）。
     *  注意：UI 模型不携带交互状态（#617 评论八）— 展开/收起只存在
     *  expandedVolumeIds 一份真相，快照只负责仓库数据，不读也不写展开状态。
     *  #617 评论九：不再吞异常 — 任一读取失败让异常自然传播到 [refreshProject]
     *  统一处理，避免把"读取失败"伪装成"作品真的是空的"。 */
    private suspend fun loadProjectSnapshot(
        repo: ProjectRepository,
        projectId: String,
    ): Result<ProjectSnapshot> =
        withContext(Dispatchers.IO) {
            try {
                val volumes =
                    repo.getVolumes(projectId).map { vol ->
                        VolumeUiModel(
                            id = vol.id,
                            title = vol.title,
                            chapters =
                                repo.getChapters(projectId, vol.id).map { ch ->
                                    ChapterUiModel(
                                        id = ch.id,
                                        title = ch.title,
                                        wordCount = ch.wordCount,
                                    )
                                },
                        )
                    }
                val stats =
                    repo.getProjectStats(projectId)?.let {
                        ProjectStatsUiModel(
                            totalWordCount = it.totalWordCount,
                            volumeCount = it.volumeCount,
                            chapterCount = it.chapterCount,
                        )
                    }
                Result.success(ProjectSnapshot(volumes, stats))
            } catch (e: Throwable) {
                Result.failure(e)
            }
        }

    /** #617 评论九：异常 → 错误文本。ProjectViewModel 不持有 Context（评论一明确禁止塞
     *  Application/Activity），RepositoryException.message 已在 Repository 层用
     *  context.getString 本地化，直接透出；其他异常是编程不变式，用英文 fallback
     *  （非 UI 文案，i18n 守卫只管 UI 硬编码）。 */
    private fun errorMessage(e: Throwable): String =
        when (e) {
            is RepositoryException -> e.message ?: "Repository operation failed"
            else -> e.message ?: "Unexpected error"
        }

    /** #617 评论九：把八个 CRUD/排序方法里重复的 try/catch {} 收成一个私有入口 —
     *  仓库操作成功才刷新；失败只发错误事件，不再执行"失败后也 refresh"，
     *  不再把"保存失败"伪装成"按钮没反应"。 */
    private fun mutateAndRefresh(
        pid: String,
        block: suspend (ProjectRepository) -> Unit,
    ) {
        val repo = projectRepository ?: return
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    try {
                        Result.success(block(repo))
                    } catch (e: Throwable) {
                        Result.failure(e)
                    }
                }
            if (currentProjectId != pid) return@launch
            result
                .onSuccess { refreshProject() }
                .onFailure { _uiEvents.tryEmit(ProjectTreeUiEvent.Error(errorMessage(it))) }
        }
    }

    fun toggleVolumeExpand(volumeId: String) {
        val pid = currentProjectId ?: return
        val current = _uiState.value.expandedVolumeIds
        val newExpanded =
            if (current.contains(volumeId)) {
                current - volumeId
            } else {
                current + volumeId
            }
        // #617 评论七/八：展开状态按 projectId 分 key 保存，不跨作品混用；
        // 这是展开状态的唯一真相 — 不写 volumes（UI 模型不携带 isExpanded），
        // 渲染方从 expandedVolumeIds 派生，刷新写回永不覆盖用户切换。
        savedStateHandle[expandedVolumeKey(pid)] = newExpanded
        _uiState.value = _uiState.value.copy(expandedVolumeIds = newExpanded)
    }

    fun selectChapter(chapterId: String) {
        _uiState.value = _uiState.value.copy(selectedChapterId = chapterId)
    }

    fun createVolume(title: String) {
        val pid = currentProjectId ?: return
        mutateAndRefresh(pid) { repo -> repo.createVolume(pid, title) }
    }

    fun createChapter(
        volumeId: String,
        title: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        // #617 评论九：createChapter 特殊 — 只有创建成功后才能把该卷加入
        // expandedVolumeIds；此前失败也会自动展开卷，状态是假的。
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    try {
                        Result.success(repo.createChapter(pid, volumeId, title))
                    } catch (e: Throwable) {
                        Result.failure(e)
                    }
                }
            if (currentProjectId != pid) return@launch
            result
                .onSuccess {
                    val expanded = _uiState.value.expandedVolumeIds + volumeId
                    savedStateHandle[expandedVolumeKey(pid)] = expanded
                    _uiState.value = _uiState.value.copy(expandedVolumeIds = expanded)
                    refreshProject()
                }
                .onFailure { _uiEvents.tryEmit(ProjectTreeUiEvent.Error(errorMessage(it))) }
        }
    }

    fun renameVolume(
        volumeId: String,
        newTitle: String,
    ) {
        val pid = currentProjectId ?: return
        mutateAndRefresh(pid) { repo -> repo.renameVolume(pid, volumeId, newTitle) }
    }

    fun deleteVolume(volumeId: String) {
        val pid = currentProjectId ?: return
        mutateAndRefresh(pid) { repo -> repo.deleteVolume(pid, volumeId) }
    }

    fun renameChapter(
        volumeId: String,
        chapterId: String,
        newTitle: String,
    ) {
        val pid = currentProjectId ?: return
        mutateAndRefresh(pid) { repo -> repo.renameChapter(pid, volumeId, chapterId, newTitle) }
    }

    fun deleteChapter(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        mutateAndRefresh(pid) { repo -> repo.deleteChapter(pid, volumeId, chapterId) }
    }

    fun moveVolumeUp(volumeId: String) {
        val pid = currentProjectId ?: return
        val currentVolumes = _uiState.value.volumes
        val currentIndex = currentVolumes.indexOfFirst { it.id == volumeId }
        if (currentIndex <= 0) return
        val reordered = currentVolumes.toMutableList()
        reordered.add(currentIndex - 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        mutateAndRefresh(pid) { repo -> repo.reorderVolumes(pid, orderedIds) }
    }

    fun moveVolumeDown(volumeId: String) {
        val pid = currentProjectId ?: return
        val currentVolumes = _uiState.value.volumes
        val currentIndex = currentVolumes.indexOfFirst { it.id == volumeId }
        if (currentIndex < 0 || currentIndex >= currentVolumes.size - 1) return
        val reordered = currentVolumes.toMutableList()
        reordered.add(currentIndex + 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        mutateAndRefresh(pid) { repo -> repo.reorderVolumes(pid, orderedIds) }
    }

    fun moveChapterUp(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        val volume = _uiState.value.volumes.find { it.id == volumeId } ?: return
        val currentIndex = volume.chapters.indexOfFirst { it.id == chapterId }
        if (currentIndex <= 0) return
        val reordered = volume.chapters.toMutableList()
        reordered.add(currentIndex - 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        mutateAndRefresh(pid) { repo -> repo.reorderChapters(pid, volumeId, orderedIds) }
    }

    fun moveChapterDown(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        val volume = _uiState.value.volumes.find { it.id == volumeId } ?: return
        val currentIndex = volume.chapters.indexOfFirst { it.id == chapterId }
        if (currentIndex < 0 || currentIndex >= volume.chapters.size - 1) return
        val reordered = volume.chapters.toMutableList()
        reordered.add(currentIndex + 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        mutateAndRefresh(pid) { repo -> repo.reorderChapters(pid, volumeId, orderedIds) }
    }
}
