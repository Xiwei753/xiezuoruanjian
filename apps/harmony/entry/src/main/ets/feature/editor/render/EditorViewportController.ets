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
  // 待恢复的状态：SujianEditor 注册前 WritingScreen 可能已调 restore，缓存等注册后执行。
  private pendingRestore: EditorViewportState | null = null

  // SujianEditor 注册 scrollTo 和 getDimensions 实现。
  register(scrollTo: ScrollToImpl, getDimensions: GetDimensionsImpl): void {
    this.scrollToImpl = scrollTo
    this.getDimensionsImpl = getDimensions
    // 注册后如果有 pendingRestore，立即执行。
    if (this.pendingRestore) {
      this.doRestore(this.pendingRestore)
      this.pendingRestore = null
    }
  }

  // SujianEditor 注销（aboutToDisappear 时）。
  unregister(): void {
    this.scrollToImpl = null
    this.getDimensionsImpl = null
  }

  // WritingScreen 调用：恢复到指定 viewport 状态。
  // 如果 SujianEditor 已注册，立即 clamp + scrollTo；
  // 如果未注册（layout 还未就绪），缓存等注册后执行。
  restore(state: EditorViewportState): void {
    if (this.scrollToImpl && this.getDimensionsImpl) {
      this.doRestore(state)
    } else {
      this.pendingRestore = state
    }
  }

  // 真正执行恢复：按目标设备当前尺寸 clamp 后 scrollTo。
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
