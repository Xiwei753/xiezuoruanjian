// EditorMotionSourcePool — 稳定运动字形源池。
//
// Issue #879 最新复核评论修复（问题3）：
// - 源池 sourceRevision 只保存可共享的 StyledString 内容（同一 revision 的内容确实一致）。
// - TextController + mounted + layoutReady 状态放在与真实节点 1:1 的 renderNodeKey 窗口条目中，
//   不再按 old/new-revision 归并——避免某一窗口先 mounted 后，后续同版本新窗口的
//   item.mounted 从 source 读到 true，导致新 Text 的 onAppear 跳过 setStyledString 而空白。
// - layoutReady 由真实 ComponentObserver('layout') 回调设置，不再用 setTimeout 充当证据。
//
// 核心问题：旧实现每次动画都用 ForEach 创建新的 Text 组件，key 用自增的 runIdPrefix——
// 每笔输入都销毁、重建整份运动文字组件。这导致文字源要等
// Text.onAppear → setStyledString → layout 才能真正绘制，先走了几十帧再出现字，效果跳帧。
//
// 解决方案：让源 Text 先常驻并排版好，运动帧仅改变 clip 和位置。
// - 同一 runId 的 Text 不会因新事务而重建——只更新 clip 和位置。
// - 旧字源到新画面的交棒结束才释放（markedForRelease → releaseMarkedSources）。

/**
 * 运动字形源池条目——只保存可共享的内容。
 *
 * 每个条目对应一个稳定的排版源 Text——旧正文源或新正文源。
 * runId 基于 sourceKind + revision，不随事务自增，确保同一源的 Text 常驻不重建。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * MotionSourceEntry 只保存可共享的 StyledString 内容（同一 revision 的内容确实一致）。
 * TextController/mounted/layoutReady 移到 MotionWindowState（按 renderNodeKey 管理）。
 */
export interface MotionSourceEntry {
  /** 稳定的 run 身份标识——基于 sourceKind + revision，不随事务自增 */
  runId: string
  /** 该源对应的正文 revision */
  sourceRevision: number
  /** 该源的完整排版内容——同一 revision 的 StyledString 内容一致，可共享 */
  styledString: StyledString
  /** 旧正文源还是新正文源 */
  sourceKind: 'old' | 'new'
  /** 是否标记为可释放（交棒完成后才真正释放） */
  markedForRelease: boolean
}

/**
 * 窗口级状态——与真实节点 1:1 对应。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * 每个 renderNodeKey 对应一个独立的 ArkUI Text 节点，拥有自己的 TextController、
 * mounted 和 layoutReady 状态。不再按 old/new-revision 归并——
 * 避免某一窗口先 mounted 后，后续同版本新窗口从 source 读到 mounted=true，
 * 导致新 Text 的 onAppear 跳过 setStyledString 而空白。
 *
 * - renderNodeKey：以 sourceKind + sourceRevision + windowId 构造的渲染节点身份标识
 * - textController：每个窗口独立的 TextController（ArkUI 不允许同一 Controller 绑定多个 Text）
 * - mounted：本窗口的 Text 是否已挂载（onAppear 已触发）
 * - layoutReady：本窗口的布局是否已完成（ComponentObserver 'layout' 回调已触发）
 */
export interface MotionWindowState {
  /** 窗口渲染节点身份标识——与 MotionGlyphWindow.renderNodeKey 对应 */
  renderNodeKey: string
  /** 独立的 TextController——每个窗口有自己的 controller，不共享 */
  textController: TextController
  /** 本窗口的 Text 是否已挂载（onAppear 已触发） */
  mounted: boolean
  /** 本窗口的布局是否已完成（ComponentObserver 'layout' 回调已触发） */
  layoutReady: boolean
  /**
   * Issue #879 评论6087544240 问题1：逻辑 owner 集合（幂等管理）。
   *
   * 元素为 'displayed' 或 'candidate'，由 reconcileWindowOwners 一次性设置。
   * 旧实现用 displayedRefCount/candidateRefCount 在 build 阶段逐次 ++，
   * 但 build() 在 ArkUI 构建/重绘阶段被反复调用（动画逐帧、光标移动、候选切换），
   * 同一物理 Text 从头到尾只创建/销毁一次，计数可能从 1 累加到上千，
   * 结束时仅减 1，永远不会删掉 windowState。
   *
   * 改用 Set<string> 后，reconcileWindowOwners 对同一集合应用任意次结果不变（幂等），
   * owner 集合为空且 mounted === false（物理节点已卸载）时才删除 windowState。
   */
  owners: Set<string>
}

