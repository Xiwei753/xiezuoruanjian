// EditorMotionSourcePool — 稳定运动字形源池。
//
// Issue #879 复核评论修复：用 sourceRevision + stableRunId 保存
// StyledString/TextController/mounted/layoutReady/sourceGeometry。
// ForEach key 基于稳定 run 身份（sourceKind + revision），而非一次性自增事务编号。
// 旧字源到新画面的交棒结束才释放。
//
// 核心问题：旧实现每次动画都用 ForEach 创建新的 Text 组件，key 用自增的 runIdPrefix——
// 每笔输入都销毁、重建整份运动文字组件。这导致文字源要等
// Text.onAppear → setStyledString → layout 才能真正绘制，先走了几十帧再出现字，效果跳帧。
//
// 解决方案：让源 Text 先常驻并排版好，运动帧仅改变 clip 和位置。
// - 同一 runId 的 Text 不会因新事务而重建——只更新 clip 和位置。
// - 旧字源到新画面的交棒结束才释放（markedForRelease → releaseMarkedSources）。

/**
 * 运动字形源池条目。
 *
 * 每个条目对应一个稳定的排版源 Text——旧正文源或新正文源。
 * runId 基于 sourceKind + revision，不随事务自增，确保同一源的 Text 常驻不重建。
 */
export interface MotionSourceEntry {
  /** 稳定的 run 身份标识——基于 sourceKind + revision，不随事务自增 */
  runId: string
  /** 该源对应的正文 revision */
  sourceRevision: number
  /** 该源的完整排版内容 */
  styledString: StyledString
  /** 独立的 TextController——每个源有自己的 controller，不共享 */
  textController: TextController
  /** Text 是否已挂载（onAppear 已触发） */
  mounted: boolean
  /** 布局是否已完成（layout observer 已回调） */
  layoutReady: boolean
  /** 旧正文源还是新正文源 */
  sourceKind: 'old' | 'new'
  /** 是否标记为可释放（交棒完成后才真正释放） */
  markedForRelease: boolean
}

/**
 * 稳定运动字形源池。
 *
 * 管理 MotionSourceEntry 的创建、获取、标记释放和实际释放。
 *
 * 使用方式：
 * 1. 运动开始时，调用 getOrCreateSource 获取或创建 old/new 源。
 *    同一 sourceKind + revision 的源不会重复创建——返回现有条目。
 * 2. ForEach key 使用 entry.runId（稳定身份），而非 `${sourceKind}-${runIdPrefix}-${index}`。
 *    同一 runId 的 Text 不会因新事务而重建——只更新 clip 和位置。
 * 3. 交棒完成后（动画结束或新源已接管），调用 markForRelease 标记旧源可释放。
 * 4. 调用 releaseMarkedSources 真正释放已标记的源。
 */
export class EditorMotionSourcePool {
  private sources: Map<string, MotionSourceEntry> = new Map()

  /**
   * 获取或创建源。
   *
   * 如果已有匹配的源（sourceKind + revision），返回现有条目——不创建新的。
   * 这确保同一源的 Text 常驻不重建，避免 onAppear → setStyledString → layout 的跳帧延迟。
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
    // 创建新条目
    const entry: MotionSourceEntry = {
      runId,
      sourceRevision: revision,
      styledString,
      textController: new TextController(),
      mounted: false,
      layoutReady: false,
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
   * 实际释放在 releaseMarkedSources 中执行——确保旧字源到新画面的交棒结束才释放。
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

  /**
   * 清空所有源。
   *
   * 编辑器销毁或重置时调用——释放所有源（包括未标记释放的）。
   */
  clear(): void {
    this.sources.clear()
  }
}
