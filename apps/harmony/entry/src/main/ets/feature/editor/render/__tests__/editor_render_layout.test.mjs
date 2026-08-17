// editor_render_layout.test.mjs — editor_render_geometry.ts 的纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_render_geometry.ts（纯 TS，无 ArkUI 依赖）。
// 注入确定性 mock measureFn（每 UTF-16 code unit 10px），验证选区/光标/composition 下划线
// 矩形计算的数学性质：单行/多行/空选区/越界/自动交换/行尾归行。
//
// 运行：node --experimental-strip-types editor_render_layout.test.mjs

import { strict as assert } from 'node:assert'
import { layoutLines, LineBreakKind, CaretAffinity } from '../editor_layout_math.ts'
import {
  toLineLayouts,
  computeSelectionRects,
  computeCaretRect,
  computeCompositionUnderlineRects,
  CARET_WIDTH_PX,
  UNDERLINE_HEIGHT_PX,
} from '../editor_render_geometry.ts'

// 确定性 mock measureFn：每 UTF-16 code unit 10px。满足 measure('')===0 与单调不减。
const mockMeasure = (s) => s.length * 10

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_render_geometry 纯逻辑单测')
console.log('---')

// ── 常量 ──
test('常量: CARET_WIDTH_PX === 2', () => {
  assert.equal(CARET_WIDTH_PX, 2)
})
test('常量: UNDERLINE_HEIGHT_PX === 2', () => {
  assert.equal(UNDERLINE_HEIGHT_PX, 2)
})

// ── toLineLayouts ──
// Issue #629 评论18：toLineLayouts 现在返回 LineLayout[]（含 breakKind + caretStops）。
test('toLineLayouts: 空数组返回 []', () => {
  assert.deepEqual(toLineLayouts([], 20, '', mockMeasure), [])
})
test('toLineLayouts: 单行 y=0 height=lineSpacing 含 breakKind + caretStops', () => {
  const ls = toLineLayouts([{ start: 0, end: 3, breakKind: LineBreakKind.EndOfText }], 20, 'abc', mockMeasure)
  assert.equal(ls.length, 1)
  assert.equal(ls[0].startUtf16, 0)
  assert.equal(ls[0].endUtf16, 3)
  assert.equal(ls[0].y, 0)
  assert.equal(ls[0].height, 20)
  assert.equal(ls[0].breakKind, LineBreakKind.EndOfText)
  assert.ok(Array.isArray(ls[0].caretStops))
  assert.ok(ls[0].caretStops.length >= 2) // 至少行首 + 行尾
})
test('toLineLayouts: 多行 y = i * lineSpacing', () => {
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const ls = toLineLayouts(lines, 20, 'abcdef', mockMeasure)
  assert.equal(ls.length, 3)
  assert.equal(ls[0].y, 0)
  assert.equal(ls[1].y, 20)
  assert.equal(ls[2].y, 40)
})
test('toLineLayouts: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const ls = toLineLayouts(lines, 0, 'abcdef', mockMeasure)
  assert.equal(ls.length, 1)
  assert.equal(ls[0].y, 0)
  assert.equal(ls[0].height, 0)
})

