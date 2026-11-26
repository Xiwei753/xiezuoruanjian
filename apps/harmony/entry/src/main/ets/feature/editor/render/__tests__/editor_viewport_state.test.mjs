// editor_viewport_state.test.mjs — EditorViewportState 纯逻辑单测。
//
// 验证 Issue #629 评论 5 第 8 节：viewport 用强类型 EditorViewportState，不用裸 object。
// - 明确字段：scrollTop/scrollLeft/viewportWidth/viewportHeight/contentWidth/contentHeight
// - clamp：越界 offset 归到 [0, maxScroll]
// - canScrollVertical/canScrollHorizontal
// - fromObject 容错：缺失字段用 0，非有限数用 0
// - toObject/fromObject round-trip
// - equals
//
// Issue #1063 评论第 6 条：新增 topVisibleByteOffset（Core UTF-8 byte offset，-1 表示未知），
// 同步覆盖构造函数默认值、clamped 透传、fromObject 缺字段/非法值容错、toObject 字段与 equals。
//
// 运行：node editor_viewport_state.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// ── 纯逻辑：与 EditorViewportState.ets 对齐 ──
// EditorViewportState 是强类型值对象，不用裸 object。

class EditorViewportState {
  constructor(scrollTop = 0, scrollLeft = 0, viewportWidth = 0, viewportHeight = 0,
    contentWidth = 0, contentHeight = 0, topVisibleByteOffset = -1) {
    this.scrollTop = scrollTop
    this.scrollLeft = scrollLeft
    this.viewportWidth = viewportWidth
    this.viewportHeight = viewportHeight
    this.contentWidth = contentWidth
    this.contentHeight = contentHeight
    this.topVisibleByteOffset = topVisibleByteOffset
  }

  clamped() {
    const maxScrollTop = Math.max(0, this.contentHeight - this.viewportHeight)
    const maxScrollLeft = Math.max(0, this.contentWidth - this.viewportWidth)
    return new EditorViewportState(
      Math.max(0, Math.min(this.scrollTop, maxScrollTop)),
      Math.max(0, Math.min(this.scrollLeft, maxScrollLeft)),
      this.viewportWidth,
      this.viewportHeight,
      this.contentWidth,
      this.contentHeight,
      this.topVisibleByteOffset >= 0 ? this.topVisibleByteOffset : -1
    )
  }

  canScrollVertical() {
    return this.contentHeight > this.viewportHeight
  }

  canScrollHorizontal() {
    return this.contentWidth > this.viewportWidth
  }

  static fromObject(obj) {
    const num = (k) => {
      const v = obj[k]
      if (v === undefined || v === null) return 0
      return typeof v === 'number' && isFinite(v) ? v : 0
    }
    const byteOffset = (k) => {
      const v = obj[k]
      if (v === undefined || v === null) return -1
      if (typeof v !== 'number' || !isFinite(v) || !Number.isInteger(v) || v < 0) return -1
      return v
    }
    return new EditorViewportState(
      num('scrollTop'), num('scrollLeft'),
      num('viewportWidth'), num('viewportHeight'),
      num('contentWidth'), num('contentHeight'),
      byteOffset('topVisibleByteOffset')
    )
  }

  toObject() {
    return {
      scrollTop: this.scrollTop,
      scrollLeft: this.scrollLeft,
      viewportWidth: this.viewportWidth,
      viewportHeight: this.viewportHeight,
      contentWidth: this.contentWidth,
      contentHeight: this.contentHeight,
      topVisibleByteOffset: this.topVisibleByteOffset,
    }
  }

  equals(other) {
    return this.scrollTop === other.scrollTop
      && this.scrollLeft === other.scrollLeft
      && this.viewportWidth === other.viewportWidth
      && this.viewportHeight === other.viewportHeight
      && this.contentWidth === other.contentWidth
      && this.contentHeight === other.contentHeight
      && this.topVisibleByteOffset === other.topVisibleByteOffset
  }
}