/**
 * Issue #879 最新复核评论问题1：源池存活根集合。
 *
 * 一个源是否可以被释放，只能由「谁还在引用它」决定：
 * - runIds：displayed 帧 ∪ candidate 帧的窗口 runId——运动层正在绘制这些字形，
 *   释放它们会让可见文字突然消失；
 * - revisions：displayed 版本 ∪ candidate 版本的正文 revision——主 Text 与候选
 *   主文本源正在使用该版本的 StyledString。
 *
 * 两阶段发布期间 displayed 与 candidate 两个 owner 都算存活；
 * 释放只发生在 owner 退出（提交完成、候选被取代、会话切换）之后的下一轮收集里。
 */
export interface MotionSourceRoots {
  /** displayed ∪ candidate 帧的窗口 runId */
  runIds: Set<string>
  /** displayed ∪ candidate 依赖的正文 revision */
  revisions: Set<number>
}

/**
 * 稳定运动字形源池。
 *
 * 管理 MotionSourceEntry（可共享内容）和 MotionWindowState（窗口级状态）的
 * 创建、获取、标记释放和实际释放。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * - sources：按 runId（sourceKind + revision）管理可共享的 StyledString 内容
 * - windowStates：按 renderNodeKey 管理窗口级状态（TextController/mounted/layoutReady）
 * - 同一 revision 的多个窗口共享同一个 source 的 StyledString，但各自有独立的窗口状态
 *
 * 使用方式：
  * 1. 运动开始时，调用 getOrCreateSource 获取或创建 old/new 源（可共享内容）。
  * 2. 为每个运动窗口调用 getOrCreateWindowState 获取或创建窗口级状态。
  * 3. ForEach key 使用 renderNodeKey（稳定身份），确保同一窗口的 Text 常驻不重建。
  * 4. Text.onAppear 中调用 markWindowMounted 标记本窗口已挂载。
  * 5. ComponentObserver 'layout' 回调中调用 markWindowLayoutReady 标记本窗口布局完成。
  * 6. 每轮按存活根集合 releaseUnusedSources 释放没人引用的源。
  * 7. prepareMotionSources 中调用 reconcileWindowOwners 幂等对齐 owner 集合。
  * 8. Text.onDisAppear 中调用 markWindowUnmounted 标记物理卸载（不删除状态）。
  */
export class EditorMotionSourcePool {
  /** 可共享的源内容——按 runId（sourceKind + revision）管理 */
  private sources: Map<string, MotionSourceEntry> = new Map()
  /** 窗口级状态——按 renderNodeKey 管理，与真实节点 1:1 对应 */
  private windowStates: Map<string, MotionWindowState> = new Map()

  // ── 源内容管理 ──

  /**
   * 获取或创建源（可共享内容）。
   *
   * 如果已有匹配的源（sourceKind + revision），返回现有条目——不创建新的。
   * 同一 revision 的 StyledString 内容应一致，可被多个窗口共享。
   *
   * @param sourceKind 旧正文源（'old'）或新正文源（'new'）
   * @param revision 该源对应的正文 revision
   * @param styledString 该源的完整排版内容
   * @returns 匹配的现有条目或新创建的条目
   */
  getOrCreateSource(
    sourceKind: 'old' | 'new',
    revision: number,
    styledString: StyledString
  ): MotionSourceEntry {
    const runId = `${sourceKind}-${revision}`
    const existing = this.sources.get(runId)
    if (existing !== undefined) {
      // 已有匹配的源——返回现有条目（不创建新的）
      // 同一 revision 的 StyledString 内容应一致，无需更新
      return existing
    }
    // 创建新条目——只保存可共享的内容，不保存窗口级状态
    const entry: MotionSourceEntry = {
      runId,
      sourceRevision: revision,
      styledString,
      sourceKind,
      markedForRelease: false
    }
    this.sources.set(runId, entry)
    return entry
  }

  /**
   * 标记源为可释放。
   *
   * 交棒完成后（动画结束或新源已接管）才调用此方法标记旧源。
   * 实际释放在 releaseMarkedSources 中执行。
   *
   * @param runId 要标记释放的源的 runId
   */
  markForRelease(runId: string): void {
    const entry = this.sources.get(runId)
    if (entry !== undefined) {
      entry.markedForRelease = true
    }
  }

  /**
   * 真正释放已标记的源。
   *
   * 在交棒完成后调用——删除所有 markedForRelease 为 true 的源。
   * 释放后，对应的 Text 组件将从组件树中移除（ForEach 不再包含该 runId）。
   */
  releaseMarkedSources(): void {
    const toDelete: string[] = []
    this.sources.forEach((entry: MotionSourceEntry, key: string) => {
      if (entry.markedForRelease) {
        toDelete.push(key)
      }
    })
    for (const key of toDelete) {
      this.sources.delete(key)
    }
  }

