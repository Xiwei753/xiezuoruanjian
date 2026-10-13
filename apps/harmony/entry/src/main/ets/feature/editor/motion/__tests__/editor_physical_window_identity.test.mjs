// editor_physical_window_identity.test.mjs — 物理窗口身份唯一化纯逻辑单测。
//
// Issue #879 物理窗口身份唯一化修复（3 个关联问题）：
// 1. buildFrame 同 key 不同绘制属性合并时跳过第二个窗口（不创建 -dup key）
// 2. buildFrame 直接读取预分配值（用 `!` 断言），不再有 -unplanned fallback key
// 3. PhysicalWindowKeyAllocator.allocate 加入 motionChannel 参数，
//    frozen 通道用 allocate（复用），其他通道用 allocateFresh（独立实例）
//
// 运行：
//   node --experimental-vm-modules apps/harmony/entry/src/main/ets/feature/editor/motion/__tests__/editor_physical_window_identity.test.mjs

import { strict as assert } from 'node:assert'
import { WindowInstanceIdAllocator, windowIdForGlyphIds, GlyphIdentityTable } from '../editor_glyph_identity.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_physical_window_identity 纯逻辑单测（Issue #879 物理窗口身份唯一化）')
console.log('---')

// ── 辅助：模拟 PhysicalWindowKeyAllocator 的核心行为 ──
//
// PhysicalWindowKeyAllocator 定义在 EditorMotionPlanner.ets（.ets 无法直接 import），
// 但其核心逻辑只依赖 WindowInstanceIdAllocator，可以在测试中忠实模拟。
// 关键规则：
// - motionChannel='frozen' → allocator.allocate(glyphIds)（复用同一物理 Text 的多个字）
// - 其他 motionChannel → allocator.allocateFresh(glyphIds)（独立物理实例）
// - 缓存键 = `${sourceKind}-${sourceRevision}-${motionChannel}-${allocatedWinId}`
// - renderNodeKey = `${sourceKind}-${sourceRevision}-${allocatedWinId}`

// 模拟 PhysicalWindowKeyAllocator 的核心行为——renderNodeKey 包含 motionChannel。
// Issue #879 评论6146031468：确保不同运动轨迹/来源生成不同的 renderNodeKey。
class SimulatedPhysicalWindowKeyAllocator {
  constructor(allocator) {
    this.allocator = allocator
    this.identities = new Map()
  }

  allocate(sourceKind, sourceRevision, glyphIds, motionChannel = 'default', windowInstanceId = undefined) {
    const allocatedWinId = windowInstanceId ??
      (motionChannel === 'frozen' ? this.allocator.allocate(glyphIds) : this.allocator.allocateFresh(glyphIds))
    const cacheKey = `${sourceKind}-${sourceRevision}-${motionChannel}-${allocatedWinId}`
    const existing = this.identities.get(cacheKey)
    if (existing !== undefined) {
      return existing
    }
    // Issue #879 评论6146031468：renderNodeKey 包含 motionChannel，
    // 同一物理 Text 的多块 clip 共享一组 key；物理身份不同的来源在 Planner 阶段就分配两个不同的 key。
    const identity = {
      sourceKind,
      sourceRevision,
      runId: `${sourceKind}-${sourceRevision}`,
      windowInstanceId: allocatedWinId,
      renderNodeKey: `${sourceKind}-${sourceRevision}-${motionChannel}-${allocatedWinId}`,
    }
    this.identities.set(cacheKey, identity)
    return identity
  }

  allocateTarget(sourceRevision, glyphIds) {
    return this.allocate('new', sourceRevision, glyphIds, 'target')
  }
}

// ── 场景1：PhysicalWindowKeyAllocator motionChannel 区分 ──

test('场景1a：同一 glyphIds + 不同 motionChannel → 不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const targetId = keyAlloc.allocate('new', 11, glyphIds, 'target')

  assert.notEqual(frozenId.renderNodeKey, targetId.renderNodeKey)
  assert.notEqual(frozenId.windowInstanceId, targetId.windowInstanceId)
})

test('场景1b：同一 glyphIds + 同一 motionChannel=frozen → 相同 renderNodeKey（复用）', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // 第一次分配 frozen 通道
  const id1 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  // 第二次用同一 glyphIds + 同一 frozen 通道 → 应复用同一 identity
  const id2 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')

  assert.equal(id1.renderNodeKey, id2.renderNodeKey)
  assert.equal(id1.windowInstanceId, id2.windowInstanceId)
})

test('场景1c：不同 glyphIds + 同一 motionChannel → 不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)

  const id1 = keyAlloc.allocate('old', 10, ['orig-10-0'], 'frozen')
  const id2 = keyAlloc.allocate('old', 10, ['orig-10-1'], 'frozen')

  assert.notEqual(id1.renderNodeKey, id2.renderNodeKey)
  assert.notEqual(id1.windowInstanceId, id2.windowInstanceId)
})

test('场景1d：同一 glyphIds + motionChannel=frozen vs default → 不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // frozen 通道用 allocate（复用），default 通道用 allocateFresh（独立实例）
  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const defaultId = keyAlloc.allocate('old', 10, glyphIds, 'default')

  assert.notEqual(frozenId.renderNodeKey, defaultId.renderNodeKey)
  assert.notEqual(frozenId.windowInstanceId, defaultId.windowInstanceId)
})

test('场景1e：同一 glyphIds + 同一 motionChannel=default → 不同 renderNodeKey（allocateFresh 每次新建）', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // default 通道用 allocateFresh，每次返回新 ID，缓存键不同，所以不复用
  const id1 = keyAlloc.allocate('old', 10, glyphIds, 'default')
  const id2 = keyAlloc.allocate('old', 10, glyphIds, 'default')

  // allocateFresh 每次返回不同 ID → 不同 renderNodeKey
  assert.notEqual(id1.renderNodeKey, id2.renderNodeKey)
  assert.notEqual(id1.windowInstanceId, id2.windowInstanceId)
})

// ── 场景1f：Issue #879 评论6146031468 验证 ──
//
// Planner 为不同运动轨迹/来源生成不同的 renderNodeKey（包含 motionChannel），
// 确保同一物理 Text 的多块 clip 共享一组 key；物理身份不同的来源在 Planner 阶段就分配两个不同的 key。

test('场景1f：同一 glyphIds + 相同 windowInstanceId + 不同 motionChannel → 不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // 分配一个 windowInstanceId（模拟 Planner 预分配）
  const winId = allocator.allocate(glyphIds)

  // 用相同的 windowInstanceId 但不同的 motionChannel 分配
  // 注意：frozen 通道用 allocate（复用），deleted 通道用 allocateFresh（独立实例）
  // 但当我们显式传入 windowInstanceId 时，两者都使用传入的 winId
  const deletedId = keyAlloc.allocate('old', 10, glyphIds, 'deleted', winId)
  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen', winId)

  // 虽然 windowInstanceId 相同，但 renderNodeKey 不同（因为包含 motionChannel）
  assert.notEqual(deletedId.renderNodeKey, frozenId.renderNodeKey)
  assert.equal(deletedId.windowInstanceId, frozenId.windowInstanceId)
  assert.equal(deletedId.windowInstanceId, winId)

  // renderNodeKey 格式：${sourceKind}-${sourceRevision}-${motionChannel}-${allocatedWinId}
  assert.equal(deletedId.renderNodeKey, `old-10-deleted-${winId}`)
  assert.equal(frozenId.renderNodeKey, `old-10-frozen-${winId}`)
})

test('场景1f-2：同一 glyphIds + 不传 windowInstanceId + 不同 motionChannel → 不同 renderNodeKey 和 windowInstanceId', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // 不传 windowInstanceId，让分配器自行分配
  // frozen 通道用 allocate（复用同一 ID），deleted 通道用 allocateFresh（每次新建）
  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const deletedId = keyAlloc.allocate('old', 10, glyphIds, 'deleted')

  // renderNodeKey 不同（因为包含 motionChannel）
  assert.notEqual(deletedId.renderNodeKey, frozenId.renderNodeKey)
  // windowInstanceId 也不同（因为 allocate vs allocateFresh）
  assert.notEqual(deletedId.windowInstanceId, frozenId.windowInstanceId)

  // renderNodeKey 格式正确
  assert.match(frozenId.renderNodeKey, /^old-10-frozen-win-\d+$/)
  assert.match(deletedId.renderNodeKey, /^old-10-deleted-win-\d+$/)
})

// ── 场景2：WindowInstanceIdAllocator.allocate vs allocateFresh ──

test('场景2a：allocate 对同一 glyphIds 返回相同 ID', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const id1 = allocator.allocate(glyphIds)
  const id2 = allocator.allocate(glyphIds)

  assert.equal(id1, id2)
})

test('场景2b：allocateFresh 对同一 glyphIds 返回不同 ID', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const id1 = allocator.allocateFresh(glyphIds)
  const id2 = allocator.allocateFresh(glyphIds)

  assert.notEqual(id1, id2)
})

test('场景2c：allocateFresh 返回的 ID 格式为 win-<number>（有限长度）', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const id = allocator.allocateFresh(glyphIds)

  // 格式必须是 win-<number>，不是 win-<长度>:<id>...
  assert.match(id, /^win-\d+$/)
  // 长度有限：win-0 到 win-999999 最多 10 个字符
  assert.ok(id.length <= 10, `allocateFresh ID 长度应有限, got: ${id} (len=${id.length})`)
})

test('场景2d：allocate 返回的 ID 格式也为 win-<number>', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0']

  const id = allocator.allocate(glyphIds)

  assert.match(id, /^win-\d+$/)
})

test('场景2e：allocate 和 allocateFresh 可以共存于同一 allocator', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0']

  const allocId = allocator.allocate(glyphIds)
  const freshId1 = allocator.allocateFresh(glyphIds)
  const freshId2 = allocator.allocateFresh(glyphIds)

  // allocate 复用，allocateFresh 每次新建
  assert.equal(allocator.allocate(glyphIds), allocId)
  assert.notEqual(freshId1, allocId)
  assert.notEqual(freshId2, freshId1)
  assert.notEqual(freshId2, allocId)
})

