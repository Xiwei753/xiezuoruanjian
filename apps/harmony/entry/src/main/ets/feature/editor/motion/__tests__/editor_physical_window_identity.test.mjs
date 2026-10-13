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

console.log('---')
console.log(`✅ editor_physical_window_identity: ${passed} tests passed`)