  /**
   * 按存活根集合释放没人引用的源。
   *
   * Issue #879 最新复核评论问题1：存活根只有两个来源——
   * displayed/candidate 帧的窗口 runId，以及 displayed/candidate 的正文 revision。
   * 不在任何根里的源说明既没有窗口在画它、也不属于任何在屏版本，才可以释放。
   *
   * @param roots 存活根集合（displayed ∪ candidate ∪ 主文本源）
   */
  releaseUnusedSources(roots: MotionSourceRoots): void {
    this.sources.forEach((entry: MotionSourceEntry) => {
      if (!roots.runIds.has(entry.runId) && !roots.revisions.has(entry.sourceRevision)) {
        entry.markedForRelease = true
      }
    })
    this.releaseMarkedSources()
  }

  /**
   * 获取所有活跃源（未标记释放）。
   *
   * 供 ForEach 渲染使用——只渲染活跃的源，不渲染已标记释放的源。
   *
   * @returns 未标记释放的源条目列表
   */
  getActiveSources(): MotionSourceEntry[] {
    const result: MotionSourceEntry[] = []
    this.sources.forEach((entry: MotionSourceEntry) => {
      if (!entry.markedForRelease) {
        result.push(entry)
      }
    })
    return result
  }

  /**
   * 获取特定源。
   *
   * @param runId 源的 runId
   * @returns 匹配的源条目，或 undefined（不存在）
   */
  getSource(runId: string): MotionSourceEntry | undefined {
    return this.sources.get(runId)
  }

  // ── 窗口级状态管理 ──

  /**
   * 获取或创建窗口级状态。
   *
   * Issue #879 最新复核评论修复（问题3）：
   * 每个 renderNodeKey 对应一个独立的 ArkUI Text 节点，拥有自己的 TextController。
   * 同一 revision 的多个窗口共享同一个 source 的 StyledString，但各自有独立的窗口状态。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   * @returns 匹配的现有窗口状态或新创建的窗口状态
   */
  getOrCreateWindowState(renderNodeKey: string): MotionWindowState {
    const existing = this.windowStates.get(renderNodeKey)
    if (existing !== undefined) {
      return existing
    }
    const state: MotionWindowState = {
      renderNodeKey,
      textController: new TextController(),
      mounted: false,
      layoutReady: false,
      // Issue #879 评论6087544240 问题1：owners 集合由 reconcileWindowOwners 幂等管理，
      // 不再在 build 阶段逐次 ++ 引用计数。
      owners: new Set<string>()
    }
    this.windowStates.set(renderNodeKey, state)
    return state
  }

  /**
   * 标记本窗口已挂载（Text.onAppear 已触发）。
   *
   * Issue #879 最新复核评论修复（问题3）：
   * 窗口级 mounted 状态独立管理——不再从源池 source 读取，
   * 避免某一窗口先 mounted 后，后续同版本新窗口从 source 读到 mounted=true，
   * 导致新 Text 的 onAppear 跳过 setStyledString 而空白。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   */
  markWindowMounted(renderNodeKey: string): void {
    const state = this.windowStates.get(renderNodeKey)
    if (state !== undefined) {
      state.mounted = true
    }
  }

  /**
   * 标记本窗口布局已完成。
   *
   * Issue #879 最新复核评论修复（问题3）：
   * 由真实 ComponentObserver('layout') 回调调用，不再用 setTimeout 充当绘制完成证据。
   * ComponentObserver 'layout' 回调确保 ArkUI Text 的 measure/layout 真正完成后才标记 ready。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   */
  markWindowLayoutReady(renderNodeKey: string): void {
    const state = this.windowStates.get(renderNodeKey)
    if (state !== undefined) {
      state.layoutReady = true
    }
  }

  /**
   * 查询本窗口是否布局已完成。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   * @returns layoutReady 状态；窗口状态不存在时返回 false
   */
  isWindowLayoutReady(renderNodeKey: string): boolean {
    const state = this.windowStates.get(renderNodeKey)
    if (state !== undefined) {
      return state.layoutReady
    }
    return false
  }

  /**
   * 查询本窗口是否已挂载。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   * @returns mounted 状态；窗口状态不存在时返回 false
   */
  isWindowMounted(renderNodeKey: string): boolean {
    const state = this.windowStates.get(renderNodeKey)
    if (state !== undefined) {
      return state.mounted
    }
    return false
  }