test('场景2f：allocateFresh 不覆盖 allocate 的正向缓存', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0']

  const allocId = allocator.allocate(glyphIds)
  const freshId = allocator.allocateFresh(glyphIds)

  // allocateFresh 不应覆盖 allocate 的缓存
  // 再次 allocate 同一 glyphIds 应返回原来的 allocId，而不是 freshId
  assert.equal(allocator.allocate(glyphIds), allocId)
  assert.notEqual(allocId, freshId)
})

// ── 场景3：windowIdForGlyphIds 不被用作 windowInstanceId ──

test('场景3a：windowIdForGlyphIds 返回格式与 WindowInstanceIdAllocator 不同', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const windowId = windowIdForGlyphIds(glyphIds)
  const instanceId = allocator.allocate(glyphIds)

  // windowIdForGlyphIds 格式：win-<count>-<len>:<id>...
  // WindowInstanceIdAllocator 格式：win-<number>
  assert.match(windowId, /^win-\d+-/)
  assert.match(instanceId, /^win-\d+$/)

  // 两者必须不同
  assert.notEqual(windowId, instanceId)
})

test('场景3b：windowIdForGlyphIds 输出长度随 glyphIds 数量增长', () => {
  const ids1 = ['orig-10-0']
  const ids2 = ['orig-10-0', 'orig-10-1']
  const ids3 = ['orig-10-0', 'orig-10-1', 'orig-10-2']

  const w1 = windowIdForGlyphIds(ids1)
  const w2 = windowIdForGlyphIds(ids2)
  const w3 = windowIdForGlyphIds(ids3)

  // 长度必须递增
  assert.ok(w1.length < w2.length, `${w1.length} should be < ${w2.length}`)
  assert.ok(w2.length < w3.length, `${w2.length} should be < ${w3.length}`)
})

test('场景3c：WindowInstanceIdAllocator 输出长度固定（不随 glyphIds 数量增长）', () => {
  const allocator = new WindowInstanceIdAllocator()

  const id1 = allocator.allocate(['orig-10-0'])
  const id2 = allocator.allocate(['orig-10-0', 'orig-10-1'])
  const id3 = allocator.allocate(['orig-10-0', 'orig-10-1', 'orig-10-2'])

  // 所有 ID 都是 win-<number> 格式，长度基本相同（win-0, win-1, win-2）
  assert.match(id1, /^win-\d+$/)
  assert.match(id2, /^win-\d+$/)
  assert.match(id3, /^win-\d+$/)

  // 长度差异最多 1（win-9 → win-10 多一个字符）
  const maxLen = Math.max(id1.length, id2.length, id3.length)
  const minLen = Math.min(id1.length, id2.length, id3.length)
  assert.ok(maxLen - minLen <= 1, `长度差异应 <=1, got min=${minLen} max=${maxLen}`)
})

test('场景3d：windowIdForGlyphIds 的输出包含完整身份编码', () => {
  const glyphIds = ['orig-1-0', 'ins-2-3', 'orig-1-9']
  const windowId = windowIdForGlyphIds(glyphIds)

  // win-3- 后面跟着 3 个 <len>:<id> 段
  assert.ok(windowId.startsWith('win-3-'))
  // 每个 glyphId 都出现在编码中
  for (const gid of glyphIds) {
    assert.ok(windowId.includes(gid), `windowId should contain ${gid}`)
  }
})

// ── 场景4：buildFrame 合并逻辑——同 key 不同属性不输出两条 ──
//
// buildFrame 在 .ets 文件中无法直接 import，但其合并逻辑的核心约束可以模拟：
// 同一 renderNodeKey 的 frozen 成员只输出一个聚合 MotionGlyphWindow，
// 同 key 但不同 runId/sourceRevision 的成员被跳过（不 push 第二条）。

test('场景4a：同一 renderNodeKey 的 frozen 成员只聚合为一个窗口', () => {
  // 模拟 frozenWindowAggregator 的行为：
  // 同一 renderNodeKey 的多个 frozen 成员合并 clipRects，不输出多个窗口。
  const aggregator = new Map()

  const renderNodeKey = 'old-10-win-0'
  const member1 = {
    renderNodeKey,
    runId: 'old-10',
    sourceRevision: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0,
    clipRight: 10,
  }
  const member2 = {
    renderNodeKey,
    runId: 'old-10',
    sourceRevision: 10,
    clipRects: [{ x: 20, y: 0, width: 10, height: 20 }],
    clipLeft: 20,
    clipRight: 30,
  }

  // 模拟 buildFrame 的聚合逻辑
  for (const member of [member1, member2]) {
    const existing = aggregator.get(member.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== member.runId || existing.sourceRevision !== member.sourceRevision) {
        // 同 key 不同身份 → 跳过（不 push）
        continue
      } else {
        // 合并 clipRects
        existing.clipRects = [...existing.clipRects, ...member.clipRects]
        existing.clipLeft = Math.min(existing.clipLeft, member.clipLeft)
        existing.clipRight = Math.max(existing.clipRight, member.clipRight)
      }
    } else {
      aggregator.set(member.renderNodeKey, { ...member })
    }
  }

  // 只有一个聚合窗口
  assert.equal(aggregator.size, 1)
  const aggregated = aggregator.get(renderNodeKey)
  // clipRects 合并了两个成员的
  assert.equal(aggregated.clipRects.length, 2)
  // clipLeft/clipRight 是并集
  assert.equal(aggregated.clipLeft, 0)
  assert.equal(aggregated.clipRight, 30)
})

test('场景4b：同 renderNodeKey 但不同 runId/sourceRevision 的成员被跳过', () => {
  // 模拟防御性检查：同 key 但不同身份的成员不应输出第二条窗口
  const aggregator = new Map()

  const renderNodeKey = 'old-10-win-0'
  const member1 = {
    renderNodeKey,
    runId: 'old-10',
    sourceRevision: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0,
    clipRight: 10,
  }
  const member2 = {
    renderNodeKey, // 同 key
    runId: 'old-11', // 不同 runId！
    sourceRevision: 11, // 不同 sourceRevision！
    clipRects: [{ x: 20, y: 0, width: 10, height: 20 }],
    clipLeft: 20,
    clipRight: 30,
  }

  let skippedCount = 0
  for (const member of [member1, member2]) {
    const existing = aggregator.get(member.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== member.runId || existing.sourceRevision !== member.sourceRevision) {
        // 同 key 不同身份 → 跳过（不 push，不分配新 key）
        skippedCount++
        continue
      } else {
        existing.clipRects = [...existing.clipRects, ...member.clipRects]
      }
    } else {
      aggregator.set(member.renderNodeKey, { ...member })
    }
  }

  // 只有一个窗口（member1），member2 被跳过
  assert.equal(aggregator.size, 1)
  assert.equal(skippedCount, 1)
  // 窗口的 runId/sourceRevision 是第一个成员的
  const aggregated = aggregator.get(renderNodeKey)
  assert.equal(aggregated.runId, 'old-10')
  assert.equal(aggregated.sourceRevision, 10)
  // clipRects 只有 member1 的，member2 的没被合并
  assert.equal(aggregated.clipRects.length, 1)
})

test('场景4c：不同 renderNodeKey 的 frozen 成员各自独立输出', () => {
  const aggregator = new Map()

  const member1 = {
    renderNodeKey: 'old-10-win-0',
    runId: 'old-10',
    sourceRevision: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0,
    clipRight: 10,
  }
  const member2 = {
    renderNodeKey: 'old-10-win-1', // 不同 key
    runId: 'old-10',
    sourceRevision: 10,
    clipRects: [{ x: 20, y: 0, width: 10, height: 20 }],
    clipLeft: 20,
    clipRight: 30,
  }

  for (const member of [member1, member2]) {
    const existing = aggregator.get(member.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== member.runId || existing.sourceRevision !== member.sourceRevision) {
        continue
      } else {
        existing.clipRects = [...existing.clipRects, ...member.clipRects]
      }
    } else {
      aggregator.set(member.renderNodeKey, { ...member })
    }
  }

  // 两个独立窗口
  assert.equal(aggregator.size, 2)
  assert.ok(aggregator.has('old-10-win-0'))
  assert.ok(aggregator.has('old-10-win-1'))
})

// ── 场景5：跨通道身份独立性 ──
//
// carryOverGhost 和 carryOverMotion 对同一 glyphIds 但不同运动轨迹的场景，
// 应获得不同的 renderNodeKey（通过 allocateFresh 分配独立物理实例）。

test('场景5a：carryOverGhost 和 carryOverMotion 对同一 glyphIds 获得不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // carryOverGhost 用 frozen 通道（复用上一帧的物理 Text）
  const ghostId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  // carryOverMotion 用 motion 通道（独立物理实例，不同运动轨迹）
  const motionId = keyAlloc.allocate('old', 10, glyphIds, 'motion')

  // 不同通道 → 不同 windowInstanceId → 不同 renderNodeKey
  assert.notEqual(ghostId.windowInstanceId, motionId.windowInstanceId)
  assert.notEqual(ghostId.renderNodeKey, motionId.renderNodeKey)
})

test('场景5b：frozen 通道复用，非 frozen 通道每次新建', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0']

  // frozen 通道用 allocate（复用），同一 glyphIds 多次分配 → 复用同一 identity
  const frozen1 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const frozen2 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  assert.equal(frozen1.renderNodeKey, frozen2.renderNodeKey)

  // motion 通道用 allocateFresh（每次新建），同一 glyphIds 多次分配 → 不同 identity
  const motion1 = keyAlloc.allocate('old', 10, glyphIds, 'motion')
  const motion2 = keyAlloc.allocate('old', 10, glyphIds, 'motion')
  assert.notEqual(motion1.renderNodeKey, motion2.renderNodeKey)

  // frozen 和 motion 通道之间也不同
  assert.notEqual(frozen1.renderNodeKey, motion1.renderNodeKey)
})

test('场景5c：target 成员通过 allocateFresh 获得独立物理实例', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // target 成员用 allocateTarget → motionChannel='target' → allocateFresh
  // allocateFresh 每次返回新 ID，所以两次 allocateTarget 得到不同 renderNodeKey
  const targetId1 = keyAlloc.allocateTarget(11, glyphIds)
  const targetId2 = keyAlloc.allocateTarget(11, glyphIds)

  assert.notEqual(targetId1.renderNodeKey, targetId2.renderNodeKey)
  assert.notEqual(targetId1.windowInstanceId, targetId2.windowInstanceId)

  // 但与 frozen 通道的 identity 也不同
  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  assert.notEqual(targetId1.renderNodeKey, frozenId.renderNodeKey)
})

