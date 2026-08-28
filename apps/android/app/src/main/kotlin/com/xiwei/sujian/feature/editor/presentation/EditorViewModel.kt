package com.xiwei.sujian.feature.editor.presentation

// ! # 编辑器 ViewModel（Android UI 层 - ViewModel）
// !
// ! 管理编辑器的 UI 状态、自动保存、设置同步、写作统计。
// !
// ! ## 架构定位
// !
// ! ```text
// ! EditorActivity → EditorViewModel → ProjectRepository → WritingBridge/ProjectBridge/ChapterBridge → Rust Core
// ! ```
// !
// ! ## 职责边界
// !
// ! - **做**：UI 状态管理、自动保存调度、设置加载/应用、写作统计上报、
// !   章节打开事务（latest-wins + 提交前 session 预准备）
// ! - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 BasicTextField/TextLayoutResult 负责）
// ! - **不直接调用 legacy JNI adapter**：只通过 Repository 和领域 Bridge 间接调用
// !
// ! ## 关键流程
// !
// ! 1. **章节加载**：`requestOpenChapter()` → 保存旧章节 → 加载新章节 →
// !   预准备 Rust session → 一次性提交（数据 + 会话 + 导航）
// ! 2. **自动保存**：`onContentChanged()` → `scheduleAutoSave()` → `performSave()`
// ! 3. **设置同步**：`onSettingsChanged()` → `reloadSettings()` → 更新 `EditorSettingsState`
// ! 4. **写作统计**：`onContentChanged()` → `reportWritingEvent()` → `ProjectRepository.processWritingEvent()`
// !
// ! ## 线程模型
// !
// ! - UI 操作在 `Dispatchers.Main`
// ! - 文件 I/O 在 `Dispatchers.IO`
// ! - 保存互斥锁 `saveMutex` 防止并发保存冲突
// !
// ! ## #595 一：章节打开事务
// !
// ! `requestOpenChapter` 是数据、Rust session、窗口和 Navigator 的同一次提交：
// ! 串行门 → requestId 校验 → 冻结旧章节输入 → 保存/flush A → 加载 B →
// ! 为 B 预准备 Rust session（提交前取得有效 snapshot/bind plan）→
// ! 再次确认 requestId 仍为最新 → 一次性提交 active=B/EditorUiState/EditorSessionState →
// ! 调用方提交业务选择并导航。过期请求在每个可见提交边界回滚临时状态并返回
// ! [ChapterSwitchResult.Stale]；保存/加载/session 预准备失败时 A 的状态、
// ! 输入回调和 Navigator 全部保持不变。
// !
// ! 输入窗口防护：提交成功返回后 `inputFrozen` 保持 true，直到新章节的
// ! WritingPane 真实附着编辑器后调用 [confirmEditorAttached] 才解除 —
// ! 旧章节 A 的 View 在"提交 → 导航"窗口期内无法把输入写入已切到 B 的 ViewModel。
// !
// ! ## #595 二：文档版本
// !
// ! 正文更新事实携带 [com.xiwei.sujian.feature.editor.session.DocumentVersion]
// ! （Repository contentHash + 同步 manifest 锚点），不再使用进程内 contentVersion
// ! 计数器；新旧判断由会话层 reducer 按版本锚点 + localDirty 完成。

import android.app.Application
import androidx.compose.ui.text.TextRange
import androidx.core.content.edit
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.feature.editor.input.CommitResult
import com.xiwei.sujian.feature.editor.input.CommittedTextEdit
import com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.interop.TextEditSessionBridge
import com.xiwei.sujian.feature.editor.session.CoreVisualIntentEvent
import com.xiwei.sujian.feature.editor.session.DocumentSaveReceiptTracker
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.TargetDocumentFact
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex

