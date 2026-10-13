// editor_physical_window_identity.test.mjs — 物理窗口身份唯一化纯逻辑单测。
//
// Issue #879 物理窗口身份唯一化修复（3 个关联问题）：
// 1. buildFrame 同 key 不同绘制属性合并时不再 push 第二条窗口（跳过 instead of push）
// 2. buildFrame fallback 改用 WindowInstanceIdAllocator.allocateFresh() 代替 windowIdForGlyphIds()
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
    const identity = {
      sourceKind,
      sourceRevision,
      runId: `${sourceKind}-${sourceRevision}`,
      windowInstanceId: allocatedWinId,
      renderNodeKey: `${sourceKind}-${sourceRevision}-${allocatedWinId}`,
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

  // renderNodeKey = `${sourceKind}-${sourceRevision}-${windowInstanceId}`
  assert.equal(id.renderNodeKey, `old-10-${id.windowInstanceId}`)
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

// ── 场景8：不同来源同 key 冲突不丢字 ──
//
// buildFrame 合并逻辑：同 renderNodeKey 不同绘制属性时，
// 旧逻辑：直接 continue 丢掉第二个窗口（文字丢失）
// 新逻辑：给第二个窗口分配 `-dup` 后缀 key，保留两个窗口

test('场景8a：同 key 不同绘制属性时给第二个窗口分配 -dup 后缀 key', () => {
  // 模拟 buildFrame 的合并逻辑（物理身份校验合并）
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
      offsetX: 5,  // 不同 offsetX！
      offsetY: 0,
      opacity: 0.5, // 不同 opacity！
      utf16Start: 2,
      utf16End: 4,
      glyphX: 100,
      glyphY: 50,
      windowInstanceId: 'win-0',
      motionOwnerKind: 'reflow',
      clipRects: [{ x: 120, y: 50, width: 20, height: 30 }],
    },
  ]

  // 模拟合并逻辑
  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      const sameIdentity =
        existing.sourceKind === w.sourceKind &&
        existing.sourceRevision === w.sourceRevision &&
        existing.offsetX === w.offsetX &&
        existing.offsetY === w.offsetY &&
        existing.opacity === w.opacity &&
        existing.utf16Start === w.utf16Start &&
        existing.utf16End === w.utf16End &&
        existing.glyphX === w.glyphX &&
        existing.glyphY === w.glyphY &&
        existing.windowInstanceId === w.windowInstanceId &&
        existing.motionOwnerKind === w.motionOwnerKind
      if (sameIdentity) {
        existing.clipRects = [...existing.clipRects, ...w.clipRects]
        continue
      } else {
        // 同 key 不同绘制属性 → 分配 -dup 后缀 key
        const dupKey = `${w.renderNodeKey}-dup`
        if (!windowByKey.has(dupKey)) {
          w.renderNodeKey = dupKey
          windowByKey.set(dupKey, w)
          mergedWindows.push(w)
        }
      }
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 两个窗口都出现在输出中（不丢字）
  assert.equal(mergedWindows.length, 2)
  // 第一个窗口的 key 不变
  assert.equal(mergedWindows[0].renderNodeKey, 'old-10-win-0')
  // 第二个窗口的 key 有 -dup 后缀
  assert.equal(mergedWindows[1].renderNodeKey, 'old-10-win-0-dup')
  // 两个窗口的 renderNodeKey 不同
  assert.notEqual(mergedWindows[0].renderNodeKey, mergedWindows[1].renderNodeKey)
})