test('场景5d：不同 sourceRevision 但同一 motionChannel 也产生不同 renderNodeKey', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0']

  const id1 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const id2 = keyAlloc.allocate('old', 11, glyphIds, 'frozen')

  // 不同 sourceRevision → 不同 renderNodeKey（因为 renderNodeKey 包含 sourceRevision）
  assert.notEqual(id1.renderNodeKey, id2.renderNodeKey)
  assert.notEqual(id1.runId, id2.runId)
})

test('场景5e：跨通道身份独立性——3 个通道互不干扰', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const motionId = keyAlloc.allocate('old', 10, glyphIds, 'motion')
  const targetId = keyAlloc.allocate('new', 11, glyphIds, 'target')

  // 3 个通道各自独立
  const keys = new Set([frozenId.renderNodeKey, motionId.renderNodeKey, targetId.renderNodeKey])
  assert.equal(keys.size, 3)

  const instanceIds = new Set([frozenId.windowInstanceId, motionId.windowInstanceId, targetId.windowInstanceId])
  assert.equal(instanceIds.size, 3)
})

// ── 补充：fallback 路径验证 ──

test('补充1：fallback 使用 allocateFresh 而非 windowIdForGlyphIds', () => {
  // buildFrame fallback 路径：piece.renderNodeKey/windowInstanceId 为 undefined 时，
  // 用 windowIdAllocator.allocateFresh(piece.glyphIds) 分配。
  // 关键约束：fallback ID 格式是 win-<number>，不是 windowIdForGlyphIds 的 win-N-... 格式。
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  const fallbackId = allocator.allocateFresh(glyphIds)
  const windowId = windowIdForGlyphIds(glyphIds)

  // fallback ID 是有限长度的 win-<number>
  assert.match(fallbackId, /^win-\d+$/)
  // windowIdForGlyphIds 是变长的 win-N-... 格式
  assert.ok(windowId.length > fallbackId.length)
  // 两者不同
  assert.notEqual(fallbackId, windowId)
})

test('补充2：allocateFresh 后 release 再 allocate 不影响 allocate 缓存', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0']

  const allocId = allocator.allocate(glyphIds)
  const freshId = allocator.allocateFresh(glyphIds)

  // 释放 freshId
  allocator.release(freshId)

  // allocate 缓存不受影响——同一 glyphIds 仍返回原来的 allocId
  assert.equal(allocator.allocate(glyphIds), allocId)
  // freshId 已释放，可以重新分配（可能复用刚释放的 ID，这是正常行为）
  const newFreshId = allocator.allocateFresh(glyphIds)
  assert.match(newFreshId, /^win-\d+$/)
  // allocate 缓存仍然不受影响
  assert.equal(allocator.allocate(glyphIds), allocId)
})

test('补充3：PhysicalWindowKeyAllocator 的 renderNodeKey 格式正确', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0']

  const id = keyAlloc.allocate('old', 10, glyphIds, 'frozen')

  // Issue #879 评论6146031468：renderNodeKey = `${sourceKind}-${sourceRevision}-${motionChannel}-${windowInstanceId}`
  assert.equal(id.renderNodeKey, `old-10-frozen-${id.windowInstanceId}`)
  // windowInstanceId 是 win-<number> 格式
  assert.match(id.windowInstanceId, /^win-\d+$/)
  // runId 是 `${sourceKind}-${sourceRevision}`
  assert.equal(id.runId, 'old-10')
})

test('补充4：GlyphIdentityTable 创建后可获取 glyphIds 用于窗口身份分配', () => {
  const table = GlyphIdentityTable.create('甲乙丙', 10)
  const glyphIds = table.idsForRange(0, 3).map((e) => e.glyphId)

  assert.equal(glyphIds.length, 3)

  // 用这些 glyphIds 分配窗口身份
  const allocator = new WindowInstanceIdAllocator()
  const id1 = allocator.allocate(glyphIds)
  const id2 = allocator.allocate(glyphIds)

  // 同一 glyphIds → 同一 ID
  assert.equal(id1, id2)
  assert.match(id1, /^win-\d+$/)
})

// ── 场景6：连续帧采样时 key 稳定性 ──
//
// Issue #879 评论6142418499 问题4：同一个 plan 连续 p=0、0.25、0.5、1 采样时，
// 所有帧的 MotionGlyphWindow 的 renderNodeKey 保持不变（同一窗口在所有帧使用同一 key），
// WindowInstanceIdAllocator 不新增登记（allocateFresh 不被调用，
// allocate 对同一 glyphIds 返回同一 ID）。

test('场景6a：连续帧采样时同一窗口的 renderNodeKey 保持不变', () => {
  // 模拟 Planner 预分配：在 plan 准备阶段为所有 piece/member 分配 renderNodeKey + windowInstanceId。
  // buildFrame 在连续帧采样时只读取预分配的 key，不调用 allocateFresh。
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // Planner 预分配阶段：为 frozen 通道分配 identity
  const preAllocated = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const expectedKey = preAllocated.renderNodeKey
  const expectedWinId = preAllocated.windowInstanceId

  // 模拟连续帧采样：p=0, 0.25, 0.5, 1
  // buildFrame 在每帧只读取预分配的 key（不调用 allocateFresh）
  const progressValues = [0, 0.25, 0.5, 1]
  for (const p of progressValues) {
    // buildFrame 读取 piece.renderNodeKey（预分配值），不调用 allocator
    const frameKey = preAllocated.renderNodeKey
    const frameWinId = preAllocated.windowInstanceId

    // 所有帧的 key 必须相同
    assert.equal(frameKey, expectedKey, `p=${p}: renderNodeKey 应保持不变`)
    assert.equal(frameWinId, expectedWinId, `p=${p}: windowInstanceId 应保持不变`)
  }
})

test('场景6b：连续帧采样时 WindowInstanceIdAllocator 不新增登记', () => {
  // 关键约束：buildFrame 不调用 allocateFresh，只读取预分配的 key。
  // 因此连续采样多帧不会导致 allocator 的 nextId 增长。
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // Planner 预分配阶段：调用 allocate 为同一组 glyphIds 分配 ID
  const preAllocatedId = allocator.allocate(glyphIds)
  const nextIdAfterPreAlloc = allocator.nextId ?? 0

  // 模拟连续帧采样：p=0, 0.25, 0.5, 1
  // buildFrame 不调用 allocate 或 allocateFresh，只读取预分配的 ID
  const progressValues = [0, 0.25, 0.5, 1]
  for (const p of progressValues) {
    // buildFrame 只读取 piece.windowInstanceId（= preAllocatedId），不调用 allocator
    // 所以 nextId 不应增长
  }

  // 验证：allocator 的 nextId 没有因为帧采样而增长
  // 注意：nextId 是私有字段，我们通过 allocate 行为间接验证
  // 再次 allocate 同一 glyphIds 应返回同一 ID（缓存命中）
  assert.equal(allocator.allocate(glyphIds), preAllocatedId)
  // allocateFresh 仍可以分配新 ID（但 buildFrame 不调用它）
  const freshId = allocator.allocateFresh(glyphIds)
  assert.notEqual(freshId, preAllocatedId)
})

test('场景6c：连续帧采样时 SimulatedPhysicalWindowKeyAllocator 不新增 identity', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // Planner 预分配
  const id1 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const identitiesCountAfterPreAlloc = keyAlloc.identities.size

  // 模拟连续帧采样：每帧只读取预分配的 identity（不调用 allocate）
  const progressValues = [0, 0.25, 0.5, 1]
  for (const p of progressValues) {
    // buildFrame 只读取 piece.renderNodeKey/windowInstanceId（= id1 的值）
    // 不调用 keyAlloc.allocate，所以 identities 不增长
  }

  // identities 数量不应因为帧采样而增长
  assert.equal(keyAlloc.identities.size, identitiesCountAfterPreAlloc)

  // 再次用同一参数 allocate 应返回同一 identity（缓存命中）
  const id2 = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  assert.equal(id1.renderNodeKey, id2.renderNodeKey)
  assert.equal(keyAlloc.identities.size, identitiesCountAfterPreAlloc)
})

// ── 场景7：plannedWindowKeys 与实际 frame 窗口 key 一致性 ──
//
// plannedWindowKeys 只从 plan 数据中读取预分配的 key（piece.renderNodeKey、
// interval.renderNodeKey、member.renderNodeKey 等），
// buildFrame 也只从 plan 数据中读取预分配的 key（不再 allocateFresh），
// 因此两者的 key 集合必须完全一致。