const ALL_KEYS = ['contentHeight', 'contentWidth', 'scrollLeft', 'scrollTop',
  'topVisibleByteOffset', 'viewportHeight', 'viewportWidth']

console.log('EditorViewportState 纯逻辑单测（强类型视口状态）')
console.log('---')

// ── 1. 明确字段 ──
test('构造：所有字段正确赋值', () => {
  const s = new EditorViewportState(100, 50, 800, 600, 1200, 2000)
  assert.equal(s.scrollTop, 100)
  assert.equal(s.scrollLeft, 50)
  assert.equal(s.viewportWidth, 800)
  assert.equal(s.viewportHeight, 600)
  assert.equal(s.contentWidth, 1200)
  assert.equal(s.contentHeight, 2000)
})

test('构造：缺省值（尺寸归 0，topVisibleByteOffset 为 -1 表示未知）', () => {
  const s = new EditorViewportState()
  assert.equal(s.scrollTop, 0)
  assert.equal(s.scrollLeft, 0)
  assert.equal(s.viewportWidth, 0)
  assert.equal(s.viewportHeight, 0)
  assert.equal(s.contentWidth, 0)
  assert.equal(s.contentHeight, 0)
  assert.equal(s.topVisibleByteOffset, -1)
})

test('构造：显式 topVisibleByteOffset（Core UTF-8 byte offset）', () => {
  const s = new EditorViewportState(10, 20, 30, 40, 50, 60, 1234)
  assert.equal(s.topVisibleByteOffset, 1234)
})

test('强类型：不是裸 object，有明确字段名', () => {
  const s = new EditorViewportState(10, 20, 30, 40, 50, 60)
  // 确认字段名与 EditorViewportState.ets 定义一致
  const keys = Object.keys(s).sort()
  assert.deepEqual(keys, ALL_KEYS)
})

// ── 2. clamp ──
test('clamped: scrollTop 超过 maxScroll → 归到 maxScroll', () => {
  // contentHeight=2000, viewportHeight=600 → maxScrollTop=1400
  const s = new EditorViewportState(9999, 0, 800, 600, 800, 2000)
  const c = s.clamped()
  assert.equal(c.scrollTop, 1400)
})

test('clamped: scrollTop 为负 → 归到 0', () => {
  const s = new EditorViewportState(-50, 0, 800, 600, 800, 2000)
  const c = s.clamped()
  assert.equal(c.scrollTop, 0)
})

test('clamped: scrollLeft 超过 maxScroll → 归到 maxScroll', () => {
  // contentWidth=1200, viewportWidth=800 → maxScrollLeft=400
  const s = new EditorViewportState(0, 9999, 800, 600, 1200, 600)
  const c = s.clamped()
  assert.equal(c.scrollLeft, 400)
})

test('clamped: 正常范围内不变', () => {
  const s = new EditorViewportState(500, 200, 800, 600, 1200, 2000)
  const c = s.clamped()
  assert.equal(c.scrollTop, 500)
  assert.equal(c.scrollLeft, 200)
})

test('clamped: 内容小于视口 → maxScroll=0 → scrollTop 归 0', () => {
  // contentHeight=400 < viewportHeight=600 → maxScrollTop=0
  const s = new EditorViewportState(100, 0, 800, 600, 800, 400)
  const c = s.clamped()
  assert.equal(c.scrollTop, 0)
})

test('clamped: 不改变 viewport/content 尺寸', () => {
  const s = new EditorViewportState(9999, 9999, 800, 600, 1200, 2000)
  const c = s.clamped()
  assert.equal(c.viewportWidth, 800)
  assert.equal(c.viewportHeight, 600)
  assert.equal(c.contentWidth, 1200)
  assert.equal(c.contentHeight, 2000)
})

test('clamped: topVisibleByteOffset 原样透传；负值归一为 -1', () => {
  const known = new EditorViewportState(9999, 0, 800, 600, 800, 2000, 42)
  assert.equal(known.clamped().topVisibleByteOffset, 42)
  const unknown = new EditorViewportState(0, 0, 800, 600, 800, 2000, -1)
  assert.equal(unknown.clamped().topVisibleByteOffset, -1)
  const bogus = new EditorViewportState(0, 0, 800, 600, 800, 2000, -99)
  assert.equal(bogus.clamped().topVisibleByteOffset, -1)
})

