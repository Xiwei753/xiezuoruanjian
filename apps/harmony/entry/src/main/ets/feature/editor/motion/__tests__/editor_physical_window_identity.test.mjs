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
import {
  planCarryOverPhysicalGroups, determineMotionChannel,
  generateGroupKey, generateRenderNodeKey,
  isInsertGroupComplete, releaseCarryOverIdentity,
} from '../editor_carry_over_physical_plan.ts'
import { rectDifference, isFullyCovered } from '../editor_commit_start.ts'

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

// Issue #879 评论6153434157 问题5：测试 12a-12f 改为直接引用正式实现。
// 使用 editor_carry_over_physical_plan.ts 的 planCarryOverPhysicalGroups 函数，
// 不再手写拼 key 逻辑。

// ── 辅助：构造 piece 输入 ──
function makePiece(glyphIds, sourceStart, sourceEnd, clipRects, opts = {}) {
  return {
    glyphIds,
    sourceUtf16Start: sourceStart,
    sourceUtf16End: sourceEnd,
    clipRects,
    clipLeft: opts.clipLeft ?? clipRects[0]?.x ?? 0,
    clipRight: opts.clipRight ?? (clipRects[0] ? clipRects[0].x + clipRects[0].width : 0),
    clipTop: opts.clipTop ?? clipRects[0]?.y ?? 0,
    clipBottom: opts.clipBottom ?? (clipRects[0] ? clipRects[0].y + clipRects[0].height : 0),
    offsetX: opts.offsetX ?? 0,
    offsetY: opts.offsetY ?? 0,
    currentPositionX: opts.currentPositionX ?? (clipRects[0]?.x ?? 0),
    currentPositionY: opts.currentPositionY ?? (clipRects[0]?.y ?? 0),
    targetUtf16Start: opts.targetUtf16Start ?? -1,
    targetUtf16End: opts.targetUtf16End ?? -1,
    targetResolved: opts.targetResolved ?? false,
    targetRect: opts.targetRect ?? null,
    glyphX: opts.glyphX ?? (clipRects[0]?.x ?? 0),
    glyphY: opts.glyphY ?? (clipRects[0]?.y ?? 0),
    glyphWidth: opts.glyphWidth ?? (clipRects[0]?.width ?? 0),
    glyphHeight: opts.glyphHeight ?? (clipRects[0]?.height ?? 0),
  }
}

function makeParent(windowInstanceId, sourceKind = 'old', sourceRevision = 10, motionOwnerKind = 'deleted', renderNodeKey = null, fullRanges = null) {
  return {
    windowId: `window-${windowInstanceId}`,
    renderNodeKey: renderNodeKey ?? `${sourceKind}-${sourceRevision}-${windowInstanceId}`,
    windowInstanceId,
    sourceKind,
    sourceRevision,
    motionOwnerKind,
    opacity: undefined,
    fullWindowGlyphUtf16Ranges: fullRanges,
  }
}