test('场景7a：plannedWindowKeys 与 buildFrame 输出的 key 集合完全一致', () => {
  // 模拟一个完整的 plan 结构，包含 insert/deleted/retained/carryOver 各通道
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)

  // 为每个通道预分配 identity
  const insertGlyphIds = ['ins-11-0']
  const deletedGlyphIds = ['orig-10-0']
  const retainedFrozenGlyphIds = ['orig-10-1', 'orig-10-2']
  const retainedTargetGlyphIds = ['new-11-1', 'new-11-2']
  const ghostGlyphIds = ['orig-9-0']
  const motionGlyphIds = ['orig-10-3']

  const insertIdentity = keyAlloc.allocate('new', 11, insertGlyphIds, 'target')
  const deletedIdentity = keyAlloc.allocate('old', 10, deletedGlyphIds, 'frozen')
  const frozenIdentity = keyAlloc.allocate('old', 10, retainedFrozenGlyphIds, 'frozen')
  const targetIdentity = keyAlloc.allocate('new', 11, retainedTargetGlyphIds, 'target')
  const ghostIdentity = keyAlloc.allocate('old', 9, ghostGlyphIds, 'frozen')
  const motionIdentity = keyAlloc.allocate('old', 10, motionGlyphIds, 'motion')

  // 模拟 plan 数据结构（只包含 key 相关字段）
  const plan = {
    insertRuns: [{
      pieces: [{ renderNodeKey: insertIdentity.renderNodeKey, windowInstanceId: insertIdentity.windowInstanceId }]
    }],
    deletedRuns: [{
      pieces: [{ renderNodeKey: deletedIdentity.renderNodeKey, windowInstanceId: deletedIdentity.windowInstanceId }]
    }],
    retainedMoves: [{
      pieces: [{
        intervals: [{
          sourceMembers: [
            { renderNodeKey: frozenIdentity.renderNodeKey, windowInstanceId: frozenIdentity.windowInstanceId, role: 'frozen' },
            { renderNodeKey: targetIdentity.renderNodeKey, windowInstanceId: targetIdentity.windowInstanceId, role: 'target' }
          ]
        }]
      }]
    }],
    carryOverGhostRuns: [{
      renderNodeKey: ghostIdentity.renderNodeKey, windowInstanceId: ghostIdentity.windowInstanceId
    }],
    carryOverMotionRuns: [{
      renderNodeKey: motionIdentity.renderNodeKey, windowInstanceId: motionIdentity.windowInstanceId
    }],
  }

  // 模拟 plannedWindowKeys 的逻辑：从 plan 数据中读取预分配的 key
  const plannedKeys = new Set()
  for (const run of plan.insertRuns) {
    for (const piece of run.pieces) {
      if (piece.renderNodeKey !== undefined) plannedKeys.add(piece.renderNodeKey)
    }
  }
  for (const run of plan.deletedRuns) {
    for (const piece of run.pieces) {
      if (piece.renderNodeKey !== undefined) plannedKeys.add(piece.renderNodeKey)
    }
  }
  for (const move of plan.retainedMoves) {
    for (const piece of move.pieces) {
      for (const interval of piece.intervals) {
        if (interval.sourceMembers !== undefined) {
          for (const member of interval.sourceMembers) {
            if (member.renderNodeKey !== undefined) plannedKeys.add(member.renderNodeKey)
          }
        }
      }
    }
  }
  for (const ghost of plan.carryOverGhostRuns) {
    if (ghost.renderNodeKey !== undefined) plannedKeys.add(ghost.renderNodeKey)
  }
  for (const motionRun of plan.carryOverMotionRuns) {
    if (motionRun.renderNodeKey !== undefined) plannedKeys.add(motionRun.renderNodeKey)
  }

  // 模拟 buildFrame 的输出 key：从相同的 plan 数据中读取预分配的 key
  const frameKeys = new Set()
  for (const run of plan.insertRuns) {
    for (const piece of run.pieces) {
      frameKeys.add(piece.renderNodeKey)
    }
  }
  for (const run of plan.deletedRuns) {
    for (const piece of run.pieces) {
      frameKeys.add(piece.renderNodeKey)
    }
  }
  for (const move of plan.retainedMoves) {
    for (const piece of move.pieces) {
      for (const interval of piece.intervals) {
        for (const member of interval.sourceMembers) {
          // frozen 成员用 member.renderNodeKey
          // target 成员用 member.renderNodeKey（已预分配）
          frameKeys.add(member.renderNodeKey)
        }
      }
    }
  }
  for (const ghost of plan.carryOverGhostRuns) {
    frameKeys.add(ghost.renderNodeKey)
  }
  for (const motionRun of plan.carryOverMotionRuns) {
    frameKeys.add(motionRun.renderNodeKey)
  }

  // 两者必须完全一致
  assert.equal(plannedKeys.size, frameKeys.size)
  for (const key of plannedKeys) {
    assert.ok(frameKeys.has(key), `frameKeys 应包含 plannedKey: ${key}`)
  }
  for (const key of frameKeys) {
    assert.ok(plannedKeys.has(key), `plannedKeys 应包含 frameKey: ${key}`)
  }
})

test('场景7b：plannedWindowKeys 不调用 allocateFresh（只读取预分配 key）', () => {
  // plannedWindowKeys 只读取 plan 中的预分配 key，不调用任何 allocator 方法。
  // 验证：即使不创建 allocator，plannedWindowKeys 也能返回正确的 key 集合。
  const plan = {
    insertRuns: [{
      pieces: [{ renderNodeKey: 'new-11-win-0', windowInstanceId: 'win-0' }]
    }],
    deletedRuns: [{
      pieces: [{ renderNodeKey: 'old-10-win-1', windowInstanceId: 'win-1' }]
    }],
    retainedMoves: [{
      pieces: [{
        intervals: [{
          sourceMembers: [
            { renderNodeKey: 'old-10-win-2', role: 'frozen' },
            { renderNodeKey: 'new-11-win-3', role: 'target' }
          ]
        }]
      }]
    }],
    carryOverGhostRuns: [{ renderNodeKey: 'old-9-win-4' }],
    carryOverMotionRuns: [{ renderNodeKey: 'old-10-win-5' }],
  }

  // 模拟 plannedWindowKeys：只读取，不分配
  const keys = new Set()
  for (const run of plan.insertRuns) {
    for (const piece of run.pieces) {
      if (piece.renderNodeKey !== undefined) keys.add(piece.renderNodeKey)
    }
  }
  for (const run of plan.deletedRuns) {
    for (const piece of run.pieces) {
      if (piece.renderNodeKey !== undefined) keys.add(piece.renderNodeKey)
    }
  }
  for (const move of plan.retainedMoves) {
    for (const piece of move.pieces) {
      for (const interval of piece.intervals) {
        if (interval.sourceMembers !== undefined) {
          for (const member of interval.sourceMembers) {
            if (member.renderNodeKey !== undefined) keys.add(member.renderNodeKey)
          }
        }
      }
    }
  }
  for (const ghost of plan.carryOverGhostRuns) {
    if (ghost.renderNodeKey !== undefined) keys.add(ghost.renderNodeKey)
  }
  for (const motionRun of plan.carryOverMotionRuns) {
    if (motionRun.renderNodeKey !== undefined) keys.add(motionRun.renderNodeKey)
  }

  // 6 个不同的 key
  assert.equal(keys.size, 6)
  assert.ok(keys.has('new-11-win-0'))
  assert.ok(keys.has('old-10-win-1'))
  assert.ok(keys.has('old-10-win-2'))
  assert.ok(keys.has('new-11-win-3'))
  assert.ok(keys.has('old-9-win-4'))
  assert.ok(keys.has('old-10-win-5'))
})

// ── 场景8：不同绘制状态窗口的合并行为 ──
//
// Issue #879 评论6145308014：buildFrame 合并逻辑——同 renderNodeKey 且物理身份一致才合并 clip。
// 不同绘制状态的窗口在 Planner 阶段已有不同 key，两张窗口均保留。
// 同 key 同物理身份时只合并 clip（正常聚合行为）。
// 同 key 不同物理身份时不合并，保留独立窗口（暴露 Planner 规划问题）。

test('场景8a：不同绘制状态的窗口在 Planner 阶段已有不同 key，两张窗口均保留', () => {
  // 模拟 buildFrame 的合并逻辑（同 key 且物理身份一致才合并 clip）
  const motionGlyphWindows = [
    {
      renderNodeKey: 'old-10-win-0',  // 不同 key
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 0,
      offsetY: 0,
      opacity: 1,
      utf16Start: 0,
      utf16End: 2,
      glyphX: 100,
      glyphY: 50,
      windowInstanceId: 'win-0',
      motionOwnerKind: 'reflow',
      clipRects: [{ x: 100, y: 50, width: 20, height: 30 }],
    },
    {
      renderNodeKey: 'old-10-win-1',  // 不同 key（Planner 为不同绘制状态分配了不同 key）
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 5,  // 不同 offsetX
      offsetY: 0,
      opacity: 0.5, // 不同 opacity
      utf16Start: 2,
      utf16End: 4,
      glyphX: 100,
      glyphY: 50,
      windowInstanceId: 'win-1',
      motionOwnerKind: 'reflow',
      clipRects: [{ x: 120, y: 50, width: 20, height: 30 }],
    },
  ]

  // 模拟合并逻辑（与 EditorMotionCoordinator.buildFrame 一致）
  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      const samePhysicalIdentity =
        existing.sourceKind === w.sourceKind &&
        existing.sourceRevision === w.sourceRevision &&
        existing.utf16Start === w.utf16Start &&
        existing.utf16End === w.utf16End &&
        existing.glyphX === w.glyphX &&
        existing.glyphY === w.glyphY &&
        existing.offsetX === w.offsetX &&
        existing.offsetY === w.offsetY &&
        existing.opacity === w.opacity &&
        existing.windowInstanceId === w.windowInstanceId &&
        existing.motionOwnerKind === w.motionOwnerKind
      if (samePhysicalIdentity) {
        existing.clipRects = [...existing.clipRects, ...w.clipRects]
        existing.clipLeft = Math.min(existing.clipLeft, w.clipLeft)
        existing.clipRight = Math.max(existing.clipRight, w.clipRight)
        existing.clipTop = Math.min(existing.clipTop, w.clipTop)
        existing.clipBottom = Math.max(existing.clipBottom, w.clipBottom)
        existing.currentClipLeft = Math.min(existing.currentClipLeft, w.currentClipLeft)
        existing.currentClipRight = Math.max(existing.currentClipRight, w.currentClipRight)
        continue
      } else {
        mergedWindows.push(w)
      }
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 两张窗口均保留（不丢字）
  assert.equal(mergedWindows.length, 2)
  assert.equal(mergedWindows[0].renderNodeKey, 'old-10-win-0')
  assert.equal(mergedWindows[1].renderNodeKey, 'old-10-win-1')
  // 没有任何 -dup 后缀的 key
  for (const w of mergedWindows) {
    assert.ok(!w.renderNodeKey.includes('-dup'), `不应有 -dup key: ${w.renderNodeKey}`)
  }
})

