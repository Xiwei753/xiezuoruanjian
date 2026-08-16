// EditorViewportController — 父子 viewport 恢复通道。
// WritingScreen 持有 controller 实例，SujianEditor 注册 scrollTo 实现。
// 接续加载完成、目标设备布局尺寸已拿到后，WritingScreen 调 controller.restore(state)，
// controller 调用已注册的 scrollTo 把 SujianEditor 滚动到接续位置。
// clamp 基于目标设备当前 viewport/content 尺寸，不直接拿源设备保存的尺寸裁剪。
//
// Issue #629 评论6 Part C：viewport 恢复闭环。
// - 旧实现：WritingScreen.restoreContinuationEditorState() 只 this.currentViewport = params.viewport，
//   没调 SujianEditor.restoreViewport()，子组件不会真的滚动到接续位置。
// - 新实现：WritingScreen 持有 controller，SujianEditor 在 aboutToAppear 注册 scrollTo + getDimensions，
//   WritingScreen 调 controller.restore(state)，controller 等注册就绪后按目标设备当前尺寸 clamp + scrollTo。
// - pendingRestore：SujianEditor 注册前 WritingScreen 可能已调 restore（接续加载完成早于 layout 就绪），
//   缓存等注册后立即执行。
//
// Issue #629 评论7 第4项：区分 registered 与 dimensionsReady。
// - 旧实现：register 后立即消费 pendingRestore，但此时尺寸可能为 0（layout 未就绪），
//   doRestore 把 offset clamp 成 0，pending 被清空，接续位置丢失。
// - 新实现：register 只注册实现，不立即消费 pending（尺寸可能为 0）。
//   pending 只有在 dimensionsReady（viewportWidth>0 && viewportHeight>0 && contentWidth>0 && contentHeight>0）
//   后才真正 scrollTo 并清空。尺寸未就绪时保留 pending（不 clamp 成 0）。
//   SujianEditor 的 Scroll/Text onAreaChange 更新尺寸后调 notifyDimensionsChanged() 重试消费 pending。
// - clamp 仍用目标设备当前尺寸（getDimensionsImpl()），不用源设备保存的宽高。

import { EditorViewportState } from './EditorViewportState'

// scrollTo 实现：由 SujianEditor 注册，把视口滚动到指定 offset。
export type ScrollToImpl = (xOffset: number, yOffset: number) => void

// viewport 尺寸查询：由 SujianEditor 注册，返回当前真实 viewport 和 content 尺寸。
export interface ViewportDimensions {
  viewportWidth: number
  viewportHeight: number
  contentWidth: number
  contentHeight: number
}
export type GetDimensionsImpl = () => ViewportDimensions

export class EditorViewportController {
  private scrollToImpl: ScrollToImpl | null = null
  private getDimensionsImpl: GetDimensionsImpl | null = null
  // 待恢复的状态：SujianEditor 注册前或尺寸未就绪时 WritingScreen 可能已调 restore，缓存等就绪后执行。
  private pendingRestore: EditorViewportState | null = null

  // SujianEditor 注册 scrollTo 和 getDimensions 实现。
  // 注册后不立即消费 pending：尺寸可能为 0（layout 未就绪），clamp 会把 offset 压成 0 丢失位置。
  // 改为调一次 tryConsumePending，尺寸就绪才消费；未就绪保留 pending 等 notifyDimensionsChanged。
  register(scrollTo: ScrollToImpl, getDimensions: GetDimensionsImpl): void {
    this.scrollToImpl = scrollTo
    this.getDimensionsImpl = getDimensions
    this.tryConsumePending()
  }

  // SujianEditor 注销（aboutToDisappear 时）。
  unregister(): void {
    this.scrollToImpl = null
    this.getDimensionsImpl = null
  }

  // WritingScreen 调用：恢复到指定 viewport 状态。
  // 缓存 pending 后尝试消费：已注册且尺寸就绪时立即 clamp + scrollTo；
  // 未注册或尺寸未就绪时保留 pending 等 register/notifyDimensionsChanged。
  restore(state: EditorViewportState): void {
    this.pendingRestore = state
    this.tryConsumePending()
  }

  // Issue #629 评论7 第4项：SujianEditor 的 Scroll/Text onAreaChange 更新尺寸后调用，
  // 重试消费 pending。尺寸就绪时 doRestore 并清空 pending；未就绪保留 pending。
  notifyDimensionsChanged(): void {
    this.tryConsumePending()
  }

  // 尺寸就绪且 pending 非空时消费 pending：按目标设备当前尺寸 clamp 后 scrollTo 并清空 pending。
  // 尺寸未就绪或无 pending 时保留现状（不 clamp 成 0，不丢位置）。
  private tryConsumePending(): void {
    if (!this.scrollToImpl || !this.getDimensionsImpl) {
      return
    }
    if (this.pendingRestore === null) {
      return
    }
    if (!this.dimensionsReady()) {
      return
    }
    this.doRestore(this.pendingRestore)
    this.pendingRestore = null
  }

  // 尺寸就绪：viewport 宽高 > 0 且 content 宽高 > 0。
  // 全 0 表示 layout 未就绪（onAreaChange 还没回调），此时不应消费 pending。
  private dimensionsReady(): boolean {
    if (!this.getDimensionsImpl) {
      return false
    }
    const dims: ViewportDimensions = this.getDimensionsImpl()
    return dims.viewportWidth > 0 && dims.viewportHeight > 0
      && dims.contentWidth > 0 && dims.contentHeight > 0
  }

  // 真正执行恢复：按目标设备当前尺寸 clamp 后 scrollTo。
  // 调用方需保证 scrollToImpl/getDimensionsImpl 非空且 dimensionsReady。
  private doRestore(state: EditorViewportState): void {
    if (!this.scrollToImpl || !this.getDimensionsImpl) {
      return
    }
    const dims = this.getDimensionsImpl()
    // 用目标设备当前 viewport/content 尺寸 clamp，不直接拿源设备保存的尺寸裁剪。
    const maxScrollTop = Math.max(0, dims.contentHeight - dims.viewportHeight)
    const maxScrollLeft = Math.max(0, dims.contentWidth - dims.viewportWidth)
    const clampedX = Math.max(0, Math.min(state.scrollLeft, maxScrollLeft))
    const clampedY = Math.max(0, Math.min(state.scrollTop, maxScrollTop))
    this.scrollToImpl(clampedX, clampedY)
  }

  // 是否已注册（供 WritingScreen 判断 layout 是否就绪）。
  isRegistered(): boolean {
    return this.scrollToImpl !== null && this.getDimensionsImpl !== null
  }
}