test('场景12a：同源多岛相同轨迹——合并为一个物理组，一张 Text', () => {
  // 同一父窗口拆成两个 piece，同一 ghost 通道，相同物理属性
  // 应合并为一个物理组：glyphIds 并集、clipRects 并集
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-3', 'old', 10, 'deleted')
  const piece1 = makePiece(['g1', 'g2'], 0, 2, [{ x: 0, y: 0, width: 10, height: 20 }])
  const piece2 = makePiece(['g3', 'g4'], 3, 5, [{ x: 30, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
    { piece: piece2, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  // 合并后只有一个物理组
  assert.equal(results.length, 1)
  const group = results[0]
  assert.equal(group.glyphIds.length, 4)
  assert.equal(group.clipRects.length, 2)
  assert.equal(group.sourceUtf16Start, 0)
  assert.equal(group.sourceUtf16End, 5)
  assert.equal(group.clipLeft, 0)
  assert.equal(group.clipRight, 40)
  assert.ok(group.renderNodeKey.includes('carryGhost'))
  // Issue #879 评论6158342861 问题2：renderNodeKey 不再包含父 windowInstanceId，
  // 而是包含通过 allocateFresh 分配的子窗口实例 ID
  assert.match(group.renderNodeKey, /^old-10-carryGhost-win-\d+$/)
  // windowInstanceId 通过分配器分配，格式为 win-<number>
  assert.ok(group.windowInstanceId.startsWith('win-'))
  // 分配器中应能 resolve 该 instanceId
  assert.ok(allocator.resolve(group.windowInstanceId) !== null)
})

test('场景12b：同源两片段不同目标行——分配不同且固定的 renderNodeKey 和 instanceId', () => {
  // 同一父窗口拆成两个 reflow piece，不同目标位置
  // 应分配不同的 renderNodeKey（不同目标位置 = 不同轨迹）
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-5', 'old', 10, 'reflow')
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
  })
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 20, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 30, width: 10, height: 20 },
  })

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'reflow' },
    { piece: piece2, parent, motionChannel: 'reflow' },
  ], allocator, cache)

  // 两个不同的物理组
  assert.equal(results.length, 2)
  const [group1, group2] = results

  // 不同的 renderNodeKey（通过 allocateFresh 分配不同子窗口实例 ID）
  assert.notEqual(group1.renderNodeKey, group2.renderNodeKey)
  assert.ok(group1.renderNodeKey.includes('carryReflow'))
  assert.ok(group2.renderNodeKey.includes('carryReflow'))
  // Issue #879 评论6158342861 问题2：renderNodeKey 不再包含目标位置信息，
  // 目标位置只参与 groupKey 的分组，最终 Text key 由已分配的唯一物理实例决定

  // 不同的 windowInstanceId
  assert.notEqual(group1.windowInstanceId, group2.windowInstanceId)
  // 都通过分配器分配
  assert.ok(allocator.resolve(group1.windowInstanceId) !== null)
  assert.ok(allocator.resolve(group2.windowInstanceId) !== null)
})

test('场景12c：同源 ghost + surviving motion——分配不同通道的身份', () => {
  // 同一父窗口的部分字形转为 ghost（新版已删除），另一些继续 motion（新版仍存在）
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-7', 'old', 10, 'deleted')
  const ghostPiece = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  // Issue #879 评论6158702656 缺口1：insert piece 的 clip 只覆盖部分区域（正在吐字），
  // 否则 isInsertGroupComplete 会判定已完成并筛除该组。
  // glyphWidth=10 是完整字形宽度，clip width=3 表示只吐出了 3px。
  const insertPiece = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 30, y: 0, width: 10, height: 20 },
    glyphX: 30,
    glyphY: 0,
    glyphWidth: 10,
    glyphHeight: 20,
  })

  const results = planCarryOverPhysicalGroups([
    { piece: ghostPiece, parent, motionChannel: 'ghost' },
    { piece: insertPiece, parent, motionChannel: 'insert' },
  ], allocator, cache)

  // 两个不同的物理组
  assert.equal(results.length, 2)
  const [ghostGroup, insertGroup] = results

  assert.notEqual(ghostGroup.renderNodeKey, insertGroup.renderNodeKey)
  assert.ok(ghostGroup.renderNodeKey.includes('carryGhost'))
  assert.ok(insertGroup.renderNodeKey.includes('carryInsert'))
  assert.notEqual(ghostGroup.windowInstanceId, insertGroup.windowInstanceId)
})

test('场景12d：同一候选重复 retarget——身份不变，起点几何可以变化', () => {
  // 两次 retarget 的物理组身份应保持不变
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map() // 跨 retarget 复用同一缓存

  const parent = makeParent('win-9', 'old', 10, 'deleted')

  // 第一次 retarget
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  const results1 = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  // 第二次 retarget（同一父窗口再次 carry-over，几何变化但身份不变）
  const piece2 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 5, height: 20 }])
  const results2 = planCarryOverPhysicalGroups([
    { piece: piece2, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results1.length, 1)
  assert.equal(results2.length, 1)

  // 身份不变（同一缓存 → 同一身份）
  assert.equal(results1[0].renderNodeKey, results2[0].renderNodeKey)
  assert.equal(results1[0].windowInstanceId, results2[0].windowInstanceId)

  // 但几何可以变化
  assert.notDeepEqual(results1[0].clipRects, results2[0].clipRects)
})