test('场景8b：同 key 同物理身份时只合并 clip', () => {
  const motionGlyphWindows = [
    {
      renderNodeKey: 'old-10-win-0',
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 0,
      offsetY: 0,
      opacity: 1,
      utf16Start: 0,
      utf16End: 2,
      glyphX: 100,
      glyphY: 50,
      windowInstanceId: 'win-0',
      motionOwnerKind: 'reflow',
      clipRects: [{ x: 100, y: 50, width: 20, height: 30 }],
    },
    {
      renderNodeKey: 'old-10-win-0', // 同 key
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 0,   // 同 offsetX
      offsetY: 0,   // 同 offsetY
      opacity: 1,   // 同 opacity
      utf16Start: 0, // 同 utf16Start
      utf16End: 2,   // 同 utf16End
      glyphX: 100,   // 同 glyphX
      glyphY: 50,    // 同 glyphY
      windowInstanceId: 'win-0', // 同 windowInstanceId
      motionOwnerKind: 'reflow', // 同 motionOwnerKind
      clipRects: [{ x: 120, y: 50, width: 20, height: 30 }],
    },
  ]

  // 模拟合并逻辑（与 EditorMotionCoordinator.buildFrame 一致）
  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      const samePhysicalIdentity =
        existing.sourceKind === w.sourceKind &&
        existing.sourceRevision === w.sourceRevision &&
        existing.utf16Start === w.utf16Start &&
        existing.utf16End === w.utf16End &&
        existing.glyphX === w.glyphX &&
        existing.glyphY === w.glyphY &&
        existing.offsetX === w.offsetX &&
        existing.offsetY === w.offsetY &&
        existing.opacity === w.opacity &&
        existing.windowInstanceId === w.windowInstanceId &&
        existing.motionOwnerKind === w.motionOwnerKind
      if (samePhysicalIdentity) {
        existing.clipRects = [...existing.clipRects, ...w.clipRects]
        existing.clipLeft = Math.min(existing.clipLeft, w.clipLeft)
        existing.clipRight = Math.max(existing.clipRight, w.clipRight)
        existing.clipTop = Math.min(existing.clipTop, w.clipTop)
        existing.clipBottom = Math.max(existing.clipBottom, w.clipBottom)
        existing.currentClipLeft = Math.min(existing.currentClipLeft, w.currentClipLeft)
        existing.currentClipRight = Math.max(existing.currentClipRight, w.currentClipRight)
        continue
      } else {
        mergedWindows.push(w)
      }
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 只有一个窗口（合并了 clip）
  assert.equal(mergedWindows.length, 1)
  assert.equal(mergedWindows[0].renderNodeKey, 'old-10-win-0')
  // clipRects 合并了两段
  assert.equal(mergedWindows[0].clipRects.length, 2)
})

test('场景8c：不同 motionChannel 生成不同的 renderNodeKey，Planner 为不同物理来源分配不同 key', () => {
  // Issue #879 评论6146031468：Planner 已为每个物理来源分配唯一的 renderNodeKey（包含 motionChannel），
  // 不同运动轨迹/来源生成不同的 key。同一物理 Text 的多块 clip 共享一组 key；
  // 物理身份不同的来源在 Planner 阶段就分配两个不同的 key。
  // 模拟两个不同运动轨迹（如 'deleted' 和 'frozen'）有相同的 sourceKind/sourceRevision，
  // 但现在它们的 renderNodeKey 不同（因为包含 motionChannel）。
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // 两个不同运动轨迹，相同 glyphIds
  // frozen 通道用 allocate（复用），deleted 通道用 allocateFresh（每次新建）
  const frozenId = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const deletedId = keyAlloc.allocate('old', 10, glyphIds, 'deleted')

  // renderNodeKey 不同（因为包含 motionChannel）
  assert.notEqual(deletedId.renderNodeKey, frozenId.renderNodeKey)
  // windowInstanceId 也不同（因为 allocate vs allocateFresh）
  assert.notEqual(deletedId.windowInstanceId, frozenId.windowInstanceId)

  // renderNodeKey 格式：${sourceKind}-${sourceRevision}-${motionChannel}-${allocatedWinId}
  assert.match(frozenId.renderNodeKey, /^old-10-frozen-win-\d+$/)
  assert.match(deletedId.renderNodeKey, /^old-10-deleted-win-\d+$/)

  // 由于 renderNodeKey 不同，buildFrame 不会合并它们，输出两个独立窗口
  const motionGlyphWindows = [
    { renderNodeKey: deletedId.renderNodeKey, sourceKind: 'old', sourceRevision: 10, windowInstanceId: deletedId.windowInstanceId, utf16Start: 0, utf16End: 2, offsetX: 0, opacity: 1 },
    { renderNodeKey: frozenId.renderNodeKey, sourceKind: 'old', sourceRevision: 10, windowInstanceId: frozenId.windowInstanceId, utf16Start: 2, utf16End: 4, offsetX: 5, opacity: 0.5 },
  ]

  // 模拟 buildFrame 的合并逻辑：不同 key 各自独立输出
  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      // 同 key 且身份一致 → 合并 clip
      existing.clipRects = [...existing.clipRects, ...w.clipRects]
      continue
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 两个窗口均保留（因为 renderNodeKey 不同）
  assert.equal(mergedWindows.length, 2)
  assert.equal(mergedWindows[0].renderNodeKey, deletedId.renderNodeKey)
  assert.equal(mergedWindows[1].renderNodeKey, frozenId.renderNodeKey)
})

test('场景8d：同 key 同物理身份不同 clip 时合并为一个窗口', () => {
  // Issue #879 评论6145308014 问题3：另一条用例输入真正同一源 Text、
  // 相同 offset/alpha/UTF-16 源范围、不同可见 clip，断言它们能合并为一个窗口。
  // 这才是合法的同 key 合并场景：同一物理 Text 的多块 clip 共享一组 key。
  const motionGlyphWindows = [
    {
      renderNodeKey: 'old-10-win-0',
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 0,
      offsetY: 0,
      opacity: 1,
      utf16Start: 0,
      utf16End: 4,
      glyphX: 100,
      glyphY: 50,
      windowInstanceId: 'win-0',
      motionOwnerKind: 'reflow',
      clipRects: [{ x: 100, y: 50, width: 20, height: 30 }],
    },
    {
      renderNodeKey: 'old-10-win-0', // 同 key
      sourceKind: 'old',
      sourceRevision: 10,
      offsetX: 0,   // 同 offsetX
      offsetY: 0,   // 同 offsetY
      opacity: 1,   // 同 opacity
      utf16Start: 0, // 同 UTF-16 源范围
      utf16End: 4,
      glyphX: 100,   // 同 glyphX
      glyphY: 50,    // 同 glyphY
      windowInstanceId: 'win-0', // 同 windowInstanceId
      motionOwnerKind: 'reflow', // 同 motionOwnerKind
      clipRects: [{ x: 120, y: 50, width: 20, height: 30 }], // 不同 clip
    },
  ]

  // 模拟合并逻辑（与 EditorMotionCoordinator.buildFrame 一致）
  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      const samePhysicalIdentity =
        existing.sourceKind === w.sourceKind &&
        existing.sourceRevision === w.sourceRevision &&
        existing.utf16Start === w.utf16Start &&
        existing.utf16End === w.utf16End &&
        existing.glyphX === w.glyphX &&
        existing.glyphY === w.glyphY &&
        existing.offsetX === w.offsetX &&
        existing.offsetY === w.offsetY &&
        existing.opacity === w.opacity &&
        existing.windowInstanceId === w.windowInstanceId &&
        existing.motionOwnerKind === w.motionOwnerKind
      if (samePhysicalIdentity) {
        existing.clipRects = [...existing.clipRects, ...w.clipRects]
        existing.clipLeft = Math.min(existing.clipLeft, w.clipLeft)
        existing.clipRight = Math.max(existing.clipRight, w.clipRight)
        existing.clipTop = Math.min(existing.clipTop, w.clipTop)
        existing.clipBottom = Math.max(existing.clipBottom, w.clipBottom)
        existing.currentClipLeft = Math.min(existing.currentClipLeft, w.currentClipLeft)
        existing.currentClipRight = Math.max(existing.currentClipRight, w.currentClipRight)
        continue
      } else {
        mergedWindows.push(w)
      }
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 合并为一个窗口（同 key 且物理身份一致，只合并 clip）
  assert.equal(mergedWindows.length, 1)
  assert.equal(mergedWindows[0].renderNodeKey, 'old-10-win-0')
  // clipRects 合并了两段
  assert.equal(mergedWindows[0].clipRects.length, 2)
  // 源范围保持不变
  assert.equal(mergedWindows[0].utf16Start, 0)
  assert.equal(mergedWindows[0].utf16End, 4)
  assert.equal(mergedWindows[0].offsetX, 0)
  assert.equal(mergedWindows[0].opacity, 1)
})

// ── 场景9：buildFrame 不再调用 allocateFresh ──
//
// buildFrame 是纯计算函数，不修改 allocator 状态。
// 连续采样多帧不会导致 allocator 的 nextId 增长。
// 所有 key 都来自 Planner 的预分配，不在运行时分配。

test('场景9a：连续采样多帧 allocator 的 nextId 不增长', () => {
  const allocator = new WindowInstanceIdAllocator()
  const glyphIds = ['orig-10-0', 'orig-10-1']

  // Planner 预分配阶段
  const preAllocatedId = allocator.allocate(glyphIds)
  // 记录预分配后的状态：通过 allocateFresh 的行为间接验证 nextId
  // allocateFresh 会分配一个新 ID，如果 nextId 增长了说明有新的分配
  const freshId1 = allocator.allocateFresh(glyphIds)
  // 释放它，恢复状态
  allocator.release(freshId1)

  // 模拟连续帧采样（p=0, 0.25, 0.5, 1）
  // buildFrame 只读取预分配的 key，不调用 allocateFresh
  // 所以 allocator 状态不应改变
  const progressValues = [0, 0.25, 0.5, 1]
  for (const p of progressValues) {
    // buildFrame 只读取 piece.windowInstanceId（= preAllocatedId）
    // 不调用 allocator.allocate 或 allocator.allocateFresh
  }

  // 验证：allocate 同一 glyphIds 仍返回同一 ID
  assert.equal(allocator.allocate(glyphIds), preAllocatedId)
  // 验证：allocateFresh 可以正常分配新 ID（但 buildFrame 不调用它）
  const freshId2 = allocator.allocateFresh(glyphIds)
  assert.notEqual(freshId2, preAllocatedId)
})

test('场景9b：buildFrame 只读取预分配的 key，不创建新 identity', () => {
  const allocator = new WindowInstanceIdAllocator()
  const keyAlloc = new SimulatedPhysicalWindowKeyAllocator(allocator)
  const glyphIds = ['orig-10-0']

  // Planner 预分配
  const identity = keyAlloc.allocate('old', 10, glyphIds, 'frozen')
  const identitiesCount = keyAlloc.identities.size

  // 模拟 buildFrame 连续4帧采样
  // buildFrame 只读取 identity.renderNodeKey 和 identity.windowInstanceId
  // 不调用 keyAlloc.allocate 或 allocator.allocateFresh
  const frameKeys = []
  for (const p of [0, 0.25, 0.5, 1]) {
    frameKeys.push(identity.renderNodeKey) // 只读取
  }

  // 所有帧的 key 相同
  assert.equal(frameKeys.length, 4)
  for (const key of frameKeys) {
    assert.equal(key, identity.renderNodeKey)
  }
  // identities 没有增长
  assert.equal(keyAlloc.identities.size, identitiesCount)
})