  /**
   * Issue #879 评论6087544240 问题1：幂等对齐窗口逻辑 owner 集合。
   *
   * 旧实现用 acquireWindowState/releaseWindowState 在 build 阶段逐次 ++/-- 引用计数，
   * 但 build() 在 ArkUI 构建/重绘阶段被反复调用（动画逐帧、光标移动、候选切换），
   * 同一物理 Text 从头到尾只创建/销毁一次，计数可能从 1 累加到上千，结束时仅减 1，
   * 永远不会删掉 MotionWindowState。
   *
   * 改法：build() 只做声明式状态投影，不执行修改引用计数的副作用。
   * 由 prepareMotionSources 在每次候选准备时一次性调用 reconcileWindowOwners，
   * 传入当前 displayed 帧和 candidate 帧的所有窗口 renderNodeKey：
   * - 对每个 key 调 getOrCreateWindowState 确保存在；
   * - 对 displayedKeys 中的 key，添加 'displayed' 到 owners；
   * - 对 candidateKeys 中的 key，添加 'candidate' 到 owners；
   * - 对不在 displayedKeys 中的已有 key，从 owners 中删除 'displayed'；
   * - 对不在 candidateKeys 中的已有 key，从 owners 中删除 'candidate'；
   * - owner 集合为空且 mounted === false（物理节点已卸载）时才删除 windowState。
   *
   * **关键**：同样的集合被应用任意次，结果不变（幂等）。
   *
   * Issue #879 评论6087544240 问题2：共享 key 的 candidate 被去重后没有自己的 onDisappear，
   * 双 owner 计数不可能对称归还。改用集合差集更新 owner 后，
   * 取消候选（candidateVisualFrame = null）时不清理仍被 displayed 使用的 TextController/observer——
   * candidate owner 退出，displayed owner 仍在则保留。
   *
   * @param displayedKeys displayed 帧所有窗口的 renderNodeKey
   * @param candidateKeys candidate 帧所有窗口的 renderNodeKey
   */
  reconcileWindowOwners(displayedKeys: string[], candidateKeys: string[]): void {
    const displayedSet: Set<string> = new Set(displayedKeys)
    const candidateSet: Set<string> = new Set(candidateKeys)

    // 确保所有 key 的 windowState 存在
    for (let i = 0; i < displayedKeys.length; i++) {
      this.getOrCreateWindowState(displayedKeys[i])
    }
    for (let i = 0; i < candidateKeys.length; i++) {
      this.getOrCreateWindowState(candidateKeys[i])
    }

    // 对每个已有 windowState，按集合差集更新 owner
    const toDelete: string[] = []
    this.windowStates.forEach((state: MotionWindowState, key: string) => {
      if (displayedSet.has(key)) {
        state.owners.add('displayed')
      } else {
        state.owners.delete('displayed')
      }
      if (candidateSet.has(key)) {
        state.owners.add('candidate')
      } else {
        state.owners.delete('candidate')
      }
      // owner 集合为空且物理节点已卸载才删除 windowState。
      // mounted === true 时即使 owner 集合为空也保留——等 onDisAppear 标记 mounted=false 后下一轮再清。
      if (state.owners.size === 0 && !state.mounted) {
        toDelete.push(key)
      }
    })
    for (let i = 0; i < toDelete.length; i++) {
      this.windowStates.delete(toDelete[i])
    }
  }

  /**
   * Issue #879 评论6087544240 问题1+2：标记窗口物理卸载。
   *
   * Text.onDisAppear 中调用——只标记 mounted = false，不删除 windowState。
   * windowState 的删除由 reconcileWindowOwners 负责（owner 集合为空且 mounted === false）。
   *
   * 旧实现 releaseWindowState 递减 owner 引用计数，但共享 key 的 candidate 被去重后
   * 没有自己的 onDisappear，双 owner 计数不可能对称归还。改为只标记物理卸载，
   * owner 集合由 reconcileWindowOwners 统一管理。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   */
  markWindowUnmounted(renderNodeKey: string): void {
    const state = this.windowStates.get(renderNodeKey)
    if (state !== undefined) {
      state.mounted = false
    }
  }

  /**
   * 获取特定窗口状态。
   *
   * @param renderNodeKey 窗口渲染节点身份标识
   * @returns 匹配的窗口状态，或 undefined（不存在）
   */
  getWindowState(renderNodeKey: string): MotionWindowState | undefined {
    return this.windowStates.get(renderNodeKey)
  }

  // ── 清理 ──

  /**
   * 清空所有源和窗口状态。
   *
   * 编辑器销毁或重置时调用——释放所有源（包括未标记释放的）和所有窗口状态。
   */
  clear(): void {
    this.sources.clear()
    this.windowStates.clear()
  }
}
