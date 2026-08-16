// EditorViewportState — 编辑器视口状态（强类型值对象，不用裸 object）。
//
// Issue #629 评论 5 第 8 节：viewport 不能只是 `currentViewport: object` 存进去又读出来，
// 要有明确字段。本类定义滚动偏移 + 视口尺寸 + 内容尺寸，供 SujianEditor 滚动时回调给
// WritingScreen、接续恢复时由 editor.restoreViewport(state) 消费。
//
// 所有坐标单位均为 px，相对编辑器内容坐标系（与渲染层 Stack align(TopStart) 一致）。
// scrollTop/scrollLeft 是内容滚动偏移（内容左上角相对视口左上角的偏移，向下/向右为正）。
// viewportWidth/viewportHeight 是视口可见区域尺寸。
// contentWidth/contentHeight 是内容总尺寸（正文+选区+composition+caret 在同一滚动坐标系里）。

export class EditorViewportState {
  readonly scrollTop: number       // 垂直滚动偏移（px，内容坐标系，向下为正）
  readonly scrollLeft: number      // 水平滚动偏移（px，内容坐标系，向右为正）
  readonly viewportWidth: number   // 视口可见宽度（px）
  readonly viewportHeight: number  // 视口可见高度（px）
  readonly contentWidth: number    // 内容总宽度（px）
  readonly contentHeight: number   // 内容总高度（px）

  constructor(
    scrollTop: number = 0,
    scrollLeft: number = 0,
    viewportWidth: number = 0,
    viewportHeight: number = 0,
    contentWidth: number = 0,
    contentHeight: number = 0
  ) {
    this.scrollTop = scrollTop
    this.scrollLeft = scrollLeft
    this.viewportWidth = viewportWidth
    this.viewportHeight = viewportHeight
    this.contentWidth = contentWidth
    this.contentHeight = contentHeight
  }

  // clamp：确保 scrollTop/scrollLeft 在 [0, maxScroll] 范围内。
  // 防止接续恢复时传入越界 offset（目标设备内容尺寸不同）。
  clamped(): EditorViewportState {
    const maxScrollTop: number = Math.max(0, this.contentHeight - this.viewportHeight)
    const maxScrollLeft: number = Math.max(0, this.contentWidth - this.viewportWidth)
    return new EditorViewportState(
      Math.max(0, Math.min(this.scrollTop, maxScrollTop)),
      Math.max(0, Math.min(this.scrollLeft, maxScrollLeft)),
      this.viewportWidth,
      this.viewportHeight,
      this.contentWidth,
      this.contentHeight
    )
  }

  // 是否可滚动（内容大于视口）。
  canScrollVertical(): boolean {
    return this.contentHeight > this.viewportHeight
  }

  canScrollHorizontal(): boolean {
    return this.contentWidth > this.viewportWidth
  }

  // 序列化/反序列化（接续 payload 用 JSON 透传）。
  // fromObject 容错：缺失字段用 0，非有限数用 0。
  static fromObject(obj: Record<string, Object>): EditorViewportState {
    const num = (k: string): number => {
      const v: Object | undefined = obj[k]
      if (v === undefined || v === null) {
        return 0
      }
      const n: number = v as number
      return typeof n === 'number' && isFinite(n) ? n : 0
    }
    return new EditorViewportState(
      num('scrollTop'),
      num('scrollLeft'),
      num('viewportWidth'),
      num('viewportHeight'),
      num('contentWidth'),
      num('contentHeight')
    )
  }

  toObject(): Record<string, Object> {
    return {
      scrollTop: this.scrollTop as Object,
      scrollLeft: this.scrollLeft as Object,
      viewportWidth: this.viewportWidth as Object,
      viewportHeight: this.viewportHeight as Object,
      contentWidth: this.contentWidth as Object,
      contentHeight: this.contentHeight as Object
    }
  }

  equals(other: EditorViewportState): boolean {
    return this.scrollTop === other.scrollTop
      && this.scrollLeft === other.scrollLeft
      && this.viewportWidth === other.viewportWidth
      && this.viewportHeight === other.viewportHeight
      && this.contentWidth === other.contentWidth
      && this.contentHeight === other.contentHeight
  }
}