// ── computeSelectionRects ──
test('computeSelectionRects: 空文本返回 []', () => {
  assert.deepEqual(computeSelectionRects('', [], 20, 0, 0, mockMeasure), [])
})
test('computeSelectionRects: 空选区返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeSelectionRects('abcdef', lines, 20, 2, 2, mockMeasure), [])
})
test("computeSelectionRects: selStart>selEnd 自动交换成 [1,3)", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const rects = computeSelectionRects("abcdef", lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: 单行选区', () => {
  // text='abcdef', 容器 1000px 不折行，selStart=1, selEnd=3
  // x = measure('a') = 10, width = measure('bc') = 20, y=0, height=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: 多行选区每行一个 rect', () => {
  // text='abcdef', 容器 25px 每行 2 字符 → 3 行：[0,2)[2,4)[4,6)
  // selStart=1, selEnd=5：
  //   行0: [1,2) x=measure('a')=10, width=measure('b')=10, y=0
  //   行1: [2,4) x=measure('')=0,  width=measure('cd')=20, y=20
  //   行2: [4,5) x=measure('')=0,  width=measure('e')=10,  y=40
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, 1, 5, mockMeasure)
  assert.deepEqual(rects, [
    { x: 10, y: 0, width: 10, height: 20 },
    { x: 0, y: 20, width: 20, height: 20 },
    { x: 0, y: 40, width: 10, height: 20 },
  ])
})
test('computeSelectionRects: selStart>selEnd 自动交换', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const r1 = computeSelectionRects('abcdef', lines, 20, 1, 3, mockMeasure)
  const r2 = computeSelectionRects('abcdef', lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(r1, r2)
})
test('computeSelectionRects: 选区起点越界 clamp 到行首', () => {
  // selStart=-5, selEnd=2 → start=0, end=2 → x=0, width=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, -5, 2, mockMeasure)
  assert.deepEqual(rects, [{ x: 0, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 0, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 0 }])
})
test('computeSelectionRects: 中文选区', () => {
  // '你好世界' 容器 25px 每行 2 字 → 2 行：[0,2)[2,4)
  // selStart=0, selEnd=3：
  //   行0: [0,2) x=0, width=measure('你好')=20, y=0
  //   行1: [2,3) x=0, width=measure('世')=10, y=20
  const lines = layoutLines('你好世界', 25, mockMeasure)
  const rects = computeSelectionRects('你好世界', lines, 20, 0, 3, mockMeasure)
  assert.deepEqual(rects, [
    { x: 0, y: 0, width: 20, height: 20 },
    { x: 0, y: 20, width: 10, height: 20 },
  ])
})