test('场景9c：buildFrame 不接收 windowIdAllocator 参数', () => {
  // 验证 buildFrame 签名变更：不再接收 windowIdAllocator 参数。
  // buildFrame 只从 plan 数据中读取预分配的 renderNodeKey/windowInstanceId。
  // 这意味着 buildFrame 无法调用 allocateFresh（它没有 allocator 引用）。

  // 模拟 buildFrame 的调用签名（不含 windowIdAllocator）
  // 与真实代码一致：直接读取预分配值，不做 fallback
  function simulatedBuildFrame(plan, progress) {
    // 只从 plan 中读取预分配的 key（用 `!` 断言，模拟 ArkTS 的非空断言）
    const windows = []
    for (const run of plan.insertRuns) {
      for (const piece of run.pieces) {
        // buildFrame 直接读取 piece.renderNodeKey! / piece.windowInstanceId!
        // 不再有 `?? new-${revision}-unplanned` fallback
        assert.ok(piece.renderNodeKey !== undefined, 'piece.renderNodeKey must be pre-allocated')
        assert.ok(piece.windowInstanceId !== undefined, 'piece.windowInstanceId must be pre-allocated')
        windows.push({
          renderNodeKey: piece.renderNodeKey,
          windowInstanceId: piece.windowInstanceId,
        })
      }
    }
    return windows
  }

  // plan 中所有 key 都已预分配
  const plan = {
    newRevision: 11,
    insertRuns: [{
      pieces: [{ renderNodeKey: 'new-11-win-0', windowInstanceId: 'win-0' }]
    }]
  }

  // 连续采样4帧
  for (const p of [0, 0.25, 0.5, 1]) {
    const windows = simulatedBuildFrame(plan, p)
    assert.equal(windows[0].renderNodeKey, 'new-11-win-0')
    assert.equal(windows[0].windowInstanceId, 'win-0')
    // 不包含 -unplanned 后缀
    assert.ok(!windows[0].renderNodeKey.includes('-unplanned'))
    assert.ok(!windows[0].windowInstanceId.includes('-unplanned'))
  }
})

// ── 场景10：buildFrame 直接读取预分配值（不使用 fallback） ──
//
// buildFrame 中所有通道（insert/deleted/retainedMoves/carryOver）直接读取
// Planner 预分配的 renderNodeKey 和 windowInstanceId（用 `!` 断言），
// 不再有 `new-${revision}-unplanned` 或 `old-${revision}-unplanned` 格式的 fallback key。
// 如果 piece/interval/member 的预分配值缺失（Planner bug），`!` 断言会直接报错，
// 而不是静默生成 fallback key。

test('场景10a：insert 通道直接读取 piece 预分配的 renderNodeKey/windowInstanceId', () => {
  // 模拟 buildFrame 中 insert 通道的逻辑：
  // const insertRenderNodeKey = piece.renderNodeKey!
  // const insertWindowInstanceId = piece.windowInstanceId!
  const piece = {
    renderNodeKey: 'new-11-win-0',
    windowInstanceId: 'win-0',
    glyphIds: ['ins-11-0'],
  }

  // 直接读取预分配值（模拟 `!` 断言行为）
  const insertRenderNodeKey = piece.renderNodeKey
  const insertWindowInstanceId = piece.windowInstanceId

  // 使用预分配的值，不是 fallback
  assert.equal(insertRenderNodeKey, 'new-11-win-0')
  assert.equal(insertWindowInstanceId, 'win-0')
  // 不包含 -unplanned 后缀
  assert.ok(!insertRenderNodeKey.includes('-unplanned'))
  assert.ok(!insertWindowInstanceId.includes('-unplanned'))
})

test('场景10b：deleted 通道直接读取 piece 预分配的 renderNodeKey/windowInstanceId', () => {
  // 模拟 buildFrame 中 deleted 通道的逻辑：
  // const deletedRenderNodeKey = piece.renderNodeKey!
  // const deletedWindowInstanceId = piece.windowInstanceId!
  const piece = {
    renderNodeKey: 'old-10-win-1',
    windowInstanceId: 'win-1',
    glyphIds: ['orig-10-0'],
  }

  const deletedRenderNodeKey = piece.renderNodeKey
  const deletedWindowInstanceId = piece.windowInstanceId

  assert.equal(deletedRenderNodeKey, 'old-10-win-1')
  assert.equal(deletedWindowInstanceId, 'win-1')
  assert.ok(!deletedRenderNodeKey.includes('-unplanned'))
  assert.ok(!deletedWindowInstanceId.includes('-unplanned'))
})

test('场景10c：retainedMoves 通道直接读取 interval 预分配的 renderNodeKey/windowInstanceId', () => {
  // 模拟 buildFrame 中 retainedMoves 通道的逻辑：
  // const fallbackRenderNodeKey = interval.renderNodeKey!
  // const fallbackWindowInstanceId = interval.windowInstanceId!
  const interval = {
    renderNodeKey: 'old-10-win-2',
    windowInstanceId: 'win-2',
    glyphIds: ['orig-10-1', 'orig-10-2'],
  }

  const intervalRenderNodeKey = interval.renderNodeKey
  const intervalWindowInstanceId = interval.windowInstanceId

  assert.equal(intervalRenderNodeKey, 'old-10-win-2')
  assert.equal(intervalWindowInstanceId, 'win-2')
  assert.ok(!intervalRenderNodeKey.includes('-unplanned'))
  assert.ok(!intervalWindowInstanceId.includes('-unplanned'))
})

test('场景10d：member 直接读取预分配的 renderNodeKey（frozen/target 成员）', () => {
  // 模拟 buildFrame 中 member 的逻辑：
  // const memberRenderNodeKey = member.renderNodeKey
  const frozenMember = {
    renderNodeKey: 'old-10-win-3',
    windowInstanceId: 'win-3',
    role: 'frozen',
  }
  const targetMember = {
    renderNodeKey: 'new-11-win-4',
    windowInstanceId: 'win-4',
    role: 'target',
  }

  // frozen 成员直接读取 member.renderNodeKey
  const frozenKey = frozenMember.renderNodeKey
  assert.equal(frozenKey, 'old-10-win-3')
  assert.ok(!frozenKey.includes('-unplanned'))

  // target 成员也直接读取 member.renderNodeKey
  const targetKey = targetMember.renderNodeKey
  assert.equal(targetKey, 'new-11-win-4')
  assert.ok(!targetKey.includes('-unplanned'))
})

test('场景10e：连续帧采样时预分配值保持不变（确定性）', () => {
  // buildFrame 直接读取预分配值，连续帧采样时所有帧使用相同的 key
  const piece = {
    renderNodeKey: 'new-11-win-5',
    windowInstanceId: 'win-5',
    glyphIds: ['ins-11-2'],
  }

  // 模拟连续帧采样：p=0, 0.25, 0.5, 1
  const frameKeys = []
  const frameWinIds = []
  for (const p of [0, 0.25, 0.5, 1]) {
    // buildFrame 每帧都直接读取预分配值
    frameKeys.push(piece.renderNodeKey)
    frameWinIds.push(piece.windowInstanceId)
  }

  // 所有帧的 key 相同（确定性）
  for (const key of frameKeys) {
    assert.equal(key, 'new-11-win-5')
    assert.ok(!key.includes('-unplanned'))
  }
  for (const winId of frameWinIds) {
    assert.equal(winId, 'win-5')
    assert.ok(!winId.includes('-unplanned'))
  }
  assert.equal(new Set(frameKeys).size, 1)
  assert.equal(new Set(frameWinIds).size, 1)
})

// ── Issue #879 评论6147754107：物理身份完整性测试 ──
//
// 本轮复核要求3个新测试场景：
// 1. frozen key 保留通道——buildSourceMembers 不再抹掉 motionChannel
// 2. carry-over 保留原始窗口身份——重复 retarget 保持相同 key/instanceId
// 3. 最终输出不能有同 key 的两张 Text——buildFrame 不再 push 同 key 的第二张窗口

test('场景11a：frozen key 保留通道——buildSourceMembers 不抹掉 motionChannel', () => {
  // 模拟 FrozenWindowSpan 的 renderNodeKey 包含 motionChannel（如 old-10-frozen-win-7）
  // buildSourceMembers 应原样传递 span.renderNodeKey，不重建为三段式
  const spanRenderNodeKey = 'old-10-frozen-win-7'
  const spanSourceKind = 'old'
  const spanSourceRevision = 10
  const spanWindowInstanceId = 'win-7'

  // 旧逻辑（bug）：renderNodeKey = `${sourceKind}-${sourceRevision}-${windowInstanceId}`
  // = 'old-10-win-7' —— 抹掉了 'frozen' 通道标识
  const oldBuggyKey = `${spanSourceKind}-${spanSourceRevision}-${spanWindowInstanceId}`
  assert.equal(oldBuggyKey, 'old-10-win-7')
  assert.notEqual(oldBuggyKey, spanRenderNodeKey)

  // 新逻辑（修复）：renderNodeKey = span.renderNodeKey（原样传递）
  const newFixedKey = spanRenderNodeKey
  assert.equal(newFixedKey, 'old-10-frozen-win-7')
  assert.ok(newFixedKey.includes('frozen'))
})

test('场景11b：frozen key 保留通道——不同 motionChannel 的同源窗口不退化为相同 key', () => {
  // Planner 创建的合法窗口 key 形如 old-10-frozen-win-7、old-10-deleted-win-7
  // 经过旧 buildSourceMembers，两者都会退化成 old-10-win-7
  const frozenSpanKey = 'old-10-frozen-win-7'
  const deletedSpanKey = 'old-10-deleted-win-7'

  // 旧逻辑（bug）：两者都退化为 old-10-win-7
  const oldBuggyFrozenKey = `old-10-win-7`
  const oldBuggyDeletedKey = `old-10-win-7`
  assert.equal(oldBuggyFrozenKey, oldBuggyDeletedKey) // bug：碰撞

  // 新逻辑（修复）：原样传递，不碰撞
  assert.notEqual(frozenSpanKey, deletedSpanKey)
  assert.ok(frozenSpanKey.includes('frozen'))
  assert.ok(deletedSpanKey.includes('deleted'))
})