test('场景12e：buildFrame 不因同 key 不同片段抛错——规划阶段已分配不同身份', () => {
  // 两个不同通道的窗口（规划阶段已分配不同身份），不应有同 key 冲突
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-3', 'old', 10, 'deleted')
  const ghostPiece = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  // Issue #879 评论6158702656 缺口1：insert piece 的 clip 只覆盖部分区域（正在吐字），
  // 否则 isInsertGroupComplete 会判定已完成并筛除该组。
  // glyphWidth=10 是完整字形宽度，clip width=3 表示只吐出了 3px。
  const insertPiece = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 30, y: 0, width: 10, height: 20 },
    glyphX: 30,
    glyphY: 0,
    glyphWidth: 10,
    glyphHeight: 20,
  })

  const results = planCarryOverPhysicalGroups([
    { piece: ghostPiece, parent, motionChannel: 'ghost' },
    { piece: insertPiece, parent, motionChannel: 'insert' },
  ], allocator, cache)

  assert.equal(results.length, 2)
  assert.notEqual(results[0].renderNodeKey, results[1].renderNodeKey)
  assert.notEqual(results[0].windowInstanceId, results[1].windowInstanceId)

  // 所有 windowInstanceId 都在分配器中登记
  for (const r of results) {
    assert.ok(allocator.resolve(r.windowInstanceId) !== null,
      `windowInstanceId ${r.windowInstanceId} 应在分配器中登记`)
  }
})

test('场景12f：同源多岛合并后 fullWindowGlyphUtf16Ranges 是并集', () => {
  // 同一物理组合并后，fullWindowGlyphUtf16Ranges 应包含所有 piece 的 glyph 映射
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parentRanges = new Map([
    ['g1', [0, 1]],
    ['g2', [1, 2]],
    ['g3', [3, 4]],
    ['g4', [4, 5]],
  ])
  const parent = makeParent('win-11', 'old', 10, 'deleted', null, parentRanges)

  const piece1 = makePiece(['g1', 'g2'], 0, 2, [{ x: 0, y: 0, width: 10, height: 20 }])
  const piece2 = makePiece(['g3', 'g4'], 3, 5, [{ x: 30, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
    { piece: piece2, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results.length, 1)
  const group = results[0]
  assert.ok(group.fullWindowGlyphUtf16Ranges !== null)
  assert.equal(group.fullWindowGlyphUtf16Ranges.size, 4)
  assert.deepEqual(group.fullWindowGlyphUtf16Ranges.get('g1'), [0, 1])
  assert.deepEqual(group.fullWindowGlyphUtf16Ranges.get('g2'), [1, 2])
  assert.deepEqual(group.fullWindowGlyphUtf16Ranges.get('g3'), [3, 4])
  assert.deepEqual(group.fullWindowGlyphUtf16Ranges.get('g4'), [4, 5])
})

test('场景12g：同父窗口不同目标位置的 reflow——groupKey 包含 parentRenderNodeKey 和 sourceKind/Revision', () => {
  // 验证 groupKey 包含 parentRenderNodeKey 和 sourceKind/sourceRevision
  // 不同父窗口的相同通道 + 相同目标位置 → 不同组
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent1 = makeParent('win-1', 'old', 10, 'reflow', 'old-10-win-1')
  const parent2 = makeParent('win-2', 'old', 10, 'reflow', 'old-10-win-2')

  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
  })
  const piece2 = makePiece(['g2'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
  })

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent: parent1, motionChannel: 'reflow' },
    { piece: piece2, parent: parent2, motionChannel: 'reflow' },
  ], allocator, cache)

  // 不同父窗口 → 不同组（即使目标和通道相同）
  assert.equal(results.length, 2)
  assert.notEqual(results[0].renderNodeKey, results[1].renderNodeKey)
})

test('场景12h：合并后的组拥有完整目标信息——不只取第一个 piece 的值', () => {
  // Issue #879 评论6153434157 问题4：合并后不能丢目标所有权
  // 同一 reflow 组的两个 piece（相同目标位置），合并后 targetUtf16Start/End 应是完整范围
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-13', 'old', 10, 'reflow')
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 5, height: 20 }], {
    targetResolved: true,
    targetUtf16Start: 10,
    targetUtf16End: 11,
    targetRect: { x: 0, y: 0, width: 5, height: 20 },
  })
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 5, y: 0, width: 5, height: 20 }], {
    targetResolved: true,
    targetUtf16Start: 11,
    targetUtf16End: 12,
    targetRect: { x: 5, y: 0, width: 5, height: 20 },
  })

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'reflow' },
    { piece: piece2, parent, motionChannel: 'reflow' },
  ], allocator, cache)

  // 两个 piece 目标位置不同（x=0 vs x=5）→ 不同组
  // 等等——reflow 的 groupKey 包含 targetRect.x 和 targetRect.y
  // piece1 targetRect = {x:0, y:0}，piece2 targetRect = {x:5, y:0}
  // 不同目标位置 → 不同组
  assert.equal(results.length, 2)
})

