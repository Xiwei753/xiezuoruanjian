// editor_viewport_controller.test.mjs — EditorViewportController 纯逻辑单测。
//
// 验证 Issue #629 评论6 Part C + 评论7 第4项：父子 viewport 恢复通道。
// - register(scrollTo, getDimensions)：SujianEditor 注册实现
// - restore(state)：WritingScreen 调用，缓存 pending 后尝试消费
// - pendingRestore：注册前或尺寸未就绪时调 restore，缓存等就绪后执行
// - Issue #629 评论7 第4项：区分 registered 与 dimensionsReady
//   - register 不立即消费 pending（尺寸可能为 0）；tryConsumePending 检查 dimensionsReady
//   - notifyDimensionsChanged()：SujianEditor onAreaChange 更新尺寸后调用，重试消费 pending
//   - 尺寸未就绪（viewportWidth/Height 或 contentWidth/Height 为 0）时保留 pending，不 clamp 成 0
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
    // 注册后不立即消费 pending：尺寸可能为 0。尝试一次，就绪才消费。
    this.tryConsumePending()
  }
  unregister() {
    this.scrollToImpl = null
    this.getDimensionsImpl = null
  }
  restore(state) {
    this.pendingRestore = state
    this.tryConsumePending()
  }
  // Issue #629 评论7 第4项：onAreaChange 更新尺寸后调用，重试消费 pending。
  notifyDimensionsChanged() {
    this.tryConsumePending()
  }
  tryConsumePending() {
    if (!this.scrollToImpl || !this.getDimensionsImpl) return
    if (this.pendingRestore === null) return
    if (!this.dimensionsReady()) return
    this.doRestore(this.pendingRestore)
    this.pendingRestore = null
  }
  dimensionsReady() {
    if (!this.getDimensionsImpl) return false
    const dims = this.getDimensionsImpl()
    return dims.viewportWidth > 0 && dims.viewportHeight > 0
      && dims.contentWidth > 0 && dims.contentHeight > 0
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

// ── 2. restore 已注册且尺寸就绪：立即 clamp + scrollTo ──
test('restore: 已注册且尺寸就绪 → 立即 scrollTo（正常范围内不变）', () => {
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

test('restore: 已注册且尺寸就绪 → clamp scrollTop 到 maxScroll', () => {
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

test('restore: 已注册且尺寸就绪 → clamp scrollLeft 到 maxScroll', () => {
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

test('restore: 已注册且尺寸就绪 → 负 offset 归 0', () => {
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

test('restore: 已注册且尺寸就绪 → 内容小于视口 → maxScroll=0 → offset 归 0', () => {
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

// ── 4. pendingRestore：注册前调 restore，注册后尺寸就绪才执行 ──
test('pendingRestore: 注册前 restore → 缓存，不调 scrollTo', () => {
  const c = new EditorViewportController()
  let scrollCalled = false
  // 未注册，restore 应缓存，不调 scrollTo
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(c.pendingRestore !== null, true)
  assert.equal(scrollCalled, false)
})

test('pendingRestore: 注册后尺寸就绪 → 立即执行缓存的 restore', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // 先 restore（未注册，缓存）
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  // 注册后尺寸就绪，应立即执行 pendingRestore
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

// ════════════════════════════════════════════════════════════════════
// ── 7. Issue #629 评论7 第4项：区分 registered 与 dimensionsReady ──
// ════════════════════════════════════════════════════════════════════

test('评论7第4项: register 时尺寸全 0 → 不消费 pending，scrollTo 不被调', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  // 先 restore（缓存 pending）
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  // 注册时尺寸全 0（layout 未就绪），不应消费 pending
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 })
  )
  assert.equal(scrollCount, 0)
  // pending 保留（不 clamp 成 0，不丢位置）
  assert.equal(c.pendingRestore !== null, true)
  assert.equal(c.pendingRestore.scrollTop, 500)
})

test('评论7第4项: 已注册但尺寸全 0 → restore 保留 pending，不调 scrollTo', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  // 注册时尺寸全 0
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 })
  )
  // restore 时尺寸未就绪，应保留 pending
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrollCount, 0)
  assert.equal(c.pendingRestore !== null, true)
  assert.equal(c.pendingRestore.scrollTop, 500)
})

test('评论7第4项: 尺寸部分为 0（viewportHeight=0）→ 仍保留 pending', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  // viewportHeight=0（Scroll onAreaChange 还没回调），content 尺寸已就绪
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: 800, viewportHeight: 0, contentWidth: 800, contentHeight: 2000 })
  )
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrollCount, 0)
  assert.equal(c.pendingRestore !== null, true)
})

test('评论7第4项: 尺寸部分为 0（contentHeight=0）→ 仍保留 pending', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  // contentHeight=0（Text onAreaChange 还没回调），viewport 尺寸已就绪
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 0 })
  )
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrollCount, 0)
  assert.equal(c.pendingRestore !== null, true)
})

// ── 8. notifyDimensionsChanged：onAreaChange 重试消费 pending ──