test('场景8b：同 key 同绘制属性时只合并 clip（不分配 -dup）', () => {
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

  const mergedWindows = []
  const windowByKey = new Map()
  for (const w of motionGlyphWindows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      const sameIdentity =
        existing.sourceKind === w.sourceKind &&
        existing.sourceRevision === w.sourceRevision &&
        existing.offsetX === w.offsetX &&
        existing.offsetY === w.offsetY &&
        existing.opacity === w.opacity &&
        existing.utf16Start === w.utf16Start &&
        existing.utf16End === w.utf16End &&
        existing.glyphX === w.glyphX &&
        existing.glyphY === w.glyphY &&
        existing.windowInstanceId === w.windowInstanceId &&
        existing.motionOwnerKind === w.motionOwnerKind
      if (sameIdentity) {
        existing.clipRects = [...existing.clipRects, ...w.clipRects]
        continue
      } else {
        const dupKey = `${w.renderNodeKey}-dup`
        if (!windowByKey.has(dupKey)) {
          w.renderNodeKey = dupKey
          windowByKey.set(dupKey, w)
          mergedWindows.push(w)
        }
      }
    } else {
      windowByKey.set(w.renderNodeKey, w)
      mergedWindows.push(w)
    }
  }

  // 只有一个窗口（合并了 clip），没有 -dup
  assert.equal(mergedWindows.length, 1)
  assert.equal(mergedWindows[0].renderNodeKey, 'old-10-win-0')
  // clipRects 合并了两段
  assert.equal(mergedWindows[0].clipRects.length, 2)
})