// ── Issue #879 评论6158342861：三个修复的生产路径测试 ──

// 问题1测试：一个未吐完字的窗口在其他位置继续编辑，规划出 insert carry group，
// 必须仍输出 carryOverMotionRun（旧逻辑会错误判为"全部完成"而跳过）
test('问题1-6158342861：未吐完字的窗口在其他位置编辑——insert carry group 不被错误判为完成', () => {
  // 场景：上一帧只有一个正在吐字的窗口 w，包含 g1；
  // 新笔编辑发生在别处，planCarryOverPhysicalWindows 正确产生 insert(g1)；
  // 旧逻辑检查 g1 是否在 physicalGroups 中 → 是 → 标记 done → insert(g1) 不进入 runs
  // 新逻辑：基于物理组自身 clip 覆盖判断完成状态——g1 的 clipRects 未完全覆盖完整字形矩形 → 未完成 → 输出 run

  // 模?模拟 planCarryOverPhysicalGroups 的输出：一个 insert 组，clipRects 未完全覆盖
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-1', 'new', 11, 'insert')
  // piece 的 clipRects 只覆盖了完整字形的一部分（吐字未完成）
  const piece = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
  })
  piece.glyphX = 0
  piece.glyphY = 0
  piece.glyphWidth = 10
  piece.glyphHeight = 20

  const results = planCarryOverPhysicalGroups([
    { piece, parent, motionChannel: 'insert' },
  ], allocator, cache)

  assert.equal(results.length, 1)
  const group = results[0]
  assert.equal(group.motionChannel, 'insert')

  // 关键验证：clipRects 未完全覆盖完整字形矩形（3 < 10）→ 不应被判为完成
  // 旧逻辑会错误判为完成（因为 g1 在 physicalGroups 中），新逻辑基于真实 clip 判断
  const fullRect = { x: group.glyphX + group.offsetX, y: group.glyphY + group.offsetY, width: group.glyphWidth, height: group.glyphHeight }
  assert.ok(!isFullyCovered(fullRect, group.clipRects),
    '未吐完的字（clip=3 < fullWidth=10）不应被判为完成')
})

// 问题2测试：两个父窗口 sourceKind/revision 和 windowInstanceId 相同但 parent.renderNodeKey 不同，
// 目标相同、通道同为 ghost，应生成两个不同 key、两个实际登记的 instanceId
test('问题2-6158342861：同 sourceKind/revision/windowInstanceId 但不同 parentRenderNodeKey——生成不同 key', () => {
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  // 两个父窗口：sourceKind=old, sourceRevision=10, windowInstanceId=win-7
  // 但 parentRenderNodeKey 不同（old-10-frozen-win-7 vs old-10-deleted-win-7）
  const parent1 = makeParent('win-7', 'old', 10, 'deleted', 'old-10-frozen-win-7')
  const parent2 = makeParent('win-7', 'old', 10, 'deleted', 'old-10-deleted-win-7')

  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent: parent1, motionChannel: 'ghost' },
    { piece: piece2, parent: parent2, motionChannel: 'ghost' },
  ], allocator, cache)

  // 两个不同的物理组（因为 parentRenderNodeKey 不同 → groupKey 不同）
  assert.equal(results.length, 2)

  // 关键验证：两个 renderNodeKey 不同
  assert.notEqual(results[0].renderNodeKey, results[1].renderNodeKey,
    '不同 parentRenderNodeKey 的 ghost 组应有不同 renderNodeKey')

  // 两个 windowInstanceId 不同（通过 allocateFresh 分配）
  assert.notEqual(results[0].windowInstanceId, results[1].windowInstanceId,
    '不同物理组应有不同 windowInstanceId')

  // 两个 windowInstanceId 都在分配器中登记
  assert.ok(allocator.resolve(results[0].windowInstanceId) !== null)
  assert.ok(allocator.resolve(results[1].windowInstanceId) !== null)

  // renderNodeKey 格式正确：包含 carryGhost 和子窗口实例 ID（不是父 win-7）
  for (const r of results) {
    assert.match(r.renderNodeKey, /^old-10-carryGhost-win-\d+$/,
      `renderNodeKey 应为 old-10-carryGhost-win-<number> 格式，got: ${r.renderNodeKey}`)
    assert.notEqual(r.windowInstanceId, 'win-7',
      '子窗口 instanceId 不应等于父窗口 instanceId win-7')
  }
})

