// editor_viewport_controller.test.mjs — EditorViewportController 纯逻辑单测。
//
// 验证 Issue #629 评论6 Part C：父子 viewport 恢复通道。
// - register(scrollTo, getDimensions)：SujianEditor 注册实现
// - restore(state)：WritingScreen 调用，已注册时立即 clamp + scrollTo，未注册时缓存
// - pendingRestore：注册前调 restore，注册后立即执行
// - clamp 基于目标设备当前 viewport/content 尺寸，不直接拿源设备保存的尺寸裁剪
// - unregister：注销后 restore 缓存
// - isRegistered：判断 layout 是否就绪
//
// 运行：node editor_viewport_controller.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// ── 纯逻辑：与 EditorViewportController.ets 对齐 ──

class EditorViewportState {
  constructor(scrollTop = 0, scrollLeft = 0, viewportWidth = 0, viewportHeight = 0, contentWidth = 0, contentHeight = 0) {
    this.scrollTop = scrollTop
    this.scrollLeft = scrollLeft
    this.viewportWidth = viewportWidth
    this.viewportHeight = viewportHeight
    this.contentWidth = contentWidth
    this.contentHeight = contentHeight
  }
}

class EditorViewportController {
  constructor() {
    this.scrollToImpl = null
    this.getDimensionsImpl = null
    this.pendingRestore = null
  }
  register(scrollTo, getDimensions) {
    this.scrollToImpl = scrollTo
    this.getDimensionsImpl = getDimensions
    if (this.pendingRestore) {
      this.doRestore(this.pendingRestore)
      this.pendingRestore = null
    }
  }
  unregister() {
    this.scrollToImpl = null
    this.getDimensionsImpl = null
  }
  restore(state) {
    if (this.scrollToImpl && this.getDimensionsImpl) {
      this.doRestore(state)
    } else {
      this.pendingRestore = state
    }
  }
  doRestore(state) {
    if (!this.scrollToImpl || !this.getDimensionsImpl) return
    const dims = this.getDimensionsImpl()
    const maxScrollTop = Math.max(0, dims.contentHeight - dims.viewportHeight)
    const maxScrollLeft = Math.max(0, dims.contentWidth - dims.viewportWidth)
    const clampedX = Math.max(0, Math.min(state.scrollLeft, maxScrollLeft))
    const clampedY = Math.max(0, Math.min(state.scrollTop, maxScrollTop))
    this.scrollToImpl(clampedX, clampedY)
  }
  isRegistered() {
    return this.scrollToImpl !== null && this.getDimensionsImpl !== null
  }
}

console.log('EditorViewportController 纯逻辑单测（父子 viewport 恢复通道）')
console.log('---')

// ── 1. isRegistered ──
test('isRegistered: 新建 controller → false', () => {
  const c = new EditorViewportController()
  assert.equal(c.isRegistered(), false)
})

test('isRegistered: register 后 → true', () => {
  const c = new EditorViewportController()
  c.register(() => {}, () => ({ viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }))
  assert.equal(c.isRegistered(), true)
})

test('isRegistered: unregister 后 → false', () => {
  const c = new EditorViewportController()
  c.register(() => {}, () => ({ viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }))
  c.unregister()
  assert.equal(c.isRegistered(), false)
})

// ── 2. restore 已注册：立即 clamp + scrollTo ──
test('restore: 已注册 → 立即 scrollTo（正常范围内不变）', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 2000 })
  )
  // state.scrollTop=500，maxScrollTop=2000-600=1400，500 在范围内
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.deepEqual(scrolled, { x: 0, y: 500 })
})

test('restore: 已注册 → clamp scrollTop 到 maxScroll', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1000 })
  )
  // 目标设备 contentHeight=1000，viewportHeight=600 → maxScrollTop=400
  // 源设备 state.scrollTop=1500（源设备 contentHeight=3000），clamp 到 400
  c.restore(new EditorViewportState(1500, 0, 800, 600, 800, 3000))
  assert.equal(scrolled.y, 400)
})

test('restore: 已注册 → clamp scrollLeft 到 maxScroll', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 1000, contentHeight: 600 })
  )
  // 目标设备 contentWidth=1000，viewportWidth=800 → maxScrollLeft=200
  c.restore(new EditorViewportState(0, 9999, 800, 600, 1200, 600))
  assert.equal(scrolled.x, 200)
})