// ── computeCaretRect ──
test('computeCaretRect: 空文本返回 null', () => {
  assert.equal(computeCaretRect('', [], 20, 0, mockMeasure), null)
})
test('computeCaretRect: 光标在行首 x=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 0, mockMeasure)
  assert.deepEqual(caret, { x: 0, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 光标在行中', () => {
  // cursor=2 → x=measure('ab')=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 2, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 光标在行尾（单行）', () => {
  // cursor=6 → x=measure('abcdef')=60
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 6, mockMeasure)
  assert.deepEqual(caret, { x: 60, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: soft-wrap 边界 Upstream → 上一行尾', () => {
  // text='abcdef' 容器 30px → 2 行 [0,3) [3,6)
  // cursor=3 在 soft-wrap 边界：Upstream → 行0 末尾，x=measure('abc')=30, y=0
  const lines = layoutLines('abcdef', 30, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 3, mockMeasure, CaretAffinity.Upstream)
  assert.deepEqual(caret, { x: 30, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: soft-wrap 边界 Downstream → 下一行首', () => {
  // cursor=3 在 soft-wrap 边界：Downstream → 行1 首，x=0, y=20
  const lines = layoutLines('abcdef', 30, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 3, mockMeasure, CaretAffinity.Downstream)
  assert.deepEqual(caret, { x: 0, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: hard break 两侧 offset 不同不靠 affinity', () => {
  // text='a\nb' 容器 1000px → 2 行 [0,1 HardBreak] [2,3 EndOfText]
  // cursor=1 → 行0 末尾，x=measure('a')=10
  const lines = layoutLines('a\nb', 1000, mockMeasure)
  const caret = computeCaretRect('a\nb', lines, 20, 1, mockMeasure)
  assert.deepEqual(caret, { x: 10, y: 0, width: CARET_WIDTH_PX, height: 20 })
  // cursor=2 → 行1 首，x=0
  const caret2 = computeCaretRect('a\nb', lines, 20, 2, mockMeasure)
  assert.deepEqual(caret2, { x: 0, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 多行光标在行中归到对应行', () => {
  // cursor=3 → line.end=4>=3 归到行1，x=measure(text.substring(2,3))=measure('c')=10, y=20
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 3, mockMeasure)
  assert.deepEqual(caret, { x: 10, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: cursor=4 归到行1行尾', () => {
  // cursor=4 → line.end=4>=4 归到行1，x=measure('cd')=20, y=20
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 4, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: cursor=5 归到行2', () => {
  // cursor=5 → line.end=6>=5 归到行2，x=measure('e')=10, y=40
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 5, mockMeasure)
  assert.deepEqual(caret, { x: 10, y: 40, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 越界 cursor<0 clamp 到 0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, -5, mockMeasure)
  assert.deepEqual(caret, { x: 0, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 越界 cursor>text.length clamp 到行尾', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 100, mockMeasure)
  assert.deepEqual(caret, { x: 60, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 0, 2, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 0, width: CARET_WIDTH_PX, height: 0 })
})

// ── computeCompositionUnderlineRects ──
test('computeCompositionUnderlineRects: 空文本返回 []', () => {
  assert.deepEqual(computeCompositionUnderlineRects('', [], 20, 0, 0, mockMeasure), [])
})
test('computeCompositionUnderlineRects: null start 返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, null, 3, mockMeasure), [])
})
test('computeCompositionUnderlineRects: null end 返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, 1, null, mockMeasure), [])
})
test('computeCompositionUnderlineRects: 空范围返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, 2, 2, mockMeasure), [])
})
test("computeCompositionUnderlineRects: compStart>compEnd 自动交换成 [1,3)", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const rects = computeCompositionUnderlineRects("abcdef", lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 单行 composition 下划线在行底', () => {
  // text='abcdef' 容器 1000px 单行，compStart=1, compEnd=3, lineSpacing=20
  // x=measure('a')=10, width=measure('bc')=20
  // y = 0*20 + 20 - 2 = 18, height = 2
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 多行 composition 每行一个下划线', () => {
  // text='abcdef' 容器 25px → 3 行 [0,2)[2,4)[4,6)
  // compStart=1, compEnd=5, lineSpacing=20：
  //   行0: x=10, y=0+20-2=18,  width=10
  //   行1: x=0,  y=20+20-2=38, width=20
  //   行2: x=0,  y=40+20-2=58, width=10
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const rects = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 5, mockMeasure)
  assert.deepEqual(rects, [
    { x: 10, y: 18, width: 10, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 38, width: 20, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 58, width: 10, height: UNDERLINE_HEIGHT_PX },
  ])
})
test("computeCompositionUnderlineRects: compStart>compEnd 自动交换", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const r1 = computeCompositionUnderlineRects("abcdef", lines, 20, 1, 3, mockMeasure)
  const r2 = computeCompositionUnderlineRects("abcdef", lines, 20, 3, 1, mockMeasure)
  // compStart>compEnd 自动交换，r1 和 r2 应相等
  assert.deepEqual(r1, r2)
  assert.deepEqual(r1, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 中文 composition', () => {
  // '你好世界' 容器 25px → 2 行 [0,2)[2,4)
  // compStart=0, compEnd=3, lineSpacing=20：
  //   行0: x=0, y=18, width=20
  //   行1: x=0, y=38, width=10
  const lines = layoutLines('你好世界', 25, mockMeasure)
  const rects = computeCompositionUnderlineRects('你好世界', lines, 20, 0, 3, mockMeasure)
  assert.deepEqual(rects, [
    { x: 0, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 38, width: 10, height: UNDERLINE_HEIGHT_PX },
  ])
})

// ── 端到端：snapshot 几何一致性 ──
test('端到端: 选区/光标/composition 共存于多行文本', () => {
  // text='abcdef' 容器 25px → 3 行
  // 选区 [1,5)，光标 cursor=5，composition [1,5)
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const sel = computeSelectionRects('abcdef', lines, 20, 1, 5, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 5, mockMeasure)
  const comp = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 5, mockMeasure)
  // 选区 3 个 rect（每行一个）
  assert.equal(sel.length, 3)
  // 光标在行2，y=40
  assert.equal(caret.y, 40)
  // composition 3 个下划线
  assert.equal(comp.length, 3)
  // composition 下划线 y = 选区 y + lineSpacing - UNDERLINE_HEIGHT
  for (let i = 0; i < sel.length; i++) {
    assert.equal(comp[i].y, sel[i].y + 20 - UNDERLINE_HEIGHT_PX)
    assert.equal(comp[i].x, sel[i].x)
    assert.equal(comp[i].width, sel[i].width)
  }
})

test('端到端: emoji surrogate pair 选区不切断', () => {
  // 'a😀b' 容器 15px → 3 行 [0,1)[1,3)[3,4)
  // 选区 [0,3) 跨 emoji：行0 [0,1)，行1 [1,3)（含完整 emoji）
  const lines = layoutLines('a😀b', 15, mockMeasure)
  const sel = computeSelectionRects('a😀b', lines, 20, 0, 3, mockMeasure)
  assert.equal(sel.length, 2)
  // 行0: x=0, width=measure('a')=10
  assert.equal(sel[0].x, 0)
  assert.equal(sel[0].width, 10)
  // 行1: x=0, width=measure('😀')=20（2 code units * 10）
  assert.equal(sel[1].x, 0)
  assert.equal(sel[1].width, 20)
})

// ════════════════════════════════════════════════════════════════════
// ── Issue #629 评论7 第3项：EditorLayoutSnapshot composition 显示投影 ──
// 纯逻辑：与 EditorLayoutSnapshot.ets + TextOffsetMapper.ets 对齐。
// 验证 composition 活跃时显示光标/选区按 preeditCursorUtf16→UTF8 byte 投影到 displayText。
// ════════════════════════════════════════════════════════════════════

// ── 纯逻辑：与 TextOffsetMapper.ets 对齐 ──
class TextOffsetMapper {
  static utf16ToUtf8(text, utf16Offset) {
    if (utf16Offset <= 0) return 0
    const limited = utf16Offset > text.length ? text.length : utf16Offset
    const sub = text.substring(0, limited)
    return new TextEncoder().encode(sub).length
  }
  static utf8ToUtf16(text, utf8Offset) {
    if (utf8Offset <= 0) return 0
    let byteLen = 0
    let utf16Index = 0
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i)
      let charByteLen = 1
      if (code < 0x80) {
        charByteLen = 1
      } else if (code < 0x800) {
        charByteLen = 2
      } else if (code >= 0xD800 && code <= 0xDBFF) {
        charByteLen = 4
        i += 1
      } else {
        charByteLen = 3
      }
      if (byteLen + charByteLen > utf8Offset) {
        return utf16Index
      }
      byteLen += charByteLen
      utf16Index += (charByteLen === 4 ? 2 : 1)
    }
    return utf16Index
  }
}

// ── 纯逻辑：与 EditorLayoutSnapshot.ets 对齐 ──
class EditorLayoutSnapshot {
  constructor(text, selectionAnchor, selectionHead, compositionStart, compositionEnd,
              cursorByteOffset, revision, fontSize = 16) {
    this.text = text
    this.selectionAnchor = selectionAnchor
    this.selectionHead = selectionHead
    this.compositionStart = compositionStart
    this.compositionEnd = compositionEnd
    this.cursorByteOffset = cursorByteOffset
    this.revision = revision
    this.fontSize = fontSize
  }
  static fromEditorSnapshot(snap, fontSize) {
    const composition = snap.composition
    if (composition === null || composition === undefined) {
      return new EditorLayoutSnapshot(
        snap.text, snap.selectionAnchor, snap.cursor,
        null, null, snap.cursor, snap.revision, fontSize ?? 16)
    }
    const committedText = snap.text
    const startUtf16 = TextOffsetMapper.utf8ToUtf16(committedText, composition.replaceByteStart)
    const endUtf16 = TextOffsetMapper.utf8ToUtf16(committedText, composition.replaceByteEndExclusive)
    const before = committedText.substring(0, startUtf16)
    const after = committedText.substring(endUtf16)
    const displayText = before + composition.preeditText + after
    const compositionStart = composition.replaceByteStart
    const preeditUtf8Len = new TextEncoder().encode(composition.preeditText).length
    const compositionEnd = composition.replaceByteStart + preeditUtf8Len
    // Issue #629 评论7 第3项：preedit cursor 投影
    const preeditCursorByte = TextOffsetMapper.utf16ToUtf8(
      composition.preeditText, composition.preeditCursorUtf16)
    const displayCaretByte = composition.replaceByteStart + preeditCursorByte
    return new EditorLayoutSnapshot(
      displayText, displayCaretByte, displayCaretByte,
      compositionStart, compositionEnd, displayCaretByte,
      snap.revision, fontSize ?? 16)
  }
}

console.log('---')
console.log('Issue #629 评论7 第3项：EditorLayoutSnapshot composition 显示投影')
console.log('---')

// ── 无 composition：行为不变 ──
test('composition 投影: 无 composition → text=committed, cursor=snap.cursor', () => {
  const snap = {
    text: 'abc', revision: 1, cursor: 2, selectionAnchor: 1,
    generation: 0, chapterId: 'c1', composition: null
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'abc')
  assert.equal(layout.selectionAnchor, 1)
  assert.equal(layout.selectionHead, 2)
  assert.equal(layout.cursorByteOffset, 2)
  assert.equal(layout.compositionStart, null)
  assert.equal(layout.compositionEnd, null)
})

// ── composition displayText 构造 ──
test('composition 投影: displayText = before + preedit + after', () => {
  // committed = "abc你好def"（"你好" 在 UTF-8 byte 3..9）
  // replaceByteStart=3, replaceByteEndExclusive=9, preeditText="世界"
  // displayText = "abc世界def"
  const snap = {
    text: 'abc你好def', revision: 1, cursor: 3, selectionAnchor: 3,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 3, replaceByteEndExclusive: 9,
      preeditText: '世界', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'abc世界def')
})

test('composition 投影: preeditText 为空 → displayText = committed 删掉 replace range', () => {
  // committed = "abcdef", replace [1,4) → displayText = "a" + "" + "ef" = "aef"
  const snap = {
    text: 'abcdef', revision: 1, cursor: 1, selectionAnchor: 1,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 1, replaceByteEndExclusive: 4,
      preeditText: '', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'aef')
})

// ── composition underline 范围 [replaceByteStart, replaceByteStart + preeditUtf8Len) ──
test('composition 投影: underline 范围 = [replaceByteStart, replaceByteStart+preeditUtf8Len)', () => {
  // preeditText="你好" → preeditUtf8Len=6, replaceByteStart=0
  // compositionStart=0, compositionEnd=6
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.compositionStart, 0)
  assert.equal(layout.compositionEnd, 6)
})

test('composition 投影: underline 范围 replaceByteStart>0', () => {
  // committed="abc你好def", replace [3,9), preedit="世界"
  // preeditUtf8Len=6, compositionStart=3, compositionEnd=9
  const snap = {
    text: 'abc你好def', revision: 1, cursor: 3, selectionAnchor: 3,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 3, replaceByteEndExclusive: 9,
      preeditText: '世界', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.compositionStart, 3)
  assert.equal(layout.compositionEnd, 9)
})

// ── display caret 投影：preeditCursorUtf16 → UTF8 byte → displayCaretByte ──
test('composition 投影: 中文 preedit cursor 在中间 → displayCaretByte=3', () => {
  // preeditText="你好", preeditCursorUtf16=1（"你"之后，UTF-16 offset 1）
  // preeditCursorByte = utf16ToUtf8("你好", 1) = 3（"你" 的 UTF-8 byte len=3）
  // replaceByteStart=0 → displayCaretByte=0+3=3
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 1
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.cursorByteOffset, 3)
  assert.equal(layout.selectionAnchor, 3)
  assert.equal(layout.selectionHead, 3)
  assert.equal(layout.compositionStart, 0)
  assert.equal(layout.compositionEnd, 6)
})

test('composition 投影: preedit cursor 在开头 → displayCaretByte=replaceByteStart', () => {
  // preeditCursorUtf16=0 → preeditCursorByte=0 → displayCaretByte=0
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.cursorByteOffset, 0)
  assert.equal(layout.selectionAnchor, 0)
  assert.equal(layout.selectionHead, 0)
})

test('composition 投影: preedit cursor 在末尾 → displayCaretByte=replaceByteStart+preeditUtf8Len', () => {
  // preeditText="你好", preeditCursorUtf16=2（末尾）
  // preeditCursorByte = utf16ToUtf8("你好", 2) = 6
  // displayCaretByte = 0 + 6 = 6 = compositionEnd
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 2
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.cursorByteOffset, 6)
  assert.equal(layout.selectionAnchor, 6)
  assert.equal(layout.selectionHead, 6)
  assert.equal(layout.cursorByteOffset, layout.compositionEnd)
})