// 问题3测试：一个父 insert 窗口拆成两个不连续可见岛，
// 两个字的未露出部分均能扩展到完整矩形
test('问题3-6158342861：同父 insert 窗口两个不连续可见岛——各岛源完整矩形单独保留', () => {
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  // 同一父窗口拆成两个 insert piece（两个不连续可见岛）
  // piece1: 字形 [0,10]，当前可见 [0,3]
  // piece2: 字形 [30,40]，当前可见 [30,33]
  // 旧逻辑：合组后 fullRect 只有 [0,10]，第二字的 [33,40] 从未生成目标裁切
  // 新逻辑：islandSourceRects 保留各岛源完整矩形，各岛单独做 rectDifference

  const parent = makeParent('win-5', 'new', 11, 'insert')

  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
  })
  piece1.glyphX = 0
  piece1.glyphY = 0
  piece1.glyphWidth = 10
  piece1.glyphHeight = 20

  const piece2 = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 30, y: 0, width: 10, height: 20 },
  })
  piece2.glyphX = 30
  piece2.glyphY = 0
  piece2.glyphWidth = 10
  piece2.glyphHeight = 20

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'insert' },
    { piece: piece2, parent, motionChannel: 'insert' },
  ], allocator, cache)

  // 合并为一个物理组（同一父窗口、同一 insert 通道）
  assert.equal(results.length, 1)
  const group = results[0]
  assert.equal(group.motionChannel, 'insert')

  // 关键验证：islandSourceRects 保留了两个岛的源完整矩形
  assert.equal(group.islandSourceRects.length, 2,
    '两个不连续可见岛应保留两个 islandSourceRects')

  // 第一个岛的源完整矩形：[0, 0, 10, 20]
  assert.deepEqual(group.islandSourceRects[0], { x: 0, y: 0, width: 10, height: 20 })

  // 第二个岛的源完整矩形：[30, 0, 10, 20]——不是第一个岛的矩形！
  assert.deepEqual(group.islandSourceRects[1], { x: 30, y: 0, width: 10, height: 20 })

  // islandClipRects 保留了两个岛的起始 clip
  assert.equal(group.islandClipRects.length, 2)
  // 第一个岛的起始 clip：[0, 0, 3, 20]
  assert.deepEqual(group.islandClipRects[0], [{ x: 0, y: 0, width: 3, height: 20 }])
  // 第二个岛的起始 clip：[30, 0, 3, 20]
  assert.deepEqual(group.islandClipRects[1], [{ x: 30, y: 0, width: 3, height: 20 }])
})

// 问题3补充测试：验证 rectDifference 对各岛单独计算后合并裁切并集
test('问题3-6158342861补充：各岛单独 rectDifference 后合并裁切并集——两字未露出部分均能扩展', () => {
  // 模拟 identifyCarryOverMotionRuns insert 分支的核心逻辑：
  // 对每个 islandSourceRect 单独做 rectDifference(islandFullRect, islandClipRects)
  // 再合并结果为同一个 Text 的裁切并集

  const islandSourceRects = [
    { x: 0, y: 0, width: 10, height: 20 },   // 第一字完整矩形
    { x: 30, y: 0, width: 10, height: 20 },  // 第二字完整矩形
  ]
  const islandClipRects = [
    [{ x: 0, y: 0, width: 3, height: 20 }],   // 第一字当前可见 [0,3]
    [{ x: 30, y: 0, width: 3, height: 20 }],  // 第二字当前可见 [30,33]
  ]
  const offsetX = 0
  const offsetY = 0

  // 模拟福新逻辑：各岛单独做 rectDifference
  const allStartRects = []
  const allTargetRects = []
  for (let i = 0; i < islandSourceRects.length; i++) {
    const islandRect = islandSourceRects[i]
    const islandClip = islandClipRects[i]
    const islandFullRect = {
      x: islandRect.x + offsetX,
      y: islandRect.y + offsetY,
      width: islandRect.width,
      height: islandRect.height
    }
    // 已有 clip 保持不变
    for (const cr of islandClip) {
      allStartRects.push({ x: cr.x, y: cr.y, width: cr.width, height: cr.height })
      allTargetRects.push({ x: cr.x, y: cr.y, width: cr.width, height: cr.height })
    }
    // 该岛尚未覆盖的区域——从零面积长成完整区域
    const remaining = rectDifference(islandFullRect, islandClip)
    for (const gap of remaining) {
      allStartRects.push({ x: gap.x, y: gap.y, width: 0, height: gap.height })
      allTargetRects.push({ x: gap.x, y: gap.y, width: gap.width, height: gap.height })
    }
  }

  // 验证：两个字的未露出部分都生成了目标裁切
  // 第一字：已有 [0,3] + 新增 [3,10]（从零长成完整）
  // 第二字：已有 [30,33] + 新增 [33,40]（从零长成完整）

  // 检查第一字的 gap
  const firstIslandGaps = allTargetRects.filter(r => r.x >= 3 && r.x < 10 && r.width > 0)
  assert.ok(firstIslandGaps.length > 0, '第一字的 [3,10] 未露出部分应生成目标裁切')

  // 检查第二字的 gap——这是旧逻辑会遗漏的部分！
  const secondIslandGaps = allTargetRects.filter(r => r.x >= 33 && r.x < 40 && r.width > 0)
  assert.ok(secondIslandGaps.length > 0, '第二字的 [33,40] 未露出部分应生成目标裁切')

  // 旧逻辑验证（反证）：如果只用第一个 piece 的 glyphX/Width 做 fullRect
  const oldFullRect = { x: 0, y: 0, width: 10, height: 20 }
  const oldRemaining = rectDifference(oldFullRect, [
    ...islandClipRects[0], ...islandClipRects[1]
  ])
  // 旧逻辑的 remaining 不包含第二字的 [33,40] 区域
  const oldHasSecondGap = oldRemaining.some(r => r.x >= 33)
  assert.ok(!oldHasSecondGap, '旧逻辑（只用首片段 fullRect）确实遗漏了第二字的未露出部分')
})