test('评论7第4项: notifyDimensionsChanged 尺寸就绪后消费 pending', () => {
  const c = new EditorViewportController()
  let scrolled = null
  // 用可变 dims 模拟 onAreaChange 更新尺寸
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  // restore 时尺寸全 0，pending 保留
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  assert.equal(scrolled, null)
  assert.equal(c.pendingRestore !== null, true)
  // 模拟 onAreaChange：尺寸就绪
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  dims.contentWidth = 800
  dims.contentHeight = 2000
  // 通知 controller 尺寸变化
  c.notifyDimensionsChanged()
  // pending 被消费，scrollTo 被调
  assert.deepEqual(scrolled, { x: 0, y: 500 })
  assert.equal(c.pendingRestore, null)
})

test('评论7第4项: notifyDimensionsChanged 尺寸仍未就绪 → 保留 pending', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  c.register(
    () => { scrollCount++ },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  // 模拟 onAreaChange：只有 viewport 尺寸就绪，content 仍为 0
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  c.notifyDimensionsChanged()
  // 尺寸未完全就绪，pending 仍保留
  assert.equal(scrollCount, 0)
  assert.equal(c.pendingRestore !== null, true)
})

test('评论7第4项: onAreaChange 重试 → scrollTo 按目标尺寸 clamp', () => {
  const c = new EditorViewportController()
  let scrolled = null
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  // 源设备 scrollTop=1500, contentHeight=3000
  c.restore(new EditorViewportState(1500, 0, 800, 600, 800, 3000))
  assert.equal(scrolled, null)
  // 模拟 onAreaChange：目标设备 contentHeight=1000 → maxScrollTop=400
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  dims.contentWidth = 800
  dims.contentHeight = 1000
  c.notifyDimensionsChanged()
  // scrollTop=1500 clamp 到 400
  assert.equal(scrolled.y, 400)
  assert.equal(c.pendingRestore, null)
})

test('评论7第4项: 多次 notifyDimensionsChanged 只消费一次 pending', () => {
  const c = new EditorViewportController()
  let scrollCount = 0
  let lastScrolled = null
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  c.register(
    (x, y) => { scrollCount++; lastScrolled = { x, y } },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  c.restore(new EditorViewportState(500, 0, 800, 600, 800, 2000))
  // 尺寸就绪
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  dims.contentWidth = 800
  dims.contentHeight = 2000
  c.notifyDimensionsChanged()
  assert.equal(scrollCount, 1)
  assert.equal(c.pendingRestore, null)
  // 再次 notify：pending 已空，不应再调 scrollTo
  c.notifyDimensionsChanged()
  assert.equal(scrollCount, 1)
})

test('评论7第4项: clamp 按目标尺寸（scrollLeft/scrollTop 超过范围 clamp 到 maxScroll）', () => {
  const c = new EditorViewportController()
  let scrolled = null
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  // 源设备 scrollLeft=5000, scrollTop=5000
  c.restore(new EditorViewportState(5000, 5000, 1200, 800, 2000, 3000))
  // 目标设备 viewportWidth=800, viewportHeight=600, contentWidth=1000, contentHeight=1500
  // maxScrollLeft = 1000-800 = 200, maxScrollTop = 1500-600 = 900
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  dims.contentWidth = 1000
  dims.contentHeight = 1500
  c.notifyDimensionsChanged()
  assert.equal(scrolled.x, 200)
  assert.equal(scrolled.y, 900)
})

test('评论7第4项: 完整闭环 restore→register(尺寸0)→onAreaChange→消费', () => {
  const c = new EditorViewportController()
  let scrolled = null
  const dims = { viewportWidth: 0, viewportHeight: 0, contentWidth: 0, contentHeight: 0 }
  // 1. WritingScreen 接续加载完成，调 restore（controller 未注册）
  c.restore(new EditorViewportState(800, 0, 800, 600, 800, 2000))
  assert.equal(scrolled, null)
  // 2. SujianEditor aboutToAppear 注册（此时 onAreaChange 还没回调，尺寸全 0）
  c.register(
    (x, y) => { scrolled = { x, y } },
    () => ({ viewportWidth: dims.viewportWidth, viewportHeight: dims.viewportHeight,
             contentWidth: dims.contentWidth, contentHeight: dims.contentHeight })
  )
  // 注册时尺寸全 0，pending 保留
  assert.equal(scrolled, null)
  assert.equal(c.pendingRestore !== null, true)
  // 3. Scroll onAreaChange：viewport 尺寸就绪
  dims.viewportWidth = 800
  dims.viewportHeight = 600
  c.notifyDimensionsChanged()
  // content 仍为 0，pending 保留
  assert.equal(scrolled, null)
  // 4. Text onAreaChange：content 尺寸就绪
  dims.contentWidth = 800
  dims.contentHeight = 1500
  c.notifyDimensionsChanged()
  // 尺寸全就绪，pending 消费，scrollTo 被调
  // maxScrollTop = 1500-600 = 900, scrollTop=800 在范围内
  assert.deepEqual(scrolled, { x: 0, y: 800 })
  assert.equal(c.pendingRestore, null)
})

console.log('---')
console.log(`✅ editor_viewport_controller: ${passed} tests passed`)
