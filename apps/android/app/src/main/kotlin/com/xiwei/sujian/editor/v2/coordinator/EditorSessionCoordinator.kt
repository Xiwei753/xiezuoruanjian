package com.xiwei.sujian.editor.v2.coordinator

import android.util.Log
import androidx.compose.runtime.Immutable
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy

/**
 * #592 四：窗口绑定状态机 — 会话层唯一的窗口生命周期事实。
 *
 * - [Idle]：无窗口绑定、无活动会话。
 * - [Attaching]：窗口正在绑定（beginEdit 进行中）。
 * - [Attached]：窗口已绑定，输入法/渲染活跃。
 * - [Detaching]：窗口销毁中，正在保存 snapshot。
 * - [Detached]：窗口已销毁，Rust session 与 snapshot 保留，等待新窗口附着。
 * - [Committing] / [Cancelling]：编辑事务结束中。
 *
 * Detached 状态下 commit/cancel 不再依赖 target 对象存在：正文已通过
 * onTextChanged 流式保存，业务关闭（[EditorSessionCoordinator.closeTarget]）
 * 直接关闭 Rust session 并回到 Idle。
 */

/**
 * #592 一/四：#595 四：会话层协调器 — 只管理 Rust session、正文/选区纯数据快照、
 * Undo/Redo 所属 session、活动目标、窗口绑定状态机与编辑事务。
 *
 * 不持有 View、Activity、Choreographer、WindowDisplayFrameClock、窗口几何、
 * Compose mutableState、TextPaint、TargetDisplayRuntime。
 * 由 Activity 级 ViewModel 持有，跨配置变化存活；窗口/渲染对象全部在
 * [EditorWindowHost]（窗口层）。
 *
 * #595 四：per-target 持久事实由 [EditorSessionStore]（Map<TargetId, EditorSessionRecord>）
 * 保存（sessionId 属于所有活动 session，非持久 target 同样记录）；
 * 窗口重绑只改 binding，正文版本/hash/transaction/selection 保留。
 * #595 二：正文版本使用 [DocumentVersion]（Repository/Core 锚点），
 * 不再使用进程内 contentVersion 计数器。
 */