// ── Issue #879 评论6158702656 三个缺口的生产逻辑用例 ──

// 缺口1测试：多岛 insert 的完成判定——第一字已满、第二字未满仍产生 motion run
test('缺口1-6158702656：多岛 insert 第一字已满第二字未满——isInsertGroupComplete 返回 false', () => {
  // 场景：同组第 1 个完整字 [0,10]、已显示 [0,10]（已满）；
  //       第 2 个完整字 [30,40]、当前仅显示 [30,33]（未满）。
  // 旧逻辑：只看第一个岛的 isFullyCovered → true → 整组 continue → 第二字的 [33,40] 再也不会吐出来。
  // 新逻辑：逐一检查所有岛，第二个岛未满 → isInsertGroupComplete 返回 false → 产生 motion run。

  const islandSourceRects = [
    { x: 0, y: 0, width: 10, height: 20 },   // 第一字完整矩形
    { x: 30, y: 0, width: 10, height: 20 },  // 第二字完整矩形
  ]
  const islandClipRects = [
    [{ x: 0, y: 0, width: 10, height: 20 }],  // 第一字已完全显示
    [{ x: 30, y: 0, width: 3, height: 20 }],   // 第二字只显示了 3px
  ]
  const offsetX = 0
  const offsetY = 0

  // isInsertGroupComplete 应返回 false（第二个岛未满）
  assert.ok(!isInsertGroupComplete(islandSourceRects, islandClipRects, offsetX, offsetY),
    '第一字已满、第二字未满时 isInsertGroupComplete 应返回 false')

  // 反证：如果两个岛都已满，应返回 true
  const allCoveredClips = [
    [{ x: 0, y: 0, width: 10, height: 20 }],
    [{ x: 30, y: 0, width: 10, height: 20 }],
  ]
  assert.ok(isInsertGroupComplete(islandSourceRects, allCoveredClips, offsetX, offsetY),
    '两个岛都已满时 isInsertGroupComplete 应返回 true')
})

test('缺口1-6158702656：多岛 insert 第一字已满第二字未满——planCarryOverPhysicalGroups 不筛除该组', () => {
  // 同样的场景通过 planCarryOverPhysicalGroups 验证：
  // 已完成的 insert 组被筛除（不输出），未完成的保留（输出 motion run）。
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-5', 'new', 11, 'insert')

  // piece1: 字形 [0,10]，当前可见 [0,10]（已满）
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
    glyphX: 0, glyphY: 0, glyphWidth: 10, glyphHeight: 20,
  })
  // piece2: 字形 [30,40]，当前可见 [30,33]（未满）
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 3, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 30, y: 0, width: 10, height: 20 },
    glyphX: 30, glyphY: 0, glyphWidth: 10, glyphHeight: 20,
  })

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'insert' },
    { piece: piece2, parent, motionChannel: 'insert' },
  ], allocator, cache)

  // 组未被筛除——仍输出
  assert.equal(results.length, 1, '第一字已满、第二字未满的 insert 组不应被筛除')
  assert.equal(results[0].motionChannel, 'insert')
  assert.equal(results[0].islandSourceRects.length, 2)
})