test('composition 投影: replaceByteStart>0 + 中文 cursor 在中间', () => {
  // committed="abc你好def", replace [3,9), preedit="世界", preeditCursorUtf16=1
  // preeditCursorByte = utf16ToUtf8("世界", 1) = 3（"世" 的 UTF-8 byte len=3）
  // displayCaretByte = 3 + 3 = 6
  const snap = {
    text: 'abc你好def', revision: 1, cursor: 3, selectionAnchor: 3,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 3, replaceByteEndExclusive: 9,
      preeditText: '世界', preeditCursorUtf16: 1
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'abc世界def')
  assert.equal(layout.cursorByteOffset, 6)
  assert.equal(layout.selectionAnchor, 6)
  assert.equal(layout.selectionHead, 6)
  assert.equal(layout.compositionStart, 3)
  assert.equal(layout.compositionEnd, 9)
})

test('composition 投影: ASCII preedit cursor', () => {
  // preeditText="xyz", preeditCursorUtf16=2 → preeditCursorByte=2
  // replaceByteStart=0 → displayCaretByte=2
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: 'xyz', preeditCursorUtf16: 2
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'xyz')
  assert.equal(layout.cursorByteOffset, 2)
  assert.equal(layout.selectionAnchor, 2)
  assert.equal(layout.selectionHead, 2)
  assert.equal(layout.compositionStart, 0)
  assert.equal(layout.compositionEnd, 3)
})

