// EditorMotionSourcePool — 稳定运动字形源池。
//
// Issue #879 最新复核评论修复（问题3）：
// - 源池 sourceRevision 只保存可共享的 StyledString 内容（同一 revision 的内容确实一致）。
// - TextController + mounted + layoutReady 状态放在与真实节点 1:1 的 clusterId 窗口条目中，
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
 * TextController/mounted/layoutReady 移到 MotionWindowState（按 clusterId 管理）。
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
 * 每个 clusterId 对应一个独立的 ArkUI Text 节点，拥有自己的 TextController、
 * mounted 和 layoutReady 状态。不再按 old/new-revision 归并——
 * 避免某一窗口先 mounted 后，后续同版本新窗口从 source 读到 mounted=true，
 * 导致新 Text 的 onAppear 跳过 setStyledString 而空白。
 *
 * - clusterId：以 sourceKind + sourceRevision + utf16Start + utf16End 构造的稳定身份标识
 * - textController：每个窗口独立的 TextController（ArkUI 不允许同一 Controller 绑定多个 Text）
 * - mounted：本窗口的 Text 是否已挂载（onAppear 已触发）
 * - layoutReady：本窗口的布局是否已完成（ComponentObserver 'layout' 回调已触发）
 */
export interface MotionWindowState {
  /** 窗口簇身份标识——与 MotionGlyphWindow.clusterId 对应 */
  clusterId: string
  /** 独立的 TextController——每个窗口有自己的 controller，不共享 */
  textController: TextController
  /** 本窗口的 Text 是否已挂载（onAppear 已触发） */
  mounted: boolean
  /** 本窗口的布局是否已完成（ComponentObserver 'layout' 回调已触发） */
  layoutReady: boolean
}

/**
 * 稳定运动字形源池。
 *
 * 管理 MotionSourceEntry（可共享内容）和 MotionWindowState（窗口级状态）的
 * 创建、获取、标记释放和实际释放。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * - sources：按 runId（sourceKind + revision）管理可共享的 StyledString 内容
 * - windowStates：按 clusterId 管理窗口级状态（TextController/mounted/layoutReady）
 * - 同一 revision 的多个窗口共享同一个 source 的 StyledString，但各自有独立的窗口状态
 *
 * 使用方式：
 * 1. 运动开始时，调用 getOrCreateSource 获取或创建 old/new 源（可共享内容）。
 * 2. 为每个运动窗口调用 getOrCreateWindowState 获取或创建窗口级状态。
 * 3. ForEach key 使用 clusterId（稳定身份），确保同一窗口的 Text 常驻不重建。
 * 4. Text.onAppear 中调用 markWindowMounted 标记本窗口已挂载。
 * 5. ComponentObserver 'layout' 回调中调用 markWindowLayoutReady 标记本窗口布局完成。
 * 6. 交棒完成后调用 markForRelease 标记旧源可释放，releaseMarkedSources 真正释放。
 * 7. Text.onDisAppear 中调用 releaseWindowState 清理本窗口状态。
 */
export class EditorMotionSourcePool {
  /** 可共享的源内容——按 runId（sourceKind + revision）管理 */
  private sources: Map<string, MotionSourceEntry> = new Map()
  /** 窗口级状态——按 clusterId 管理，与真实节点 1:1 对应 */
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
   * 释放不再被任何窗口引用且不属于当前 displayed/pending 版本的源。
   *
   * @param activeRunIds 当前 visualFrame 的 motionGlyphWindows 中所有 runId
   * @param displayedRevision 当前正在显示的正文 revision
   * @param pendingRevision 等待排版的正文 revision（可能还未消费）
   */
  releaseUnusedSources(
    activeRunIds: Set<string>,
    displayedRevision: number,
    pendingRevision: number
  ): void {
    this.sources.forEach((entry: MotionSourceEntry) => {
      if (!activeRunIds.has(entry.runId) &&
          entry.sourceRevision !== displayedRevision &&
          entry.sourceRevision !== pendingRevision) {
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
   * 每个 clusterId 对应一个独立的 ArkUI Text 节点，拥有自己的 TextController。
   * 同一 revision 的多个窗口共享同一个 source 的 StyledString，但各自有独立的窗口状态。
   *
   * @param clusterId 窗口簇身份标识
   * @returns 匹配的现有窗口状态或新创建的窗口状态
   */
  getOrCreateWindowState(clusterId: string): MotionWindowState {
    const existing = this.windowStates.get(clusterId)
    if (existing !== undefined) {
      return existing
    }
    const state: MotionWindowState = {
      clusterId,
      textController: new TextController(),
      mounted: false,
      layoutReady: false
    }
    this.windowStates.set(clusterId, state)
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
   * @param clusterId 窗口簇身份标识
   */
  markWindowMounted(clusterId: string): void {
    const state = this.windowStates.get(clusterId)
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
   * @param clusterId 窗口簇身份标识
   */
  markWindowLayoutReady(clusterId: string): void {
    const state = this.windowStates.get(clusterId)
    if (state !== undefined) {
      state.layoutReady = true
    }
  }

  /**
   * 查询本窗口是否布局已完成。
   *
   * @param clusterId 窗口簇身份标识
   * @returns layoutReady 状态；窗口状态不存在时返回 false
   */
  isWindowLayoutReady(clusterId: string): boolean {
    const state = this.windowStates.get(clusterId)
    if (state !== undefined) {
      return state.layoutReady
    }
    return false
  }

  /**
   * 查询本窗口是否已挂载。
   *
   * @param clusterId 窗口簇身份标识
   * @returns mounted 状态；窗口状态不存在时返回 false
   */
  isWindowMounted(clusterId: string): boolean {
    const state = this.windowStates.get(clusterId)
    if (state !== undefined) {
      return state.mounted
    }
    return false
  }

  /**
   * 释放本窗口状态。
   *
   * Issue #879 最新复核评论修复（问题3）：
   * Text.onDisAppear 中调用——清理本窗口的 TextController/mounted/layoutReady 状态。
   * 对应的 Text 组件已从组件树中移除，窗口状态不再需要。
   *
   * @param clusterId 窗口簇身份标识
   */
  releaseWindowState(clusterId: string): void {
    this.windowStates.delete(clusterId)
  }

  /**
   * 获取特定窗口状态。
   *
   * @param clusterId 窗口簇身份标识
   * @returns 匹配的窗口状态，或 undefined（不存在）
   */
  getWindowState(clusterId: string): MotionWindowState | undefined {
    return this.windowStates.get(clusterId)
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