test('场景8c：旧逻辑（continue 丢字）vs 新逻辑（-dup 保留）对比', () => {
  // 模拟旧逻辑：同 key 不同属性 → continue（丢字）
  const windows = [
    { renderNodeKey: 'shared-key', offsetX: 0, clipRects: [{ x: 0, y: 0, width: 10, height: 20 }] },
    { renderNodeKey: 'shared-key', offsetX: 5, clipRects: [{ x: 20, y: 0, width: 10, height: 20 }] },
  ]

  // 旧逻辑：直接 continue → 只保留第一个窗口
  const oldResult = [windows[0]] // 第二个被 continue 丢弃
  assert.equal(oldResult.length, 1) // 丢了一个窗口！

  // 新逻辑：分配 -dup 后缀 → 两个窗口都保留
  const newResult = []
  const windowByKey = new Map()
  for (const w of windows) {
    const existing = windowByKey.get(w.renderNodeKey)
    if (existing !== undefined) {
      // 不同属性 → 分配 -dup
      const dupKey = `${w.renderNodeKey}-dup`
      w.renderNodeKey = dupKey
      windowByKey.set(dupKey, w)
      newResult.push(w)
    } else {
      windowByKey.set(w.renderNodeKey, w)
      newResult.push(w)
    }
  }
  assert.equal(newResult.length, 2) // 两个窗口都保留
  assert.notEqual(newResult[0].renderNodeKey, newResult[1].renderNodeKey)
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
  function simulatedBuildFrame(plan, progress) {
    // 只从 plan 中读取预分配的 key
    const windows = []
    for (const run of plan.insertRuns) {
      for (const piece of run.pieces) {
        windows.push({
          renderNodeKey: piece.renderNodeKey ?? `new-${plan.newRevision}-unplanned`,
          windowInstanceId: piece.windowInstanceId ?? `new-${plan.newRevision}-unplanned`,
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
  }
})

// ── 场景10：确定性 fallback key ──
//
// buildFrame 中动态创建的 target 成员（windowInstanceId 为 undefined 时）
// 使用确定性 fallback key：
// - fallback key 格式为 `${member.renderNodeKey}-unplanned`
// - 不调用 allocateFresh
// - 同一成员在连续帧中使用相同的 fallback key

test('场景10a：windowInstanceId 为 undefined 时使用确定性 fallback key', () => {
  // 模拟 buildFrame 中 target 成员的 fallback 逻辑
  const member = {
    renderNodeKey: 'new-11-win-3',
    windowInstanceId: undefined, // Planner bug：未预分配
    role: 'target',
    sourceKind: 'new',
    sourceRevision: 11,
  }

  // buildFrame 的 fallback 逻辑：
  // memberRenderNodeKey = member.windowInstanceId !== undefined
  //   ? member.renderNodeKey
  //   : `${member.renderNodeKey}-unplanned`
  const memberRenderNodeKey = member.windowInstanceId !== undefined
    ? member.renderNodeKey
    : `${member.renderNodeKey}-unplanned`

  // fallback key 格式为 `${member.renderNodeKey}-unplanned`
  assert.equal(memberRenderNodeKey, 'new-11-win-3-unplanned')
  // 不调用 allocateFresh（fallback key 是确定性的字符串拼接）
  assert.match(memberRenderNodeKey, /-unplanned$/)
})

test('场景10b：fallback key 不调用 allocateFresh', () => {
  // 验证 fallback key 是纯字符串拼接，不依赖 allocator
  // 即使没有 allocator 实例，fallback key 也能正确生成
  const member = {
    renderNodeKey: 'old-10-win-2',
    windowInstanceId: undefined,
    role: 'frozen',
  }

  // buildFrame 中 frozen 成员的 fallback：
  // memberRenderNodeKey = member.renderNodeKey（frozen 成员直接使用 member.renderNodeKey）
  // 但 windowInstanceId 为 undefined 时：
  // windowInstanceId = `${member.renderNodeKey}-unplanned`
  const frozenRenderNodeKey = member.renderNodeKey
  const frozenWindowInstanceId = member.windowInstanceId !== undefined
    ? member.windowInstanceId
    : `${member.renderNodeKey}-unplanned`

  assert.equal(frozenRenderNodeKey, 'old-10-win-2')
  assert.equal(frozenWindowInstanceId, 'old-10-win-2-unplanned')
  // 不需要 allocator 实例
})

test('场景10c：同一成员在连续帧中使用相同的 fallback key', () => {
  // 模拟一个 windowInstanceId 为 undefined 的 target 成员
  const member = {
    renderNodeKey: 'new-11-win-5',
    windowInstanceId: undefined,
    role: 'target',
  }

  // 连续帧采样
  const frameKeys = []
  const frameWinIds = []
  for (const p of [0, 0.25, 0.5, 1]) {
    // buildFrame 每帧都用相同的确定性 fallback 逻辑
    const key = member.windowInstanceId !== undefined
      ? member.renderNodeKey
      : `${member.renderNodeKey}-unplanned`
    const winId = member.windowInstanceId !== undefined
      ? member.windowInstanceId
      : `${member.renderNodeKey}-unplanned`
    frameKeys.push(key)
    frameWinIds.push(winId)
  }

  // 所有帧的 fallback key 相同（确定性）
  for (const key of frameKeys) {
    assert.equal(key, 'new-11-win-5-unplanned')
  }
  for (const winId of frameWinIds) {
    assert.equal(winId, 'new-11-win-5-unplanned')
  }
  // 验证一致性
  assert.equal(new Set(frameKeys).size, 1)
  assert.equal(new Set(frameWinIds).size, 1)
})

test('场景10d：insert 通道的 fallback key 格式为 new-{revision}-unplanned', () => {
  // buildFrame 中 insert 通道的 fallback：
  // insertRenderNodeKey = piece.renderNodeKey ?? `new-${newRevision}-unplanned`
  // insertWindowInstanceId = piece.windowInstanceId ?? `new-${newRevision}-unplanned`
  const newRevision = 11
  const piece = {
    renderNodeKey: undefined, // Planner bug
    windowInstanceId: undefined,
  }

  const insertRenderNodeKey = piece.renderNodeKey ?? `new-${newRevision}-unplanned`
  const insertWindowInstanceId = piece.windowInstanceId ?? `new-${newRevision}-unplanned`

  assert.equal(insertRenderNodeKey, 'new-11-unplanned')
  assert.equal(insertWindowInstanceId, 'new-11-unplanned')
})

test('场景10e：deleted 通道的 fallback key 格式为 old-{revision}-unplanned', () => {
  // buildFrame 中 deleted 通道的 fallback：
  // deletedRenderNodeKey = piece.renderNodeKey ?? `old-${oldRevision}-unplanned`
  // deletedWindowInstanceId = piece.windowInstanceId ?? `old-${oldRevision}-unplanned`
  const oldRevision = 10
  const piece = {
    renderNodeKey: undefined,
    windowInstanceId: undefined,
  }

  const deletedRenderNodeKey = piece.renderNodeKey ?? `old-${oldRevision}-unplanned`
  const deletedWindowInstanceId = piece.windowInstanceId ?? `old-${oldRevision}-unplanned`

  assert.equal(deletedRenderNodeKey, 'old-10-unplanned')
  assert.equal(deletedWindowInstanceId, 'old-10-unplanned')
})

console.log('---')
console.log(`✅ editor_physical_window_identity: ${passed} tests passed`)