test('composition 投影: selection collapse 到 displayCaretByte（不用 snap.selectionAnchor/snap.cursor）', () => {
  // snap.cursor=99, snap.selectionAnchor=88（committed text 坐标，应被忽略）
  // preeditText="你", preeditCursorUtf16=1 → preeditCursorByte=3 → displayCaretByte=3
  const snap = {
    text: '', revision: 1, cursor: 99, selectionAnchor: 88,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你', preeditCursorUtf16: 1
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  // 不用 snap.cursor=99 或 snap.selectionAnchor=88
  assert.equal(layout.cursorByteOffset, 3)
  assert.equal(layout.selectionAnchor, 3)
  assert.equal(layout.selectionHead, 3)
})

test('composition 投影: emoji preedit cursor（surrogate pair）', () => {
  // preeditText="a😀b", 😀 是 surrogate pair 占 UTF-16 index 1-2
  // preeditCursorUtf16=3（😀 之后，'b' 之前，合法光标位置）
  // preeditCursorByte = utf16ToUtf8("a😀b", 3) = UTF-8 byte len of "a😀" = 1 + 4 = 5
  // replaceByteStart=0 → displayCaretByte=5
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: 'a😀b', preeditCursorUtf16: 3
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.text, 'a😀b')
  assert.equal(layout.cursorByteOffset, 5)
  assert.equal(layout.selectionAnchor, 5)
  assert.equal(layout.selectionHead, 5)
  // preeditUtf8Len = 1 + 4 + 1 = 6
  assert.equal(layout.compositionEnd, 6)
})

test('composition 投影: fontSize 透传', () => {
  const snap = {
    text: '', revision: 1, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 1, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 1
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap, 20)
  assert.equal(layout.fontSize, 20)
  const layout2 = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout2.fontSize, 16)
})

test('composition 投影: revision 透传', () => {
  const snap = {
    text: '', revision: 42, cursor: 0, selectionAnchor: 0,
    generation: 0, chapterId: 'c1',
    composition: {
      sessionId: 1, baseRevision: 42, generation: 0,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      preeditText: '你好', preeditCursorUtf16: 0
    }
  }
  const layout = EditorLayoutSnapshot.fromEditorSnapshot(snap)
  assert.equal(layout.revision, 42)
})

console.log('---')
console.log(`✅ editor_render_geometry: ${passed} tests passed`)