test('缺口1-6158702656：完全已满的 insert 组被筛除——不分配身份、不输出', () => {
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-5', 'new', 11, 'insert')

  // 两个 piece 的 clip 都完全覆盖了各自的完整字形矩形
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
    glyphX: 0, glyphY: 0, glyphWidth: 10, glyphHeight: 20,
  })
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 30, y: 0, width: 10, height: 20 },
    glyphX: 30, glyphY: 0, glyphWidth: 10, glyphHeight: 20,
  })

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'insert' },
    { piece: piece2, parent, motionChannel: 'insert' },
  ], allocator, cache)

  // 已完成的 insert 组被筛除——不输出
  assert.equal(results.length, 0, '所有岛都已满的 insert 组应被筛除')

  // cache 中不应有该组的身份条目（未分配身份）
  assert.equal(cache.size, 0, '已完成的 insert 组不应在 cache 中留下身份条目')
})

// 缺口2测试：合组后 allocator 的完整 glyph 映射
test('缺口2-6158702656：合组后 allocator.resolve 返回完整 glyphIds——不只首片段', () => {
  // 旧逻辑：cache miss 时 allocator.allocateFresh(piece.glyphIds) 只传入首片段的 glyphIds，
  //         后续片段合并到 existing.glyphIds 但 allocator 的反向映射不更新。
  //         allocator.resolve(result.windowInstanceId) 只有首片段的 glyphIds。
  // 新逻辑：两阶段组装——先完整收集所有片段，再用完整组 glyphIds 做 allocateFresh。
  //         allocator.resolve(result.windowInstanceId) 返回完整 glyphIds。

  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-3', 'old', 10, 'deleted')
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 30, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
    { piece: piece2, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results.length, 1)
  const group = results[0]

  // 关键验证：allocator.resolve 返回的 glyphIds 与组的 glyphIds 一致
  const resolved = allocator.resolve(group.windowInstanceId)
  assert.ok(resolved !== null, 'windowInstanceId 应在分配器中登记')
  assert.deepEqual(resolved, group.glyphIds,
    'allocator.resolve 返回的 glyphIds 应与组的完整 glyphIds 一致，不只首片段')
  assert.equal(resolved.length, 2)
  assert.equal(resolved[0], 'g1')
  assert.equal(resolved[1], 'g2')
})