// ── 3. canScroll ──
test('canScrollVertical: content > viewport → true', () => {
  const s = new EditorViewportState(0, 0, 800, 600, 800, 2000)
  assert.equal(s.canScrollVertical(), true)
})

test('canScrollVertical: content <= viewport → false', () => {
  const s1 = new EditorViewportState(0, 0, 800, 600, 800, 600)
  assert.equal(s1.canScrollVertical(), false)
  const s2 = new EditorViewportState(0, 0, 800, 600, 800, 400)
  assert.equal(s2.canScrollVertical(), false)
})

test('canScrollHorizontal: content > viewport → true', () => {
  const s = new EditorViewportState(0, 0, 800, 600, 1200, 600)
  assert.equal(s.canScrollHorizontal(), true)
})

test('canScrollHorizontal: content <= viewport → false', () => {
  const s = new EditorViewportState(0, 0, 800, 600, 800, 600)
  assert.equal(s.canScrollHorizontal(), false)
})

// ── 4. fromObject 容错 ──
test('fromObject: 完整对象 → 正确构造', () => {
  const obj = {
    scrollTop: 100, scrollLeft: 50, viewportWidth: 800, viewportHeight: 600,
    contentWidth: 1200, contentHeight: 2000, topVisibleByteOffset: 321
  }
  const s = EditorViewportState.fromObject(obj)
  assert.equal(s.scrollTop, 100)
  assert.equal(s.scrollLeft, 50)
  assert.equal(s.viewportWidth, 800)
  assert.equal(s.viewportHeight, 600)
  assert.equal(s.contentWidth, 1200)
  assert.equal(s.contentHeight, 2000)
  assert.equal(s.topVisibleByteOffset, 321)
})

test('fromObject: 缺失字段 → 尺寸用 0，topVisibleByteOffset 用 -1', () => {
  const s = EditorViewportState.fromObject({ scrollTop: 100 })
  assert.equal(s.scrollTop, 100)
  assert.equal(s.scrollLeft, 0)
  assert.equal(s.viewportWidth, 0)
  assert.equal(s.viewportHeight, 0)
  assert.equal(s.contentWidth, 0)
  assert.equal(s.contentHeight, 0)
  assert.equal(s.topVisibleByteOffset, -1)
})

test('fromObject: 空对象 → 全 0，topVisibleByteOffset = -1', () => {
  const s = EditorViewportState.fromObject({})
  assert.equal(s.scrollTop, 0)
  assert.equal(s.scrollLeft, 0)
  assert.equal(s.viewportWidth, 0)
  assert.equal(s.viewportHeight, 0)
  assert.equal(s.topVisibleByteOffset, -1)
})

test('fromObject: 非有限数（NaN/Infinity/string）→ 用 0 / -1', () => {
  const s = EditorViewportState.fromObject({
    scrollTop: NaN,
    scrollLeft: Infinity,
    viewportWidth: 'abc',
    viewportHeight: null,
    contentWidth: undefined,
    topVisibleByteOffset: 'x',
  })
  assert.equal(s.scrollTop, 0)
  assert.equal(s.scrollLeft, 0)
  assert.equal(s.viewportWidth, 0)
  assert.equal(s.viewportHeight, 0)
  assert.equal(s.contentWidth, 0)
  assert.equal(s.topVisibleByteOffset, -1)
})

test('fromObject: topVisibleByteOffset 非法值（负数/小数/无穷）→ -1', () => {
  assert.equal(EditorViewportState.fromObject({ topVisibleByteOffset: -5 }).topVisibleByteOffset, -1)
  assert.equal(EditorViewportState.fromObject({ topVisibleByteOffset: 1.5 }).topVisibleByteOffset, -1)
  assert.equal(EditorViewportState.fromObject({ topVisibleByteOffset: Infinity }).topVisibleByteOffset, -1)
  assert.equal(EditorViewportState.fromObject({ topVisibleByteOffset: 0 }).topVisibleByteOffset, 0)
})

