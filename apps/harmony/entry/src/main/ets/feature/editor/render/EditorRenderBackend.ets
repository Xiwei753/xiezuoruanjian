// EditorRenderBackend — 编辑器静态渲染后端。
// 输入只能是 EditorLayoutSnapshot。先做静态文本、选区、composition 下划线、光标和命中测试。
// 动画层以后新增 motion 消费同一份 snapshot，不改输入层和 session 层。

import { EditorLayoutSnapshot } from './EditorLayoutSnapshot'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { layoutLines, hitTestPoint } from './editor_layout_math'
import {
  toLineLayouts,
  computeSelectionRects,
  computeCaretRect,
  computeCompositionUnderlineRects,
  LineLayout,
  SelectionRect,
  CaretRect,
  CompositionUnderlineRect,
} from './editor_render_geometry'

export interface EditorRenderResult {
  text: string
  selectionStartUtf16: number
  selectionEndUtf16: number
  compositionStartUtf16: number | null
  compositionEndUtf16: number | null
  cursorUtf16: number
  revision: number
  // 静态渲染几何。render() 不填（默认空/null），renderLayout() 填。
  // lines: 每行文本布局（含 y/height）；selectionRects: 选区背景矩形列表；
  // caretRect: 静态光标矩形（不闪烁）；compositionUnderlineRects: composition 下划线矩形列表。
  lines: LineLayout[]
  selectionRects: SelectionRect[]
  caretRect: CaretRect | null
  compositionUnderlineRects: CompositionUnderlineRect[]
}

export class EditorRenderBackend {
  // 把 byte offset snapshot 转成 UTF-16 offset 渲染结果（ArkUI Text 用 UTF-16）。
  // EditorLayoutSnapshot 中 selectionAnchor/selectionHead/compositionStart/compositionEnd/cursorByteOffset
  // 均为 UTF-8 byte offset，必须经 TextOffsetMapper.utf8ToUtf16 转成 UTF-16 code unit offset 后再交给渲染层。
  // 本方法不含几何信息（lines/selectionRects/caretRect/compositionUnderlineRects 为空/null），
  // 用于不需要几何的场景；需要几何时用 renderLayout。
  static render(snapshot: EditorLayoutSnapshot): EditorRenderResult {
    const text = snapshot.text
    return {
      text,
      selectionStartUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor),
      selectionEndUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead),
      compositionStartUtf16: snapshot.compositionStart === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionStart),
      compositionEndUtf16: snapshot.compositionEnd === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionEnd),
      cursorUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.cursorByteOffset),
      revision: snapshot.revision,
      lines: [],
      selectionRects: [],
      caretRect: null,
      compositionUnderlineRects: [],
    }
  }

  // 完整静态渲染：在 render() 基础上计算行布局/选区/光标/composition 下划线几何。
  //
  // - containerWidth: 实际容器宽度（px，来自 onAreaChange）。
  // - textHeight: 实际文本渲染总高度（px，来自 onAreaChange）；行高 = textHeight / lines.length。
  // - measureTextFn: 真实文本测量函数（px 单行宽度），生产由 SujianEditor 注入
  //   getUIContext().getMeasureUtils().measureText；测试注入确定性 mock。
  //
  // 几何计算委托 editor_render_geometry 的纯函数，保证可单测。
  // 折行用 measureTextFn 逐 code point 二分测宽度（与 Text 的 WordBreak.BREAK_ALL 一致）。
  // 选区/光标/composition 下划线是静态显示能力，不是动画：不做闪烁/平滑移动/淡入淡出。
  static renderLayout(
    snapshot: EditorLayoutSnapshot,
    containerWidth: number,
    textHeight: number,
    measureTextFn: (s: string) => number,
  ): EditorRenderResult {
    const base = EditorRenderBackend.render(snapshot)
    const text = base.text
    const lines = layoutLines(text, containerWidth, measureTextFn)
    const lineSpacingPx = lines.length > 0 ? textHeight / lines.length : 0
    const lineLayouts = toLineLayouts(lines, lineSpacingPx)
    const selStart = Math.min(base.selectionStartUtf16, base.selectionEndUtf16)
    const selEnd = Math.max(base.selectionStartUtf16, base.selectionEndUtf16)
    const selectionRects = computeSelectionRects(text, lines, lineSpacingPx, selStart, selEnd, measureTextFn)
    const caretRect = computeCaretRect(text, lines, lineSpacingPx, base.cursorUtf16, measureTextFn)
    const compositionUnderlineRects = computeCompositionUnderlineRects(
      text,
      lines,
      lineSpacingPx,
      base.compositionStartUtf16,
      base.compositionEndUtf16,
      measureTextFn,
    )
    return {
      text: base.text,
      selectionStartUtf16: base.selectionStartUtf16,
      selectionEndUtf16: base.selectionEndUtf16,
      compositionStartUtf16: base.compositionStartUtf16,
      compositionEndUtf16: base.compositionEndUtf16,
      cursorUtf16: base.cursorUtf16,
      revision: base.revision,
      lines: lineLayouts,
      selectionRects,
      caretRect,
      compositionUnderlineRects,
    }
  }

  // 命中测试：给定 UTF-16 offset，返回是否在选区内。
  // snapshot 的 selectionAnchor/selectionHead 是 byte offset，先转成 UTF-16 offset 再与 utf16Offset 比较，
  // 避免混用两种 offset 语义导致中文等非 ASCII 文本命中判断错位。
  static hitTest(snapshot: EditorLayoutSnapshot, utf16Offset: number): 'before' | 'inside' | 'after' {
    const text = snapshot.text
    const anchorUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor)
    const headUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead)
    const start = Math.min(anchorUtf16, headUtf16)
    const end = Math.max(anchorUtf16, headUtf16)
    if (utf16Offset < start) return 'before'
    if (utf16Offset > end) return 'after'
    return 'inside'
  }

  // 坐标命中测试：给定触摸坐标 (touchX, touchY)（px，相对组件左上）和实际容器尺寸，
  // 返回命中的 UTF-16 code unit offset。命中逻辑归本方法（Issue #629 第 5 节）。
  //
  // - containerWidth: 实际容器宽度（px，来自 onAreaChange）。
  // - textHeight: 实际文本渲染总高度（px，来自 onAreaChange）；行高 = textHeight / lines.length。
  // - measureTextFn: 真实文本测量函数（px 单行宽度），生产由 SujianEditor 注入
  //   getUIContext().getMeasureUtils().measureText；测试注入确定性 mock。
  //
  // 折行用 measureTextFn 逐 code point 二分测宽度（与 Text 的 WordBreak.BREAK_ALL 一致），
  // 不切断 surrogate pair；行号用 textHeight/lines.length 算（不依赖字体 density）。
  // 返回的 UTF-16 offset 由 SelectionController.onTap 经 TextOffsetMapper.utf16ToUtf8 转 byte offset。
  static hitTestByPoint(
    snapshot: EditorLayoutSnapshot,
    containerWidth: number,
    textHeight: number,
    touchX: number,
    touchY: number,
    measureTextFn: (s: string) => number,
  ): number {
    const text = snapshot.text
    const lines = layoutLines(text, containerWidth, measureTextFn)
    const lineSpacingPx = lines.length > 0 ? textHeight / lines.length : 0
    return hitTestPoint(text, lines, lineSpacingPx, touchX, touchY, measureTextFn)
  }
}