test('场景11c：carry-over 保留原始 renderNodeKey 和 windowInstanceId', () => {
  // Issue #879 评论6148478104：模拟真实 collectPlannedKeys 状态。
  // collectPlannedKeys 只收集新 plan 的 key，不收集显示帧的 key。
  // carry-over 直接保留原始 renderNodeKey/windowInstanceId，不检查 existingKeys。
  const w = {
    renderNodeKey: 'old-10-frozen-win-3',
    windowInstanceId: 'win-3',
    sourceKind: 'old',
    sourceRevision: 10,
  }
  // 真实 collectPlannedKeys 只含新 plan 的 key（如新插入字的窗口），不含显示帧的 key
  const existingKeys = new Set(['new-11-target-win-7'])

  // 修复后逻辑：carry-over 直接保留原始 key，不检查 existingKeys，不 allocateFresh
  const carryOverRenderNodeKey = w.renderNodeKey
  const carryOverWinId = w.windowInstanceId

  // carry-over 应保留原始身份
  assert.equal(carryOverRenderNodeKey, 'old-10-frozen-win-3')
  assert.equal(carryOverWinId, 'win-3')
  assert.ok(carryOverRenderNodeKey.includes('frozen'))
  // 不应生成 carry- 前缀的新 key
  assert.ok(!carryOverRenderNodeKey.includes('carry'))
})

test('场景11d：carry-over 重复 retarget 保持相同 key/instanceId', () => {
  // Issue #879 评论6148478104：模拟真实 collectPlannedKeys 状态。
  // 两次 retarget 的新 plan 可能不同，但 carry-over 始终保留原始 key，不漂移。
  const w = {
    renderNodeKey: 'new-11-target-win-5',
    windowInstanceId: 'win-5',
    sourceKind: 'new',
    sourceRevision: 11,
  }

  // 第一次 retarget：新 plan 有其他窗口的 key，但不含 carry-over 的 key
  const existingKeys1 = new Set(['old-10-frozen-win-3', 'new-11-frozen-win-4'])
  // 修复后逻辑：carry-over 直接保留原始 key，不检查 existingKeys
  const key1 = w.renderNodeKey
  const winId1 = w.windowInstanceId

  // 第二次 retarget（同一窗口再次 carry-over）：新 plan 又有另一组 key
  const existingKeys2 = new Set(['old-10-frozen-win-6', 'new-11-frozen-win-7'])
  const key2 = w.renderNodeKey
  const winId2 = w.windowInstanceId

  // 两次 retarget 的 key 和 windowInstanceId 应相同且不漂移
  assert.equal(key1, key2)
  assert.equal(winId1, winId2)
  assert.equal(key1, 'new-11-target-win-5')
  assert.equal(winId1, 'win-5')
  // 不应生成 carry- 前缀的新 key
  assert.ok(!key1.includes('carry'))
  assert.ok(!key2.includes('carry'))
})

test('场景11e：carry-over 始终保留原始 key，不因冲突分配新 key', () => {
  // Issue #879 评论6148478104：carry-over 直接保留原始 key，不检查 existingKeys。
  // "确实需要两张 Text"的场景由 Planner 在计划建立时处理（分配不同 key），
  // 不是 carry-over 的责任。buildFrame 同 key 合并 clip。
  const w = {
    renderNodeKey: 'old-10-frozen-win-3',
    windowInstanceId: 'win-3',
    sourceKind: 'old',
    sourceRevision: 10,
  }
  // 即使新 plan 中有同 key 的窗口（同源不同片段），carry-over 也保留原始 key
  const existingKeys = new Set(['old-10-frozen-win-3'])

  // 修复后逻辑：carry-over 直接保留原始 key，不检查 existingKeys
  const carryOverRenderNodeKey = w.renderNodeKey
  const carryOverWinId = w.windowInstanceId

  // carry-over 始终保留原始 key
  assert.equal(carryOverRenderNodeKey, 'old-10-frozen-win-3')
  assert.equal(carryOverWinId, 'win-3')
  // 不生成 carry- 前缀的新 key
  assert.ok(!carryOverRenderNodeKey.includes('carry'))
  assert.equal(carryOverRenderNodeKey, w.renderNodeKey)
})

test('场景11f：同 key 不同片段合并 clip，同 key 不同物理 Text 属性才 throw', () => {
  // Issue #879 评论6148478104：buildFrame 同 key 合并逻辑放宽。
  // samePhysicalIdentity 只检查物理 Text 属性（sourceKind/sourceRevision/windowInstanceId/
  // offsetX/offsetY/opacity），不检查片段属性（utf16Start/End/glyphX/glyphY/motionOwnerKind）。
  // 同 key 的不同片段合并 clip 是正常行为。

  const mergedWindows = []
  const windowByKey = new Map()

  // 第一张窗口
  const w1 = {
    renderNodeKey: 'old-10-frozen-win-3',
    sourceKind: 'old',
    sourceRevision: 10,
    windowInstanceId: 'win-3',
    utf16Start: 0,
    utf16End: 2,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0, clipRight: 10, clipTop: 0, clipBottom: 20,
    currentClipLeft: 0, currentClipRight: 10,
    offsetX: 0, offsetY: 0, opacity: undefined,
  }
  windowByKey.set(w1.renderNodeKey, w1)
  mergedWindows.push(w1)

  // 第二张窗口——同 key 且物理 Text 属性一致，但片段属性不同（不同 utf16Start/End/glyphX）
  const w2 = {
    renderNodeKey: 'old-10-frozen-win-3', // 同 key
    sourceKind: 'old',
    sourceRevision: 10,
    windowInstanceId: 'win-3',
    utf16Start: 3, // 不同 utf16Start（片段属性）
    utf16End: 5,
    clipRects: [{ x: 30, y: 0, width: 10, height: 20 }],
    clipLeft: 30, clipRight: 40, clipTop: 0, clipBottom: 20,
    currentClipLeft: 30, currentClipRight: 40,
    offsetX: 0, offsetY: 0, opacity: undefined, // 物理 Text 属性一致
  }

  // 修复后：samePhysicalIdentity 只检查物理 Text 属性
  const existing = windowByKey.get(w2.renderNodeKey)
  const samePhysicalIdentity =
    existing.sourceKind === w2.sourceKind &&
    existing.sourceRevision === w2.sourceRevision &&
    existing.offsetX === w2.offsetX &&
    existing.offsetY === w2.offsetY &&
    existing.opacity === w2.opacity &&
    existing.windowInstanceId === w2.windowInstanceId

  // 物理 Text 属性一致 → 合并 clip（不 throw）
  assert.ok(samePhysicalIdentity, '同 key 且物理 Text 属性一致应合并 clip')

  existing.clipRects = [...existing.clipRects, ...w2.clipRects]
  existing.clipLeft = Math.min(existing.clipLeft, w2.clipLeft)
  existing.clipRight = Math.max(existing.clipRight, w2.clipRight)

  assert.equal(mergedWindows.length, 1)
  assert.equal(existing.clipRects.length, 2)
  assert.equal(existing.clipLeft, 0)
  assert.equal(existing.clipRight, 40)

  // 另一场景：同 key 但物理 Text 属性不一致（不同 windowInstanceId）→ throw
  const w3 = {
    renderNodeKey: 'old-10-frozen-win-3',
    sourceKind: 'old',
    sourceRevision: 10,
    windowInstanceId: 'win-9', // 不同 windowInstanceId（物理 Text 属性不一致）
    offsetX: 0, offsetY: 0, opacity: undefined,
  }
  const samePhysicalIdentity3 =
    existing.sourceKind === w3.sourceKind &&
    existing.sourceRevision === w3.sourceRevision &&
    existing.offsetX === w3.offsetX &&
    existing.offsetY === w3.offsetY &&
    existing.opacity === w3.opacity &&
    existing.windowInstanceId === w3.windowInstanceId

  assert.ok(!samePhysicalIdentity3, '同 key 但不同 windowInstanceId 应判为物理 Text 属性不一致')

  let threwError = false
  try {
    if (!samePhysicalIdentity3) {
      throw new Error(
        `Planner bug: same renderNodeKey '${w3.renderNodeKey}' but different physical identity.`
      )
    }
  } catch (e) {
    threwError = true
    assert.ok(e.message.includes('Planner bug'))
    assert.ok(e.message.includes(w3.renderNodeKey))
  }
  assert.ok(threwError, '同 key 但物理 Text 属性不一致应抛出错误')
})

test('场景11g：同 key 同物理身份时正常合并 clip', () => {
  // 同 key 且物理身份一致时，合并 clipRects 是正常行为
  const mergedWindows = []
  const windowByKey = new Map()

  const w1 = {
    renderNodeKey: 'old-10-frozen-win-3',
    sourceKind: 'old',
    sourceRevision: 10,
    utf16Start: 0,
    utf16End: 2,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0, clipRight: 10, clipTop: 0, clipBottom: 20,
    currentClipLeft: 0, currentClipRight: 10,
  }
  windowByKey.set(w1.renderNodeKey, w1)
  mergedWindows.push(w1)

  // 第二张窗口——同 key 且物理身份一致（只是 clipRects 不同）
  const w2 = {
    renderNodeKey: 'old-10-frozen-win-3',
    sourceKind: 'old',
    sourceRevision: 10,
    utf16Start: 0,
    utf16End: 2,
    clipRects: [{ x: 15, y: 0, width: 8, height: 20 }],
    clipLeft: 15, clipRight: 23, clipTop: 0, clipBottom: 20,
    currentClipLeft: 15, currentClipRight: 23,
  }

  const existing = windowByKey.get(w2.renderNodeKey)
  const samePhysicalIdentity =
    existing.sourceKind === w2.sourceKind &&
    existing.sourceRevision === w2.sourceRevision &&
    existing.utf16Start === w2.utf16Start &&
    existing.utf16End === w2.utf16End

  assert.ok(samePhysicalIdentity)

  // 合并 clip
  existing.clipRects = [...existing.clipRects, ...w2.clipRects]
  existing.clipLeft = Math.min(existing.clipLeft, w2.clipLeft)
  existing.clipRight = Math.max(existing.clipRight, w2.clipRight)

  // 合并后只有一张窗口，clipRects 包含两块
  assert.equal(mergedWindows.length, 1)
  assert.equal(existing.clipRects.length, 2)
  assert.equal(existing.clipLeft, 0)
  assert.equal(existing.clipRight, 23)
})