class EditorSessionCoordinator(
    internal val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {

    // ── 纯会话状态 ──
    /** #595 四：per-target 会话记录存储 — 会话层持久事实的唯一来源。 */
    internal val store = EditorSessionStore()

    // #595 三：_sessionStateFlow 是会话层唯一可写 MutableStateFlow — 所有状态变化
    // 通过 [updateSessionState]（原子 [MutableStateFlow.update]）统一推进。
    internal val _sessionStateFlow = MutableStateFlow(EditorSessionState())
    val sessionStateFlow: StateFlow<EditorSessionState> = _sessionStateFlow.asStateFlow()
    val sessionState: EditorSessionState get() = _sessionStateFlow.value

    val activeTargetId: String? get() = _sessionStateFlow.value.activeTargetId

    val editingState: EditingState get() = _sessionStateFlow.value.editingState

    val windowBindingState: WindowBindingState get() = _sessionStateFlow.value.bindingState

    /**
     * #595 三：活动 session — 从唯一 SessionState 快照派生（sessionId + 活动 target）。
     * Detached 时 activeTargetId 为 null，本 getter 同样返回 null
     * （会话已保留在 store 记录中，由 closeTarget 收口）。
     */
    internal val activeSessionId: ULong?
        get() = _sessionStateFlow.value.sessionId?.takeIf {
            _sessionStateFlow.value.activeTargetId != null
        }

    internal val _targetDecorationsVersionFlow = MutableStateFlow(0L)
    val targetDecorationsVersionFlow: StateFlow<Long> = _targetDecorationsVersionFlow.asStateFlow()
    val targetDecorationsVersion: Long get() = _targetDecorationsVersionFlow.value

    internal val _lastCommittedTextFlow = MutableStateFlow<String?>(null)
    val lastCommittedTextFlow: StateFlow<String?> = _lastCommittedTextFlow.asStateFlow()
    val lastCommittedText: String? get() = _lastCommittedTextFlow.value

    // ── #595 二：输入 lease / epoch ──

    /**
     * 输入 lease epoch — 章节切换提交、业务关闭、窗口解绑时递增，使旧窗口
     * 持有的 lease 失效。所有编辑回调必须携带绑定时的 lease，Coordinator
     * 只接受仍匹配当前活动 target、session 和 epoch 的事件。
     */
    @Volatile
    internal var inputLeaseEpoch = 0L

    /** #595 二：文档操作 ID 生成器 — 每次签发 DocumentOperationLease 递增。 */
    internal val operationIdCounter = java.util.concurrent.atomic.AtomicLong(0)

    /**
     * 窗口绑定成功时签发当前 lease — 窗口层在 performViewBind 时取得，
     * 之后每次 onLocalEdit/onExternalEdit/onContentChanged 提交都携带。
     * 无活动 target（未绑定）时返回 null — 没有可接收输入的会话。
     */
    fun currentInputLease(): EditorInputLease? {
        val s = _sessionStateFlow.value
        val targetId = s.activeTargetId ?: return null
        return EditorInputLease(targetId, s.sessionId ?: 0UL, inputLeaseEpoch)
    }

    /** 使当前所有输入 lease 失效 — 旧 View 晚到的回调不能再进入会话层。 */
    fun invalidateInputLease() {
        inputLeaseEpoch++
    }

    /**
     * #595 二：签发文档操作租约 — 保存/同步开始时一次性取得完整不可变文档快照。
     *
     * 包含 target/session/epoch/revision/text/committedVersion 全部字段，
     * 调用方不再自行拼接 currentSession + sessionState。无活动 target 或
     * 无 session 时返回 null（无可操作的文档）。
     */
    fun issueDocumentOperationLease(): DocumentOperationLease? {
        val s = _sessionStateFlow.value
        val targetId = s.activeTargetId ?: return null
        val sessionId = s.sessionId ?: return null
        val record = store.record(targetId) ?: return null
        return DocumentOperationLease(
            operationId = operationIdCounter.incrementAndGet(),
            targetId = targetId,
            coreSessionId = sessionId,
            inputEpoch = inputLeaseEpoch,
            rustRevision = s.revision,
            text = s.text,
            committedVersion = record.documentState.committedVersion,
        )
    }

    /**
     * #595 二：校验文档操作租约是否仍匹配当前活动文档。
     *
     * 保存/同步返回后由同一 reducer 判断：
     * - target/session/epoch 完全一致 → 操作有效；
     * - 任一字段不匹配 → 只记录旧版本确实落盘，当前文档保持 Unsaved。
     */
    fun isDocumentOperationLeaseCurrent(lease: DocumentOperationLease): Boolean {
        val s = _sessionStateFlow.value
        if (s.activeTargetId != lease.targetId) return false
        val expectedSession = s.sessionId ?: return false
        if (expectedSession != lease.coreSessionId) return false
        return inputLeaseEpoch == lease.inputEpoch
    }

    /**
     * 校验事件携带的 lease 是否仍匹配当前活动 target/session/epoch。
     * 窗口层在转发 onContentChanged 前也使用同一校验。
     *
     * 无活动绑定（Idle/Detached）时只校验 epoch + target — 此时窗口回调已
     * 被清除，晚到事件由 epoch 拒绝（commit/close/detach 都递增 epoch），
     * 不允许旧 View 在切换提交后写入新会话。
     */
    fun isInputLeaseCurrent(lease: EditorInputLease?, eventTargetId: String? = null): Boolean {
        if (lease == null) return false
        if (lease.epoch != inputLeaseEpoch) return false
        if (eventTargetId != null && lease.targetId != eventTargetId) return false
        val s = _sessionStateFlow.value
        val active = s.activeTargetId
        if (active == null) return true
        if (active != lease.targetId) return false
        val expectedSession = s.sessionId ?: 0UL
        return lease.sessionId == expectedSession
    }

    // #595 七：只保留一个可写事实源 — MutableStateFlow<EditorMotionPolicy>。
    internal val _motionPolicyFlow = MutableStateFlow(EditorMotionPolicy())
    val motionPolicyFlow: StateFlow<EditorMotionPolicy> = _motionPolicyFlow.asStateFlow()

    /**
     * #595 三：唯一状态更新入口（reducer）— 所有会话状态变化通过
     * [MutableStateFlow.update] 原子推进 [_sessionStateFlow]（CAS 重试，
     * 不存在读后写竞态窗口）。不得在其他位置直接赋值 [_sessionStateFlow]
     * 或创建第二套可写 Flow。
     */
    internal fun updateSessionState(transform: (EditorSessionState) -> EditorSessionState) {
        _sessionStateFlow.update(transform)
    }

    /**
     * #595 三/七：原子应用 [EditorMotionPolicy] — 唯一可写事实源。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        _motionPolicyFlow.value = policy
    }

    fun getMotionPolicy(): EditorMotionPolicy = _motionPolicyFlow.value

    // ── #595 一：本地 IME/键盘编辑 ──

    /**
     * #595 一：应用本地 IME/键盘编辑 — 更新唯一 SessionState，不触发 reset。
     *
     * 由 [EditorWindowHost.installContentCallback] 在 SujianEditorView 产生
     * EditResult 后调用。revision/transactionId 来自 Rust EditResult，
     * selectionAnchor/Head 来自 pipeline mirror（真实选区）。
     * #595 二：本地输入置 localDirty=true（存在未落盘编辑），
     * 外部版本不得直接 reset 覆盖。
     * #595 四：sessionId 从 store 记录读取 — 非持久 target 同样有 sessionId。
     * #595 二：必须携带窗口绑定时的 [EditorInputLease] — 旧章节 View 在切换
     * 提交后晚到的输入（epoch/target 不匹配）被拒绝，不得写入新章节会话。
     */

    /**
     * #595 二：撤销/恢复事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 UNDO_RESTORED。
     * 撤销/恢复改变正文但保留在 session 内（Undo 栈就是本地历史），
     * 不改变 localDirty 的既有事实（撤销后正文仍未落盘时保持 dirty）。
     * #595 二：陈旧 lease（章节切换后旧 View 晚到的撤销事件）拒绝。
     */

    /**
     * #595 二：程序化替换事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 PROGRAMMATIC_REPLACE。
     * #595 二：陈旧 lease（章节切换后旧 View 晚到的程序化结果）拒绝。
     */

    // ── #595 二：外部文档事实（Repository 加载 / 同步合并）──

    /**
     * #595 二：外部文档事实的新旧判断 — 文档版本锚点 + localDirty。
     *
     * 规则（与 Issue #595 解决二/五一致）：
     * - 空版本 → 不可应用；
     * - 同 sourceVersion 重放 → 忽略（幂等，新 collector 读到当前文档事实也不会
     *   再次执行副作用）；
     * - 外部 sourceVersion 可比较且旧于 committedVersion → 忽略；
     * - localDirty=true → 冲突 — 禁止直接 reset（本地输入不得被同步下载覆盖），
     *   调用方必须发布类型化冲突；
     * - 正文一致 → 忽略（无需 reset）；
     * - 从未建立任何版本（空 committed）→ 可应用（首次加载/首次事实）；
     * - 两侧版本不可比较（无相同 revision 锚点、incoming 父版本链不含
     *   committed）且正文不同 → 不得默认 Apply — 进入重新读取/三方合并/冲突
     *   （旧实现把"不可比较"当作"可应用"，旧 IO 结果最后返回仍可能覆盖本地）。
     */

    /**
     * 版本新旧比较 — 只有同一单调 revision 锚点可比较时才判定"旧"：
     * 两侧都携带非零 [DocumentVersion.repositoryRevision] → 比较之；
     * 否则视为不可比较（不判定"旧"，由 [isComparable] 决定是否可应用）。
     * 时间锚点（lastSyncTime）不得参与因果比较。
     */

    /**
     * #595 二：外部文档事实已应用后记录版本 — 更新 store 记录与活动 SessionState。
     *
     * 只记录版本事实（committedVersion=sourceVersion、localDirty=false），
     * 不执行 Core reset（reset 由调用方在 decision==Apply 时经
     * [resetPersistentSession] 执行，最终 revision 来自 reset 后的真实 snapshot）。
     * IgnoreSameContent 时调用本方法同样安全（幂等记录版本）。
     *
     * #595 五：session 被 reset 到新正文后，它的新 base 就是 sourceVersion
     * （旧实现保留 fact.baseVersion — 旧 base 会让下一次同步仍以过时祖先判断
     * 冲突）。Repository 加载/同步合并都来自磁盘，磁盘已处于 sourceVersion，
     * 因此 lastSavedVersion 同步推进。
     */

    /**
     * #595 二：保存成功上报 — 由 [com.xiwei.sujian.ui.EditorViewModel] 在保存
     * 成功后调用。保存回执必须作为文档提交原子推进：
     *
     * ```text
     * committedVersion = savedVersion
     * sessionBaseVersion = savedVersion
     * lastSavedVersion = savedVersion
     * localDirty = false
     * ```
     *
     * 正文已落盘，下一次同步以磁盘版本为三方合并基础；同步合并事实携带
     * parentVersion=该版本时即可比较并安全应用。
     */

    /**
     * #595 五：按 target 从 store 读取 committedVersion — 同步事实的 baseVersion
     * 必须按 target 查询（旧实现读全局 sessionState.committedVersion，活动状态
     * 属于其他 target 时 B 的同步事件会携带 A 的 base）。
     */

    // ── 纯数据目标元数据（窗口层 registerTarget/updateTargetSpec 镜像）──

    /**
     * #595 一/四：注册 target 元数据（无窗口对象，供章节切换事务预准备 session 使用）。
     * 幂等：已注册时更新 profile/persistent，保留 sessionId 与文档事实。
     */
    fun registerTargetMeta(targetId: String, profile: TextEditorProfile, persistent: Boolean) {
        val existing = store.record(targetId)
        if (existing != null) {
            store.update(targetId) { it.copy(profile = profile, persistent = persistent) }
        } else {
            store.put(EditorSessionRecord(targetId = targetId, profile = profile, persistent = persistent))
        }
    }

    fun registerTarget(target: EditableTextTarget) {
        registerTargetMeta(target.targetId, target.profile, target.isPersistent)
    }

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
    ) {
        profile?.let { registerTargetMeta(targetId, it, store.record(targetId)?.persistent ?: false) }
    }

    fun isTargetPersistent(targetId: String): Boolean = store.record(targetId)?.persistent ?: false

    /** #595 四：所有活动 session 都有 ID — 非持久 target 同样返回记录中的 ID。 */
    fun getPersistentSessionId(targetId: String): ULong? = store.record(targetId)?.sessionId

    fun isTargetRegistered(targetId: String): Boolean = store.isRegistered(targetId)

    /**
     * #595 一：无副作用预准备 — 章节切换事务在最终 requestId 校验前调用。
     *
     * 只允许：读取 B 的记录、验证或新建 B session、读取 snapshot、返回句柄。
     * 禁止：commit/cancel A、修改 activeTargetId、修改 WindowBindingState、
     * 修改全局 EditorSessionState、关闭任何既有有效 session。
     *
     * 事务的提交（A→B 一次性切换）由 [commitPreparedSession] 完成；
     * 回滚由 [releasePreparedTarget] 完成（newlyCreated 才关闭 session，
     * 借用的既有 session 恢复 previousRecord 保留 Undo 历史）。
     *
     * 记录中已有有效 session 时直接复用（Undo/Redo、composition、selection
     * 原样保留）；无效/缺失时新建（唯一一次 Core 命令，snapshot 由 create
     * 后的真实内核读取）。snapshot 读取失败视为预准备失败（返回 null，
     * 新建的临时 session 立即关闭，不留下无 snapshot 的孤儿 session）。
     */

    /**
     * #595 一：提交预准备 — 最终 requestId 校验通过后一次性执行 A→B 切换：
     *
     * ```text
     * 1. 冻结并撤销 A 的输入 lease（epoch++，旧 View 晚到一帧也不能写入 B）；
     * 2. 提交旧活动目标 A（persistent + 窗口绑定保持 Rust session，Undo 保留；
     *    非持久/未绑定关闭）；
     * 3. 激活 B：activeTargetId=B、Attaching(prepared)、BINDING，
     *    snapshot 装入 SessionState（正文/revision/选区），文档事实保留。
     * ```
     *
     * 窗口层 beginEdit 复用同一 session 并把 windowId 替换为真实窗口。
     * 失败（记录被并发移除或 session 不再匹配）返回 false，调用方必须回滚。
     */

    /**
     * #595 一：回滚预准备 — Abort 规则：
     *
     * - [PreparedSessionHandle.newlyCreated]=true → 只关闭本事务新建的临时 session
     *   并移除记录（若记录仍指向该 session）；
     * - newlyCreated=false → 恢复事务前的 [PreparedSessionHandle.previousRecord]
     *   （文档事实/selection/装饰快照），不关闭借用的既有 session — 回滚不得
     *   销毁事务开始前已经存在的 B session 与 Undo 历史。
     *
     * 无副作用：准备阶段从未修改全局 EditorSessionState，这里同样不修改。
     */

    // ── 纯数据投影快照（窗口层读写）──

    fun saveProjectionSnapshot(targetId: String, snapshot: ProjectionSnapshot) {
        store.update(targetId) { it.copy(projection = snapshot) }
    }

    fun getProjectionSnapshot(targetId: String): ProjectionSnapshot? = store.record(targetId)?.projection

    // ── 窗口绑定状态机 ──

    /**
     * #592 二：Compose onDispose 唯一入口 — 只解除窗口绑定，不关闭持久 Rust session。
     *
     * persistent target：捕获真实 snapshot 并进入 [WindowBindingState.Detached]，
     * Rust session、Undo/Redo、revision、文档事实全部保留，新窗口可自动附着。
     * 非 persistent（草稿）target：关闭临时 session、删除记录并回到 Idle。
     *
     * 关闭持久 session 必须由业务事件 [closeTarget] 触发（返回章节列表、切换章节、
     * 删除章节），配置变化不改变 workspace route，因此不会关闭 session。
     * #595 四：只清理本 target 的窗口状态，不得把其他活动 target 的 binding
     * 状态一并清成 Idle。
     */

    /**
     * #592 二：窗口绑定完成（视图已 bind/attach 成功）。
     * 由 [EditorWindowHost] 在 View 真实绑定成功后调用。
     *
     * #595 三：防御性状态守卫 — Attached 只能从 Attaching 进入。
     */

    /**
     * #592 三：#595 四：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、
     * 切换章节、删除章节）。与窗口解绑 [detachWindowBinding] 分开：关闭会销毁
     * Rust session，解绑只解除窗口引用。
     *
     * 只有关闭的 target 是当前活动/当前 SessionState 的 target 时才重置全局状态；
     * 关闭非活动 target 不得清掉活动 target 的 binding/editing。
     * 关闭同时使该 target 的输入 lease 失效（旧 View 晚到的回调不再被接受）。
     */


    /**
     * 准备会话绑定 — 创建/复用 session 并设置活动状态。
     * 返回绑定信息或 null（失败时）。
     *
     * #592 一：复用既有持久 session 时，绑定信息携带 Rust 的真实
     * textEditSessionSnapshot（text/revision/cursor/selection），窗口层据此执行
     * attachSnapshot，不再用新 Compose target 的正文/末尾光标执行 loadText
     * （那会 revision+1 并清空 Undo/Redo）。
     * #595 二：新建 session 同样携带 create 后的真实 snapshot（createSession 已
     * 接收初始正文，是唯一一次 Core 命令），窗口层 attachSnapshot 只重建本地镜像。
     * #595 一：章节切换事务预准备 session 时传入 windowId="prepared" —
     * 窗口层 beginEdit 复用同一 session，completeWindowAttach 时替换为真实 windowId。
     * #595 四：sessionId 写入 store 记录 — 非持久 target 同样记录。
     */

    fun forceEditingState(state: EditingState) {
        updateSessionState { it.copy(editingState = state) }
    }

    /**
     * 提交活动编辑会话。
     *
     * - Attached：persistent 会话保持打开（软重置语义），记录保留，由窗口层继续复用。
     * - Detached/非持久：直接关闭 Rust session 并删除记录（正文已流式保存）。
     * 不依赖 target 对象存在，Detached 状态下也能完整收口。
     */

    /**
     * 取消活动编辑会话 — Detached 状态下同样完整收口（关闭 session，不依赖 target 对象）。
     */


    /**
     * #595 一：原子提交候选 session 的真实 snapshot — 一次性更新 store 记录与活动
     * SessionState 的 text/revision/selection，消除 Rust session（新正文）/
     * SessionStore（旧正文）/ViewModel（新正文+hash）三份状态分裂。
     *
     * 候选 session 原子交换语义：
     * - [sessionId] 是已创建并装入新正文的候选 session（不是被原地 reset 的旧 session）；
     * - [oldSessionIdToClose] 是提交成功后要关闭的旧 session（null 表示无旧 session）；
     * - snapshot 读取失败时关闭候选 session、不关闭旧 session、不推进 store/state，
     *   返回 [ExternalResetResult.Failed] — 旧 session 的 Undo/Redo/composition/正文
     *   完整保留（旧实现原地 reset 旧 session 后 snapshot 失败，旧 session 已被破坏）。
     */

    /**
     * Detached 状态下外部内容重置后，刷新保留的 snapshot，新窗口附着时读到最新状态。
     * #595 五：返回 reset 后的真实 snapshot 供 [resetPersistentSession] 上报。
     */

    // ── SessionCommandPort implementation (bridge-level, no View) ──

    /** 按 sessionId 直接读取真实 snapshot（不依赖持久注册）。 */
    internal fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? {
        if (!validateSession(sessionId)) return null
        return when (val result = appServiceBridge.textEditSessionSnapshot(sessionId)) {
            is BridgeResult.Success -> {
                val snap = result.data ?: return null
                TargetSnapshot(
                    text = snap.text,
                    cursorUtf8 = snap.cursor.toInt(),
                    revision = snap.revision.toLong(),
                    selectionAnchorUtf8 = snap.selectionAnchor.toInt(),
                    selectionHeadUtf8 = snap.cursor.toInt()
                )
            }
            else -> null
        }
    }

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? {
        val sessionId = store.record(targetId)?.sessionId ?: return null
        if (sessionId == 0UL) return null
        return querySnapshotForSession(sessionId)
    }

    /**
     * Bridge 级命令执行（不接触投影/View）— 由窗口层 [EditorWindowHost.applyTargetCommand]
     * 在取得结果后负责应用到活动 View 或非活动投影运行时。
     */
    fun executeTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        val sessionId = store.record(targetId)?.sessionId
            ?: return TargetCommandResult.Failed(TargetCommandError.NO_PERSISTENT_SESSION)
        if (sessionId == 0UL || !validateSession(sessionId)) {
            return TargetCommandResult.Failed(TargetCommandError.SESSION_INVALID)
        }

        val snapshotBefore = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        val dtoResult = when (command) {
            is TargetCommand.Replace -> {
                bridge.replace(
                    command.byteStart, command.byteEndExclusive,
                    command.replacementText, command.originalText,
                    uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
                    snapshotBefore.revision
                )
            }
            is TargetCommand.ReplaceAll -> {
                bridge.replaceAll(
                    command.searchText, command.replacementText,
                    snapshotBefore.revision
                )
            }
            is TargetCommand.SetSelection -> {
                bridge.setSelection(
                    command.anchorUtf8, command.headUtf8,
                    snapshotBefore.revision
                )
            }
        }

        if (dtoResult == null) {
            return TargetCommandResult.Failed(TargetCommandError.KERNEL_NULL_RESULT)
        }

        val editResult = com.xiwei.sujian.editor.v2.mirror.EditResult.fromDto(dtoResult)
        if (!editResult.isApplied()) {
            return TargetCommandResult.Failed(TargetCommandError.KERNEL_REJECTED)
        }

        val snapshotAfter = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        return TargetCommandResult.Success(snapshotAfter)
    }

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult =
        executeTargetCommand(targetId, command)

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
        val existing = store.record(targetId)
        if (existing != null) {
            store.update(targetId) { it.copy(decorations = decorations) }
        } else {
            store.put(EditorSessionRecord(targetId = targetId, decorations = decorations))
        }
        _targetDecorationsVersionFlow.value++
    }

    fun getTargetDecorations(targetId: String): TargetDecorations? = store.record(targetId)?.decorations

    // ── Session lifecycle ──

    internal fun createSession(targetId: String, text: String, cursorByteOffset: Int, isPersistent: Boolean): ULong? {
        return when (val result = appServiceBridge.textEditSessionCreate(
            targetId,
            text,
            cursorByteOffset.toUInt(),
            isPersistent
        )) {
            is BridgeResult.Success -> {
                val id = result.data
                if (id == null || id == 0UL) {
                    Log.e(TAG, "createSession($targetId): Core returned null/0 session id")
                    null
                } else {
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(id.toString(), "create")
                    id
                }
            }
            else -> {
                Log.e(TAG, "createSession($targetId): Core session creation failed")
                null
            }
        }
    }

    internal fun closeSession(sessionId: ULong) {
        if (sessionId == 0UL) return
        when (appServiceBridge.textEditSessionClose(sessionId)) {
            is BridgeResult.Success -> { }
            else -> { }
        }
    }

    internal fun validateSession(sessionId: ULong): Boolean {
        if (sessionId == 0UL) return false
        return appServiceBridge.textEditSessionSnapshot(sessionId) is BridgeResult.Success
    }


    companion object {
        internal const val TAG = "EditorSessionCoordinator"
    }
}

data class SessionBindInfo(
    val sessionId: ULong,
    val profile: TextEditorProfile,
    val isPersistent: Boolean,
    /**
     * #592 一：既有持久 session 的真实 Rust snapshot（text/revision/cursor/selection）。
     * 非 null 时窗口层必须走 attachSession（不调用 textEditSessionLoadText），
     * 保证 Undo/Redo 与 composition 不被重置；新建 session 为 null。
     * #595 三：snapshot 为 null 时窗口层拒绝绑定（不允许 fallback 二次 loadText）。
     */
    val snapshot: TargetSnapshot? = null,
)