// ── 5. toObject/fromObject round-trip ──
test('round-trip: toObject → fromObject == identity', () => {
  const original = new EditorViewportState(100, 50, 800, 600, 1200, 2000, 777)
  const obj = original.toObject()
  const restored = EditorViewportState.fromObject(obj)
  assert.equal(restored.scrollTop, original.scrollTop)
  assert.equal(restored.scrollLeft, original.scrollLeft)
  assert.equal(restored.viewportWidth, original.viewportWidth)
  assert.equal(restored.viewportHeight, original.viewportHeight)
  assert.equal(restored.contentWidth, original.contentWidth)
  assert.equal(restored.contentHeight, original.contentHeight)
  assert.equal(restored.topVisibleByteOffset, original.topVisibleByteOffset)
})

test('toObject: 返回含全部 7 个字段', () => {
  const s = new EditorViewportState(1, 2, 3, 4, 5, 6)
  const obj = s.toObject()
  const keys = Object.keys(obj).sort()
  assert.deepEqual(keys, ALL_KEYS)
  assert.equal(obj.topVisibleByteOffset, -1)
})

// ── 6. equals ──
test('equals: 全字段相同 → true', () => {
  const a = new EditorViewportState(1, 2, 3, 4, 5, 6, 7)
  const b = new EditorViewportState(1, 2, 3, 4, 5, 6, 7)
  assert.equal(a.equals(b), true)
})

test('equals: 任一字段不同 → false', () => {
  const a = new EditorViewportState(1, 2, 3, 4, 5, 6, 7)
  assert.equal(a.equals(new EditorViewportState(99, 2, 3, 4, 5, 6, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 99, 3, 4, 5, 6, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 2, 99, 4, 5, 6, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 2, 3, 99, 5, 6, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 2, 3, 4, 99, 6, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 2, 3, 4, 5, 99, 7)), false)
  assert.equal(a.equals(new EditorViewportState(1, 2, 3, 4, 5, 6, 99)), false)
})

test('equals: 未知 topVisibleByteOffset（-1）与 0 不等价', () => {
  const a = new EditorViewportState(0, 0, 800, 600, 800, 600, -1)
  const b = new EditorViewportState(0, 0, 800, 600, 800, 600, 0)
  assert.equal(a.equals(b), false)
})

// ── 7. 接续恢复场景 ──
test('接续恢复: 源设备 viewport → 目标设备 clamp（内容尺寸不同）', () => {
  // 源设备：scrollTop=1500, contentHeight=3000
  const sourceViewport = new EditorViewportState(1500, 0, 800, 600, 800, 3000, 1200)
  // 目标设备：contentHeight=1000（内容更短，如同步后删了些段落）
  const targetViewport = new EditorViewportState(
    sourceViewport.scrollTop, sourceViewport.scrollLeft,
    800, 600, 800, 1000, sourceViewport.topVisibleByteOffset
  )
  const clamped = targetViewport.clamped()
  // maxScrollTop = 1000 - 600 = 400，scrollTop=1500 归到 400
  assert.equal(clamped.scrollTop, 400)
  // byte 锚点不随像素 clamp 变化（真实可恢复位置由 TextOffsetMapper + 正文长度再 clamp）
  assert.equal(clamped.topVisibleByteOffset, 1200)
})

test('接续恢复: 初始 viewport（全 0 / -1）clamp 后 scrollTop 仍 0、byte 仍 -1', () => {
  const s = new EditorViewportState(0, 0, 0, 0, 0, 0)
  const c = s.clamped()
  assert.equal(c.scrollTop, 0)
  assert.equal(c.scrollLeft, 0)
  assert.equal(c.topVisibleByteOffset, -1)
})

console.log('---')
console.log(`✅ editor_viewport_state: ${passed} tests passed`)