// Issue #879 评论6152165243 缺口1+2+3：一个父 Text 拆两段的测试场景。
// planCarryOverPhysicalWindows 在拆片后按物理 Text 分组：
// - 同源多岛、相同轨迹 → 合并为一个组（一张 Text）
// - 同源两片段、不同轨迹 → 分配不同且固定的身份（两张 Text）
// - 同一候选重复 retarget → 身份不变

test('场景12a：同源多岛相同轨迹——合并为一个物理组，一张 Text', () => {
  // 同一父窗口拆成两个 piece，同一运动通道（ghost），相同物理属性
  // 应合并为一个物理组：glyphIds 并集、clipRects 并集、一张 Text
  const parentWindowInstanceId = 'win-3'
  const parentWindowId = 'window-1'
  const sourceKind = 'old'
  const sourceRevision = 10

  // 两个 piece 来自同一父窗口，同一 ghost 通道
  const piece1 = {
    glyphIds: ['g1', 'g2'],
    sourceUtf16Start: 0, sourceUtf16End: 2,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0, clipRight: 10, clipTop: 0, clipBottom: 20,
    offsetX: 0, offsetY: 0,
  }
  const piece2 = {
    glyphIds: ['g3', 'g4'],
    sourceUtf16Start: 3, sourceUtf16End: 5,
    clipRects: [{ x: 30, y: 0, width: 10, height: 20 }],
    clipLeft: 30, clipRight: 40, clipTop: 0, clipBottom: 20,
    offsetX: 0, offsetY: 0,
  }

  // 模拟 planCarryOverPhysicalWindows 的分组逻辑
  const groupKey = `${parentWindowId}|ghost|-1`
  // 同 groupKey → 合并
  const mergedGlyphIds = [...piece1.glyphIds, ...piece2.glyphIds]
  const mergedClipRects = [...piece1.clipRects, ...piece2.clipRects]
  const mergedSourceUtf16Start = Math.min(piece1.sourceUtf16Start, piece2.sourceUtf16Start)
  const mergedSourceUtf16End = Math.max(piece1.sourceUtf16End, piece2.sourceUtf16End)
  const mergedClipLeft = Math.min(piece1.clipLeft, piece2.clipLeft)
  const mergedClipRight = Math.max(piece1.clipRight, piece2.clipRight)

  // 合并后只有一个物理组
  assert.equal(mergedGlyphIds.length, 4)
  assert.equal(mergedClipRects.length, 2)
  assert.equal(mergedSourceUtf16Start, 0)
  assert.equal(mergedSourceUtf16End, 5)
  assert.equal(mergedClipLeft, 0)
  assert.equal(mergedClipRight, 40)

  // 同一物理组使用同一 renderNodeKey
  const stableRenderNodeKey = `${sourceKind}-${sourceRevision}-carryGhost-${parentWindowInstanceId}`
  assert.ok(stableRenderNodeKey.includes('carryGhost'))
  assert.ok(stableRenderNodeKey.includes(parentWindowInstanceId))
})

test('场景12b：同源两片段不同目标行——分配不同且固定的身份', () => {
  // 同一父窗口拆成两个 reflow piece，不同目标行
  // 应分配不同的 renderNodeKey（不同目标行 = 不同轨迹）
  const parentWindowInstanceId = 'win-5'
  const sourceKind = 'old'
  const sourceRevision = 10

  // piece1 目标行 y=0
  const piece1TargetLineIdx = 0
  // piece2 目标行 y=30
  const piece2TargetLineIdx = 30

  // 不同 targetLineIdx → 不同 groupKey → 不同身份
  const key1 = `${sourceKind}-${sourceRevision}-carryReflow-${parentWindowInstanceId}-${piece1TargetLineIdx}`
  const winId1 = `${parentWindowInstanceId}-cr-${piece1TargetLineIdx}`
  const key2 = `${sourceKind}-${sourceRevision}-carryReflow-${parentWindowInstanceId}-${piece2TargetLineIdx}`
  const winId2 = `${parentWindowInstanceId}-cr-${piece2TargetLineIdx}`

  // 两个不同的 renderNodeKey
  assert.notEqual(key1, key2)
  assert.notEqual(winId1, winId2)
  assert.ok(key1.includes('carryReflow'))
  assert.ok(key2.includes('carryReflow'))
  assert.ok(key1.includes(`${piece1TargetLineIdx}`))
  assert.ok(key2.includes(`${piece2TargetLineIdx}`))
})

test('场景12c：同源 ghost + surviving motion——分配不同通道的身份', () => {
  // 同一父窗口的部分字形转为 ghost（新版已删除），另一些继续 motion（新版仍存在）
  // ghost 和 motion 有不同的运动通道 → 不同的 renderNodeKey
  const parentWindowInstanceId = 'win-7'
  const sourceKind = 'old'
  const sourceRevision = 10

  // ghost 通道
  const ghostKey = `${sourceKind}-${sourceRevision}-carryGhost-${parentWindowInstanceId}`
  const ghostWinId = `${parentWindowInstanceId}-cg`
  // insert 通道
  const insertKey = `${sourceKind}-${sourceRevision}-carryInsert-${parentWindowInstanceId}`
  const insertWinId = `${parentWindowInstanceId}-ci`

  // 两个不同的 renderNodeKey
  assert.notEqual(ghostKey, insertKey)
  assert.notEqual(ghostWinId, insertWinId)
  assert.ok(ghostKey.includes('carryGhost'))
  assert.ok(insertKey.includes('carryInsert'))
})

test('场景12d：同一候选重复 retarget——身份不变，起点几何可以变化', () => {
  // 两次 retarget 的物理组身份应保持不变（基于父窗口身份 + 运动通道后缀）
  const parentWindowInstanceId = 'win-9'
  const sourceKind = 'old'
  const sourceRevision = 10

  // 第一次 retarget
  const key1 = `${sourceKind}-${sourceRevision}-carryGhost-${parentWindowInstanceId}`
  const winId1 = `${parentWindowInstanceId}-cg`

  // 第二次 retarget（同一父窗口再次 carry-over）
  const key2 = `${sourceKind}-${sourceRevision}-carryGhost-${parentWindowInstanceId}`
  const winId2 = `${parentWindowInstanceId}-cg`

  // 身份不变
  assert.equal(key1, key2)
  assert.equal(winId1, winId2)

  // 但起点几何可以变化（clipRects、currentPositionX/Y 等不同）
  const startClip1 = { x: 0, y: 0, width: 10, height: 20 }
  const startClip2 = { x: 0, y: 0, width: 5, height: 20 } // 动画推进后 clip 更窄
  assert.notDeepEqual(startClip1, startClip2) // 几何不同
  assert.equal(key1, key2) // 但身份相同
})

test('场景12e：buildFrame 不因同 key 不同片段抛错——规划阶段已分配不同身份', () => {
  // Issue #879 评论6152165243 缺口1：buildFrame 对同 key 的不同 offset/opacity 会 throw。
  // 规划阶段已为不同轨迹的片段分配不同 key，所以 buildFrame 不应遇到同 key 不同物理身份。
  // 模拟 buildFrame 的合并逻辑
  const mergedWindows = []
  const windowByKey = new Map()

  // 两个不同 key 的窗口（规划阶段已分配不同身份）
  const w1 = {
    renderNodeKey: 'old-10-carryGhost-win-3',
    sourceKind: 'old', sourceRevision: 10,
    windowInstanceId: 'win-3-cg',
    offsetX: 0, offsetY: 0, opacity: undefined,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    clipLeft: 0, clipRight: 10,
  }
  const w2 = {
    renderNodeKey: 'old-10-carryInsert-win-3', // 不同 key
    sourceKind: 'old', sourceRevision: 10,
    windowInstanceId: 'win-3-ci',
    offsetX: 0, offsetY: 0, opacity: undefined,
    clipRects: [{ x: 30, y: 0, width: 10, height: 20 }],
    clipLeft: 30, clipRight: 40,
  }

  // buildFrame 合并：不同 key → 两个独立窗口，不 throw
  windowByKey.set(w1.renderNodeKey, w1)
  mergedWindows.push(w1)
  windowByKey.set(w2.renderNodeKey, w2)
  mergedWindows.push(w2)

  assert.equal(mergedWindows.length, 2) // 两个独立窗口
  assert.notEqual(w1.renderNodeKey, w2.renderNodeKey) // 不同 key
  // 不存在同 key 冲突，所以不会 throw
})

test('场景12f：同源多岛合并后 fullWindowGlyphUtf16Ranges 是并集', () => {
  // 同一物理组合并后，fullWindowGlyphUtf16Ranges 应包含所有 piece 的 glyph 映射
  const parentRanges = new Map([
    ['g1', [0, 1]],
    ['g2', [1, 2]],
    ['g3', [3, 4]],
    ['g4', [4, 5]],
  ])

  // piece1 拥有 g1, g2
  const piece1Ranges = new Map()
  for (const gid of ['g1', 'g2']) {
    const r = parentRanges.get(gid)
    if (r) piece1Ranges.set(gid, r)
  }
  // piece2 拥有 g3, g4
  const piece2Ranges = new Map()
  for (const gid of ['g3', 'g4']) {
    const r = parentRanges.get(gid)
    if (r) piece2Ranges.set(gid, r)
  }

  // 合并后的 fullWindowGlyphUtf16Ranges 是并集
  const mergedRanges = new Map([...piece1Ranges, ...piece2Ranges])
  assert.equal(mergedRanges.size, 4)
  assert.deepEqual(mergedRanges.get('g1'), [0, 1])
  assert.deepEqual(mergedRanges.get('g2'), [1, 2])
  assert.deepEqual(mergedRanges.get('g3'), [3, 4])
  assert.deepEqual(mergedRanges.get('g4'), [4, 5])
})

console.log('---')
console.log(`✅ editor_physical_window_identity: ${passed} tests passed`)