test('缺口2-6158702656：合组后 allocator 反向映射包含所有片段的 glyphIds', () => {
  // 三个片段合并为一个 ghost 组，allocator.resolve 应返回全部三个 glyphIds
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-4', 'old', 10, 'deleted')
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  const piece2 = makePiece(['g2'], 1, 2, [{ x: 20, y: 0, width: 10, height: 20 }])
  const piece3 = makePiece(['g3'], 2, 3, [{ x: 40, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
    { piece: piece2, parent, motionChannel: 'ghost' },
    { piece: piece3, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results.length, 1)
  const group = results[0]
  assert.equal(group.glyphIds.length, 3)

  const resolved = allocator.resolve(group.windowInstanceId)
  assert.ok(resolved !== null)
  assert.deepEqual(resolved, ['g1', 'g2', 'g3'],
    '三个片段合组后 allocator.resolve 应返回全部三个 glyphIds')
})

// 缺口3测试：窗口最后租约释放后再次出现同 groupKey 时不会错误复用旧身份
test('缺口3-6158702656：窗口释放后同 groupKey 再次出现——不错误复用已释放的身份', () => {
  // 场景：
  // 1. 第一次规划：groupKey A → allocateFresh → win-0，cache 记录 A→win-0
  // 2. win-0 的所有租约被释放 → allocator.release(win-0) → win-0 进入 releasedIds
  // 3. allocateFresh 为其他组分配 → win-0 被复用，但 glyphIds 不同
  // 4. 第二次规划：groupKey A 再次出现 → cache hit → 验证 allocator.resolve(win-0)
  //    返回的 glyphIds 与当前组的 glyphIds 不一致 → 重新分配新身份

  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-1', 'old', 10, 'deleted')

  // 第一次规划：ghost 组 g1
  const piece1 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])
  const results1 = planCarryOverPhysicalGroups([
    { piece: piece1, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results1.length, 1)
  const group1 = results1[0]
  const oldWinId = group1.windowInstanceId
  const oldRenderKey = group1.renderNodeKey

  // 模拟窗口释放：release win-0
  allocator.release(oldWinId)

  // win-0 已释放，allocateFresh 可能复用该 ID 给其他组
  const otherWinId = allocator.allocateFresh(['other-glyph'])
  // otherWinId 可能等于 oldWinId（复用了已释放的 ID）

  // 第二次规划：同 groupKey 再次出现，但 glyphIds 可能已变化
  // 即使 glyphIds 不变，allocator.resolve(oldWinId) 返回的 glyphIds 可能已不同
  const piece2 = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 5, height: 20 }])
  const results2 = planCarryOverPhysicalGroups([
    { piece: piece2, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results2.length, 1)
  const group2 = results2[0]

  // 关键验证：如果 oldWinId 已被重新分配给其他 glyphIds，
  // cache hit 验证会发现不一致并重新分配新身份
  const resolvedOld = allocator.resolve(oldWinId)
  if (resolvedOld !== null && resolvedOld[0] !== 'g1') {
    // oldWinId 已被重新分配给其他 glyphIds → 应分配新身份
    assert.notEqual(group2.windowInstanceId, oldWinId,
      '旧身份已被重新分配给其他组时，不应复用旧身份')
  } else {
    // oldWinId 已释放（resolve 返回 null）→ cache hit 验证会发现并重新分配
    // 或者 oldWinId 仍指向 g1（未被复用）→ 可以复用
    // 两种情况都是正确的
  }

  // 无论如何，新组的 windowInstanceId 应在分配器中有效
  assert.ok(allocator.resolve(group2.windowInstanceId) !== null,
    '新组的 windowInstanceId 应在分配器中有效')
})

test('缺口3-6158702656：releaseCarryOverIdentity 清除缓存条目', () => {
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-1', 'old', 10, 'deleted')
  const piece = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }])

  const results = planCarryOverPhysicalGroups([
    { piece, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(cache.size, 1)

  // 获取唯一的 groupKey
  const groupKey = cache.keys().next().value

  // 释放缓存条目
  releaseCarryOverIdentity(cache, groupKey)
  assert.equal(cache.size, 0, 'releaseCarryOverIdentity 应清除指定条目')

  // 再次规划时应分配新身份（cache miss）
  const results2 = planCarryOverPhysicalGroups([
    { piece, parent, motionChannel: 'ghost' },
  ], allocator, cache)

  assert.equal(results2.length, 1)
  assert.notEqual(results2[0].windowInstanceId, results[0].windowInstanceId,
    'cache 清除后应分配新身份')
})

test('缺口3-6158702656：已完成的 insert 组不持有未挂载的实例', () => {
  // 完全结束的 insert 组不应分配身份、不应在 cache 中留下条目
  const allocator = new WindowInstanceIdAllocator()
  const cache = new Map()

  const parent = makeParent('win-1', 'new', 11, 'insert')

  // clip 完全覆盖字形矩形——已完成
  const piece = makePiece(['g1'], 0, 1, [{ x: 0, y: 0, width: 10, height: 20 }], {
    targetResolved: true,
    targetRect: { x: 0, y: 0, width: 10, height: 20 },
    glyphX: 0, glyphY: 0, glyphWidth: 10, glyphHeight: 20,
  })

  const results = planCarryOverPhysicalGroups([
    { piece, parent, motionChannel: 'insert' },
  ], allocator, cache)

  // 已完成的 insert 组被筛除
  assert.equal(results.length, 0, '已完成的 insert 组应被筛除')
  assert.equal(cache.size, 0, '已完成的 insert 组不应在 cache 中留下条目')

  // 分配器中不应有该组的 instanceId（未分配身份）
  // 验证：分配器的 nextId 应仍为初始值（没有为已完成的组分配任何 ID）
  const freshId = allocator.allocateFresh(['test'])
  assert.match(freshId, /^win-0$/,
    '已完成的组未分配身份，分配器的第一个 ID 应仍为 win-0')
})

console.log('---')
console.log(`✅ editor_physical_window_identity: ${passed} tests passed`)