test('restore: 已注册 → 负 offset 归 0', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 2000 })
  )
  c.restore(new EditorViewportState(-50, -30, 800, 600, 800, 2000))
  assert.equal(scrolled.x, 0)
  assert.equal(scrolled.y, 0)
})

test('restore: 已注册 → 内容小于视口 → maxScroll=0 → offset 归 0', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 400 })
  )
  // 目标设备内容比视口小，maxScrollTop=0
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrolled.y, 0)
})

// ── 3. clamp 基于目标设备当前尺寸，不拿源设备保存的尺寸裁剪 ──
test('clamp: 用目标设备当前 viewport/content 尺寸，不用 state 里的源设备尺寸', () => {
  const c = new EditorViewportController()
  let scrolled = null
  c.register(
    (x, y) => { scrolled = { x, y } },
    // 目标设备当前尺寸：viewportHeight=800, contentHeight=1500 → maxScrollTop=700
    () => ({ viewportWidth: 1000, viewportHeight: 800, contentWidth: 1000, contentHeight: 1500 })
  )
  // state 里的源设备尺寸：viewportHeight=600, contentHeight=3000
  // 如果错误地用 state 里的尺寸，maxScrollTop=3000-600=2400，scrollTop=2000 不 clamp
  // 正确：用目标设备尺寸，maxScrollTop=1500-800=700，scrollTop=2000 clamp 到 700
  c.restore(new EditorViewportState(2000, 0, 1000, 600, 1000, 3000))
  assert.equal(scrolled.y, 700)
})

// ── 4. pendingRestore：注册前调 restore，注册后立即执行 ──
test('pendingRestore: 注册前 restore → 缓存，不调 scrollTo', () => {
  const c = new EditorViewportController()
  let scrollCalled = false
  // 未注册，restore 应缓存，不调 scrollTo
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(c.pendingRestore !== null, true)
  assert.equal(scrollCalled, false)
})

test('pendingRestore: 注册后立即执行缓存的 restore', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // 先 restore（未注册，缓存）
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  // 注册后应立即执行 pendingRestore
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 2000 })
  )
  assert.equal(scrolled !== null, true)
  assert.equal(scrolled.y, 500)
  // pendingRestore 已清空
  assert.equal(c.pendingRestore, null)
})

test('pendingRestore: 注册后缓存的 restore 也按目标设备尺寸 clamp', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // 源设备 scrollTop=1500, contentHeight=3000
  c.restore(new EditorViewportState(1500, 0, 800, 600, 800, 3000))
  // 目标设备 contentHeight=1000 → maxScrollTop=400
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1000 })
  )
  assert.equal(scrolled.y, 400)
})

// ── 5. unregister ──
test('unregister: 注销后 restore → 缓存（不调 scrollTo）', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 2000 })
  )
  c.unregister()
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrollCount, 0)
  assert.equal(c.pendingRestore !== null, true)
})

// ── 6. 接续恢复完整闭环 ──
test('闭环: WritingScreen restore → SujianEditor scrollTo（按目标设备尺寸 clamp）', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // 模拟 SujianEditor 注册（layout 就绪）
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1800 })
  )
  // 模拟 WritingScreen 接续恢复：源设备 scrollTop=1200
  c.restore(new EditorViewportState(1200, 0, 800, 600, 800, 3000))
  // 目标设备 maxScrollTop=1800-600=1200，scrollTop=1200 刚好在边界
  assert.equal(scrolled.y, 1200)
})

test('闭环: 接续加载完成早于 layout 就绪 → pendingRestore → 注册后执行', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // WritingScreen 接续加载完成，调 restore，但 SujianEditor 还未注册（layout 未就绪）
  c.restore(new EditorViewportState(800, 0, 800, 600, 800, 2000))
  assert.equal(scrolled, null)
  // SujianEditor layout 就绪，注册
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1500 })
  )
  // 注册后 pendingRestore 立即执行，目标设备 maxScrollTop=1500-600=900
  // 源 scrollTop=800 在范围内，不 clamp
  assert.equal(scrolled.y, 800)
})

console.log('---')
console.log(`✅ editor_viewport_controller: ${passed} tests passed`)