class EditorViewModel(
    application: Application,
) : AndroidViewModel(application) {
    // #595 一：依赖注入 — 必须由 SujianApp 进程级容器提供同一组 Repository。
    // 删除 fallback ProjectRepository(getApplication())/SettingsRepository(getApplication())
    // （旧实现会为首次打开章节创建独立 Repository，绕过进程级依赖容器）。
    internal var _projectRepository: ProjectRepository? = null
    internal var _settingsRepository: SettingsRepository? = null
    internal var _syncRepository: SyncRepository? = null
    internal var _syncStatusRepository: com.xiwei.sujian.feature.sync.data.SyncStatusRepository? = null
    internal var _sessionCoordinator: EditorSessionCoordinator? = null
    internal var _chapterRepository: ChapterRepository? = null
    internal var _recentEditsRepository: RecentEditsRepository? = null
    internal var _statsRepository: WritingStatsRepository? = null

    /**
     * #641 评论1 第2节：AppServiceBridge — bridge 的 commitToCore lambda 需要
     * 经 [TextEditSessionBridge] 调 Rust Core replace。由 [initialize] 注入。
     */
    internal var _appServiceBridge: AppServiceBridge? = null

    /**
     * #641 评论1 第2节：ViewModel 拥有的 TextFieldState bridge 映射 —
     * 按 targetId 生命周期创建/释放，不每次重组重新创建。
     * WritingPane 通过 [bridgeForTarget] 消费同一实例。
     */
    private val _bridges = mutableMapOf<String, EditorTextFieldStateBridge>()

    internal val projectRepository: ProjectRepository
        get() =
            _projectRepository
                ?: error("EditorViewModel 未注入 ProjectRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    internal val chapterRepository: ChapterRepository
        get() =
            _chapterRepository
                ?: error("EditorViewModel 未注入 ChapterRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    internal val recentEditsRepository: RecentEditsRepository
        get() =
            _recentEditsRepository
                ?: error("EditorViewModel 未注入 RecentEditsRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    internal val statsRepository: WritingStatsRepository
        get() =
            _statsRepository
                ?: error("EditorViewModel 未注入 WritingStatsRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    /**
     * #597：章节正文保存端口 — 默认走进程级容器注入的 [chapterRepository]；
     * 测试可替换为可控假实现以驱动真实保存流程（保存期间继续输入不被晚到回执覆盖）。
     */
    internal var chapterSavePort: com.xiwei.sujian.feature.editor.session.ChapterContentSavePort? = null

    internal val effectiveChapterSavePort: com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
        get() = chapterSavePort ?: chapterRepository
    internal val settingsRepository: SettingsRepository
        get() =
            _settingsRepository
                ?: error("EditorViewModel 未注入 SettingsRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    internal val syncRepository: SyncRepository
        get() =
            _syncRepository
                ?: error("EditorViewModel 未注入 SyncRepository — 必须通过 Factory(SujianAppDependencies) 创建")

    // #595 五：同步观察使用 viewModelScope（随 ViewModel 自动取消），
    // 不再创建独立未管理 CoroutineScope。
    internal var syncObserverJob: kotlinx.coroutines.Job? = null

    /**
     * #595 一/二/五：初始化 — 幂等；同步观察与正文 flush 回调只注册一次。
     *
     * [sessionCoordinator] 用于章节打开事务的 session 预准备（提交前取得
     * 有效 snapshot/bind plan，导航后编辑器立即可用）。
     */
    fun initialize(
        projectRepo: ProjectRepository,
        settingsRepo: SettingsRepository,
        syncRepo: SyncRepository? = null,
        syncStatusRepo: com.xiwei.sujian.feature.sync.data.SyncStatusRepository? = null,
        sessionCoordinator: EditorSessionCoordinator? = null,
        chapterRepo: ChapterRepository? = null,
        recentEditsRepo: RecentEditsRepository? = null,
        statsRepo: WritingStatsRepository? = null,
        appServiceBridge: AppServiceBridge? = null,
    ) {
        // #630 评论 5327560790: 首次注入 SettingsRepository 后立刻启动一次初始设置读取，
        // 不等 ON_RESUME 事件 — 保证首帧 typography snapshot 来自持久化权威设置。
        val firstSettingsRepositoryAttach = _settingsRepository == null
        if (_projectRepository == null) _projectRepository = projectRepo
        if (_settingsRepository == null) _settingsRepository = settingsRepo
        if (syncRepo != null && _syncRepository == null) _syncRepository = syncRepo
        if (chapterRepo != null) _chapterRepository = chapterRepo
        if (recentEditsRepo != null) _recentEditsRepository = recentEditsRepo
        if (statsRepo != null) _statsRepository = statsRepo
        if (syncStatusRepo != null && _syncStatusRepository !== syncStatusRepo) {
            _syncStatusRepository = syncStatusRepo
            restartSyncObserver()
        }
        if (sessionCoordinator != null && _sessionCoordinator !== sessionCoordinator) {
            _sessionCoordinator = sessionCoordinator
        }
        if (appServiceBridge != null) {
            _appServiceBridge = appServiceBridge
        }
        if (firstSettingsRepositoryAttach) {
            viewModelScope.launch {
                val snapshot = loadEditorSettingsSnapshot()
                _uiState.value = _uiState.value.copy(settings = snapshot, settingsReady = true)
            }
        }
        // #595 三/四：同步前统一 flush 活动正文（ActiveDocumentGate）。
        registerActiveDocumentGateIfNeeded()
    }

    // #595 三/四：同步前统一 flush 活动正文（ActiveDocumentGate）。
    // 注册携带 owner token（本 VM 实例）：旧实例 onCleared 只能关闭自己的
    // 注册，不能清掉新实例的 flusher（Activity 重建/生命周期交错防护）。
    private fun registerActiveDocumentGateIfNeeded() {
        if (gateRegistration != null) return
        gateRegistration =
            ActiveDocumentGate.register(
                this,
                flush = { requestSave().await() },
                documentIdentity = {
                    val lease = _sessionCoordinator?.currentInputLease()
                    if (lease != null) "${lease.targetId}:${lease.sessionId}:${lease.epoch}" else null
                },
            )
    }

    /**
     * #595 二/三/五：同步完成后检查当前章节磁盘内容是否变更 —
     * 变更时发布版本化文档事实（[TargetDocumentFact]），WritingPane 据此
     * 执行一次 Core reset 以反映同步合并后的新正文。
     *
     * #595 五：baseVersion 按 target 从 EditorSessionStore 读取（活动状态属于
     * 其他 target 时不得使用全局 committedVersion）；sourceVersion 使用真实
     * commit/manifest ID（lastSyncedCommit），不再用 lastSyncTime 时间锚点；
     * parentVersion=同步前磁盘版本 — 后续 flush/应用据此判断因果顺序。
     * internal 暴露 viewModelScope 供 extension functions 使用。
     */
    internal val editorScope: CoroutineScope get() = viewModelScope

    internal val _uiState = MutableStateFlow(EditorUiState())
    val uiState: StateFlow<EditorUiState> = _uiState.asStateFlow()

    internal val _events = Channel<EditorEvent>(Channel.BUFFERED)
    val events = _events.receiveAsFlow()

    // #641：Core 视觉意图事件通道 — presentation/session 层发布的纯数据事件，
    // 不含 Compose/visual 依赖。UI 层（WritingPaneEditorContent）收集后，
    // 用 TextOffsetUtils 把 Core old/new UTF-8 ranges 转成 UTF-16 EditorVisualIntent，
    // 调用 ComposeEditorVisualState.onVisualIntent。
    internal val _visualIntentEvents = Channel<CoreVisualIntentEvent>(Channel.BUFFERED)
    val visualIntentEvents = _visualIntentEvents.receiveAsFlow()

    // #595 二：Repository 真实来源的正文文档事实流 — 按 target 分区的最新事实
    // 总线（带 replay 语义：新 collector 立即拿到该 target 的当前文档事实）。
    internal val documentUpdateBus = TargetDocumentUpdateBus()

    internal fun emitDocumentFact(fact: TargetDocumentFact) {
        documentUpdateBus.emit(fact)
    }

    /** 指定 target 的正文文档事实流 — 只收该 target 的事实，新 collector 先收到当前事实。 */
    fun documentUpdates(targetId: String): kotlinx.coroutines.flow.Flow<TargetDocumentFact> =
        documentUpdateBus.updates(targetId)

    internal var currentSession: EditorSession? = null

    internal var initialWordCount = 0
    internal var sessionStartTime = System.currentTimeMillis()

    internal val saveMutex = Mutex()

    // #624 评论9：pendingSaveContent 已删除 — 保存期间继续输入只标记 dirty（session store）。
    internal var autoSaveJob: kotlinx.coroutines.Job? = null

    /** #624 评论9：延迟刷新 speed 的可取消 Job。 */
    internal var statsRefreshJob: kotlinx.coroutines.Job? = null
    internal var saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
    internal var saveActorJob: kotlinx.coroutines.Job? = null

    /**
     * #595 七：按 target 的保存回执（替代旧全局 lastSaveResult）—
     * Flush 屏障只放行"该 revision 的正文已得到保存回执"的 target。
     */
    internal val saveReceipts = DocumentSaveReceiptTracker()

    // #624 评论12 第2项：contentDirty 已删除 — dirty 唯一真值在 session store
    // （applyLocalUpdate 写入 DocumentState.localDirty，issueDocumentOperationLease
    // 从记录填入 lease.localDirty），ViewModel 不再维护第二份。

    /** #595 四：ActiveDocumentGate 注册句柄 — onCleared 只关闭自己的注册。 */
    internal var gateRegistration: com.xiwei.sujian.app.state.ActiveDocumentGate.Registration? = null

    internal val statsDeviceId: String by lazy {
        val prefs = application.getSharedPreferences("writer_stats", android.content.Context.MODE_PRIVATE)
        var id = prefs.getString("device_id", null)
        if (id == null) {
            id = "android-${java.util.UUID.randomUUID()}"
            prefs.edit { putString("device_id", id) }
        }
        id
    }

    internal var statsSessionId: String = java.util.UUID.randomUUID().toString()
    internal var statsLastEventMs: Long = 0

    // #624 评论9：previousText 已删除 — 统计改增量 recordWritingEvent。
    internal var isLoadingChapter = false

    /**
     * #595 一：输入冻结 — 章节切换事务期间与提交后（导航落地前）为 true；
     * 新章节 WritingPane 真实附着编辑器后经 [confirmEditorAttached] 解除。
     * 防止旧章节 A 的 View 在"提交 → 导航"窗口内把输入写入已切到 B 的 ViewModel。
     */
    @Volatile
    internal var inputFrozen = false

    // #624 评论17 问题3：SyncMergeEmitDedup 已删除 — 发射端不维护 lastEmittedHash。

    // #595 一：章节切换串行门 — 同一时间只允许一个切换事务执行；
    // 请求序号在每个可见提交边界校验，过期请求回滚临时状态并返回 Stale。
    internal val chapterSwitchGate = ChapterSwitchGate()

    /**
     * #595 一：章节切换统一事务入口（手机竖屏、大屏工作台共用）。
     *
     * 固定顺序：串行门 → requestId 校验 → 冻结旧章节输入 → 保存旧章节 →
     * 加载新章节 → 无副作用预准备 Rust session（[PreparedSessionHandle]）→
     * 再次校验 requestId → 一次性提交（active/正文/hash/标题/session）→
     * 返回 Success。调用方只在 Success 之后提交业务选择和 Navigator；失败时
     * 旧 EditorUiState 完整恢复、Navigator 完全不变化。
     *
     * - 过期请求（事务执行期间有更新请求排队）在每个可见提交边界回滚临时
     *   状态并返回 [ChapterSwitchResult.Stale] — 不保存可见状态、不导航；
     * - [kotlinx.coroutines.CancellationException] 重新抛出并恢复旧状态，
     *   不得当作普通加载失败处理；
     * - Success 返回后 [inputFrozen] 保持 true，直到新 pane 附着编辑器。
     *
     *
     * 兼容入口 — 由 WritingPane 在直接进入正文（深链/恢复）时调用；
     * 与 [requestOpenChapter] 共用同一串行锁和 requestId 语义。
     */
    suspend fun switchChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
    ): ChapterSwitchResult = requestOpenChapter(projectId, volumeId, chapterId, chapterTitle)

    /**
     * #595 一：判断给定章节是否为 ViewModel 当前已提交章节。
     * 防止切换事务提交后（业务选择/导航尚未落地的一帧内）旧 WritingPane 用
     * 新章节正文对旧 target 执行 beginEdit/外部替换。
     *
     *
     * #595 一：新章节 pane 真实附着编辑器后解除输入冻结。
     * 只允许当前已提交章节的 target 解除；过期 pane 调用是 no-op。
     *
     *
     * #595 三: 读取 Android 系统无障碍"减少动画"设置（Animator 时长缩放为 0 时启用）。
     * 这是系统级偏好，不属于 Core 编辑器设置；初始值与 Core 默认一致（false）。
     *
     *
     * #595 一：加载章节内容并在成功时一次性提交 loading=false/content/hash，
     * 并发布 RepositoryLoaded 文档事实（真实 hash）。
     * 返回是否加载成功；失败时调用方（switchChapter 事务）负责回退会话指针。
     * #595 一：recordRecentEdit/统计/诊断属于提交完成后的独立操作 —
     * 这里只保留不参与失败判定的统计与诊断；recordRecentEdit 移到提交后。
     *
     *
     * #595 二/三：同步合并事实已应用到 Rust session 后，同步更新 ViewModel 的
     * 正文/hash/保存状态/字数 — 禁止只更新 Rust session 不更新 ViewModel
     * （否则磁盘/Rust session/ViewModel 三份正文分裂）。
     *
     *
     * #595 二：同步下载与本地未保存编辑冲突 — 类型化冲突通知，
     * 不覆盖用户输入（reducer 已拒绝直接 reset）。
     *
     *
     * #595 三/七：同步前 flush — 把当前屏幕正文（revision 精确锚定）推入保存队列
     * 并等待持久化屏障：只有该 revision 的正文已得到保存回执才返回 true。
     *
     * - 空正文 + 显式清空（删除全部内容/菜单清空）→ ClearDocument；
     * - 空正文 + 未编辑（全新空章节，磁盘与屏幕一致）→ 直接 flush（回执来自加载）；
     * - 空正文 + 已编辑但未清空 → 防御性失败（磁盘与屏幕不一致）。
     */

    fun updateChapterNote(newNote: String) {
        val session = currentSession ?: return

        viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            try {
                chapterRepository.updateChapterNote(session.projectId, session.volumeId, session.chapterId, newNote)
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    _uiState.value = _uiState.value.copy(chapterNote = newNote)
                }
            } catch (e: Throwable) {
                launch(kotlinx.coroutines.Dispatchers.Main) {
                    emitErrorEvent(
                        getApplication<Application>().getString(
                            R.string.error_update_chapter_note_failed,
                            e.message ?: "",
                        ),
                    )
                }
            }
        }
    }

    /**
     * #641 评论1 第2节：获取或创建 target 对应的 [EditorTextFieldStateBridge]。
     * bridge 按 targetId 生命周期稳定存活于 ViewModel，不每次重组重建。
     *
     * 初始 selection 来自 Core/session 的真实 UTF-8 selection 经
     * [TextOffsetUtils.utf16TextRangeForUtf8] 转成 UTF-16，不用无条件 TextRange(0,0)。
     *
     * [commitToCore] lambda 内统一 UTF-16→UTF-8（replace offset）和
     * UTF-8→UTF-16（rejection selection），不再把 byte offset 当 Compose offset。
     *
     * #641 评论1 第3节：移除 no-op 成功 fallback — coordinator/appServiceBridge 为空时
     * 不得返回假 Accepted bridge（把未提交到 Core 的输入伪装 Accepted）。
     * 依赖由初始化保证；调用方必须等待真实注入。
     *
     * #641 架构门禁：移除 visualState 参数 — presentation 层不得依赖
     * feature.editor.visual。Core 返回的视觉意图改由 [CoreVisualIntentEvent]
     * 发布到事件通道，UI 层（WritingPaneEditorContent）收集后映射为
     * [EditorVisualIntent] 喂给 [ComposeEditorVisualState]。
     */
    fun bridgeForTarget(
        targetId: String,
        initialText: String,
    ): EditorTextFieldStateBridge {
        _bridges[targetId]?.let { return it }

        val coordinator =
            _sessionCoordinator
                ?: error(
                    "EditorViewModel.bridgeForTarget: sessionCoordinator 未注入 " +
                        "— 必须通过 Factory(SujianAppDependencies) 创建，依赖由初始化保证",
                )
        val bridge =
            _appServiceBridge
                ?: error(
                    "EditorViewModel.bridgeForTarget: appServiceBridge 未注入 " +
                        "— 必须通过 Factory(SujianAppDependencies) 创建，依赖由初始化保证",
                )

        val snapshot = coordinator.queryTargetSnapshot(targetId)
        val initialSelection =
            if (snapshot != null) {
                TextOffsetUtils.utf16TextRangeForUtf8(
                    snapshot.text,
                    snapshot.selectionAnchorUtf8,
                    snapshot.selectionHeadUtf8,
                )
            } else {
                TextRange(0, 0)
            }
        val effectiveInitialText = snapshot?.text ?: initialText

        val commitToCore: (CommittedTextEdit) -> CommitResult = { edit ->
            commitEditToCore(targetId, coordinator, bridge, edit)
        }

        return _bridges.getOrPut(targetId) {
            EditorTextFieldStateBridge(
                initialText = effectiveInitialText,
                initialSelection = initialSelection,
                commitToCore = commitToCore,
            )
        }
    }

    /**
     * #641 评论1 第2节：bridge commitToCore 实现 — UTF-16→UTF-8 偏移转换后调
     * Core replace，成功时 applyLocalEdit 推进 session，失败时回退到权威正文。
     * 从 [bridgeForTarget] 提取以控制方法长度与认知复杂度。
     *
     * 注意：Core 的 newAffectedByteRanges 在 CJK/emoji/多行上依赖完整 newText
     * 做 UTF-8 byte→UTF-16 换算；displayPatches.firstOrNull()?.insertedText 只是
     * 插入片段，不是完整正文，会导致 range 错位。必须用完整 old/new 文本。
     */
    private fun commitEditToCore(
        targetId: String,
        coordinator: EditorSessionCoordinator,
        bridge: AppServiceBridge,
        edit: CommittedTextEdit,
    ): CommitResult {
        val lease = coordinator.currentInputLease()
        if (lease == null || lease.targetId != targetId) {
            return CommitResult.Rejected(edit.oldText, edit.selection)
        }
        val byteStart = TextOffsetUtils.utf8OffsetForCharIndex(edit.oldText, edit.replaceStart)
        val byteEndExclusive = TextOffsetUtils.utf8OffsetForCharIndex(edit.oldText, edit.replaceEndExclusive)
        val kernelBridge = TextEditSessionBridge(bridge, lease.sessionId)
        val currentSnapshot = coordinator.queryTargetSnapshot(targetId)
        val expectedRevision = currentSnapshot?.revision ?: 0L
        val result =
            kernelBridge.replace(
                byteStart = byteStart,
                byteEndExclusive = byteEndExclusive,
                replacementText = edit.newText,
                originalText = edit.oldText,
                cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                expectedRevision = expectedRevision,
            )
        if (result != null) {
            coordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput(
                    targetId = targetId,
                    revision = result.newRevision.toLong(),
                    transactionId = result.transactionId.toLong(),
                    selectionAnchorUtf8 = result.newSelectionStart.toInt(),
                    selectionHeadUtf8 = result.newSelectionEnd.toInt(),
                    lease = lease,
                    contentChanged = result.displayPatches.isNotEmpty(),
                ),
            )
            // #641：Core 返回视觉意图 — 发布纯数据事件到通道，UI 层（WritingPaneEditorContent）
            // 收集后映射为 EditorVisualIntent 喂给 ComposeEditorVisualState。
            // 必须用完整 old/new 文本：删除范围用完整 oldText，插入/移动范围用完整 newText。
            val editResult = com.xiwei.sujian.feature.editor.projection.EditResult.fromDto(result)
            val fullNewText =
                buildString {
                    append(edit.oldText.substring(0, edit.replaceStart))
                    append(edit.newText)
                    append(edit.oldText.substring(edit.replaceEndExclusive))
                }
            viewModelScope.launch {
                _visualIntentEvents.send(
                    CoreVisualIntentEvent(
                        targetId = targetId,
                        oldText = edit.oldText,
                        newText = fullNewText,
                        visualIntent = editResult.visualIntent,
                        oldSelectionEndUtf8 = result.oldSelectionEnd.toInt(),
                        newSelectionEndUtf8 = result.newSelectionEnd.toInt(),
                    ),
                )
            }
            return CommitResult.Accepted
        }
        val fallbackText = currentSnapshot?.text ?: edit.oldText
        val fallbackSelection =
            if (currentSnapshot != null) {
                TextOffsetUtils.utf16TextRangeForUtf8(
                    fallbackText,
                    currentSnapshot.selectionAnchorUtf8,
                    currentSnapshot.selectionHeadUtf8,
                )
            } else {
                edit.selection
            }
        return CommitResult.Rejected(fallbackText, fallbackSelection)
    }

    /**
     * #641 评论1 第2节：切章节/返回时在 save/close 前调用 bridge.flushForClose，
     * 即使 IME 仍有 composition 也先把屏幕最终内容提交给 Core。
     */
    fun flushBridgeForClose(targetId: String) {
        _bridges[targetId]?.flushForClose()
    }

    /**
     * #641 评论1 第2节：外部同步/撤销/重载事实成功时把权威正文写回 bridge state。
     * UTF-8 selection 经 [TextOffsetUtils.utf16TextRangeForUtf8] 转成 UTF-16。
     * composition 时不得覆盖（由调用方在 composition == null 时才调用）。
     */
    fun applyAuthoritativeToBridge(
        targetId: String,
        text: String,
        selectionUtf8Anchor: Int,
        selectionUtf8Head: Int,
    ) {
        val bridge = _bridges[targetId] ?: return
        val utf16Selection = TextOffsetUtils.utf16TextRangeForUtf8(text, selectionUtf8Anchor, selectionUtf8Head)
        bridge.applyAuthoritativeText(text, utf16Selection)
    }

    /** #641：释放 target 对应的 bridge（章节关闭/session 销毁时）。 */
    fun releaseBridge(targetId: String) {
        _bridges.remove(targetId)
    }

    /** #641：获取 target 对应的 bridge（已存在时返回，否则 null）— 供 WritingPane 消费。 */
    fun existingBridgeForTarget(targetId: String): EditorTextFieldStateBridge? = _bridges[targetId]

    /**
     * #641 评论1 第2节：判断指定 target 的 bridge 是否处于 IME composition 中间态。
     * 供外部事实（同步/撤销/重载）路径在 composition 活跃时暂存 pending，
     * 不覆盖输入法正在编辑的 buffer。
     */
    fun isBridgeComposing(targetId: String): Boolean {
        return _bridges[targetId]?.state?.composition != null
    }

    override fun onCleared() {
        super.onCleared()
        syncObserverJob?.cancel()
        autoSaveJob?.cancel()
        statsRefreshJob?.cancel()
        saveCommandChannel.close()
        // #641：释放所有 ViewModel 拥有的 TextFieldState bridge。
        _bridges.clear()
        // #595 四：只关闭自己的 gate 注册 — 新实例的 flusher 不被旧实例清除。
        gateRegistration?.close()
        gateRegistration = null
        try {
            // #624 评论11 第3项：flush 入队同一 writer actor — 不在主线程直接刷盘；
            // Record/Flush 顺序由进程级 Channel 决定。
            statsRepository.flushWritingStats()
        } catch (_: Exception) {
        }
        try {
            recentEditsRepository.flushRecentEdits()
        } catch (_: Exception) {
        }
    }

    /**
     * #624 评论12 第1项：workspace 离开正文后的业务关闭收口。
     *
     * 由 [com.xiwei.sujian.feature.project.ui.ProjectWorkspaceScreen] 的 location
     * observer 在导航成功离开 Editor 后调用：
     * 1. 先 [flushBridgeForClose] 把屏幕最终内容（含 IME composition 上屏）提交给 Core；
     * 2. 再 [releaseBridge] 移除 bridge；
     * 3. 然后调用方（ProjectWorkspaceScreen）调用 [closeTarget] 关闭 Rust session。
     *
     * 清空 ViewModel 的章节身份：
     * - `currentSession = null` — 从章节树再点 A，switchChapterLocked 不再命中
     *   "相同章节直接 Success" 的 no-op，会走完整重新加载；点 B 也不会把已关闭
     *   的 A 当 oldSession 去拿活动 lease（拿不到 → 误报 SaveFailed）；
     * - 取消 autosave、关闭保存命令 channel；
     * - UI 冷状态复位（保留设置字段），不再宣称 A 是当前正文。
     */
    fun finishWorkspaceClose(targetId: String) {
        val session = currentSession ?: return
        if (chapterTargetId(session.projectId, session.volumeId, session.chapterId) != targetId) return
        // #641：关闭前 flush bridge（IME composition 上屏内容提交给 Core）。
        flushBridgeForClose(targetId)
        releaseBridge(targetId)
        currentSession = null
        autoSaveJob?.cancel()
        saveCommandChannel.close()
        _uiState.value = EditorUiState(settings = _uiState.value.settings)
    }

    internal suspend fun emitErrorEvent(message: String) {
        _events.send(EditorEvent.ToastMessage(message))
    }

    /**
     * #595 一：章节正文 target ID — 全局唯一命名空间，ViewModel 与窗口层共用。
     */
    fun chapterTargetId(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): String = "chapter-body:$projectId:$volumeId:$chapterId"

    /**
     * #595 一：显式 Factory — 从 [SujianAppDependencies]（进程级容器）
     * 注入同一组 Repository；删除 fallback getApplication() 路径。
     */
    class Factory(
        internal val application: Application,
        internal val deps: SujianAppDependencies,
        internal val sessionCoordinator: EditorSessionCoordinator?,
    ) : ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            @Suppress("UNCHECKED_CAST")
            val vm = EditorViewModel(application)
            vm.initialize(
                deps.projectRepository,
                deps.settingsRepository,
                deps.syncRepository,
                deps.syncStatusRepository,
                sessionCoordinator,
                deps.chapterRepository,
                deps.recentEditsRepository,
                deps.statsRepository,
                deps.appServiceBridge,
            )
            return vm as T
        }
    }
}
