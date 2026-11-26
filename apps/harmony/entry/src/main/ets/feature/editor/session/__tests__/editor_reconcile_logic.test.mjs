// editor_reconcile_logic.test.mjs — 三方正文合并与冲突会话恢复的纯逻辑回归
// （Issue #1063 复审评论第 4 条）。
//
// 直接 import 生产模块 session/editor_reconcile_logic.ts，不伪造 ArkUI 运行时。
// 验证：
//   1. 单段 delta 可复现输入文本；候选竞态示例（base=甲乙丙、Core=甲乙丁、native=戊乙丙）
//      必须合并成戊乙丁，而不是进入冲突。
//   2. 两侧同一区间不同改法 / 同点不同插入 / 区间重叠 → conflict，保留双方正文与事件。
//   3. Core-only、native-only、converged 分类正确。
//   4. delta 在 Core 改动之前插入时映射右移，在之后时保持原位。
//   5. conflict 不会被自动错误降级成 reconciling；只有 beginManualRetry 能重新尝试。
//   6. 冲突材料版本哨兵永远不可能匹配真实版本（空 sha256）。
//
// 运行：node --experimental-strip-types __tests__/editor_reconcile_logic.test.mjs

import { strict as assert } from 'node:assert'
import {
  applyTextDelta,
  computeTextDelta,
  editorConflictVersionSentinel,
  planThreeWayReconcile,
  ReconcileSessionState,
} from '../editor_reconcile_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_reconcile_logic：三方合并与冲突会话恢复')
console.log('---')

// ── 1. delta 基础 ──
test('computeTextDelta/applyTextDelta 对 ASCII、CRLF、emoji 都能复现输入', () => {
  const cases = [
    ['abcdef', 'abXdef'],
    ['第一段\r\n第二段', '第一段\r\n第二段\n尾段'],
    ['a😀b', 'a😁b'],
    ['abc', 'abc'],
    ['', '新增'],
    ['删除', ''],
  ]
  for (const [base, text] of cases) {
    assert.equal(applyTextDelta(base, computeTextDelta(base, text)), text, `${base} -> ${text}`)
  }
})

// ── 2. 三方合并 ──
test('converged：Core 与原生逐字一致', () => {
  const plan = planThreeWayReconcile('base', 'same', 'same')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'converged')
  assert.equal(plan.merged, 'same')
})

test('core-only：原生仍是已确认基线，Core 先行', () => {
  const plan = planThreeWayReconcile('base', 'core-new', 'base')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'core-only')
  assert.equal(plan.merged, 'core-new')
})

test('native-only：Core 未变，原生有未确认差量', () => {
  const plan = planThreeWayReconcile('base', 'base', 'native-new')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'native-only')
  assert.equal(plan.merged, 'native-new')
})

test('两侧改不同位置：按共同基线合并（甲乙丙/甲乙丁/戊乙丙 → 戊乙丁）', () => {
  const plan = planThreeWayReconcile('甲乙丙', '甲乙丁', '戊乙丙')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'merged')
  assert.equal(plan.merged, '戊乙丁')
})

test('两侧同一区间同一改法：已是同一正文，按 converged 收敛', () => {
  const plan = planThreeWayReconcile('abcdef', 'abXdef', 'abXdef')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'converged')
  assert.equal(plan.merged, 'abXdef')
})

test('两侧同一区间不同改法：conflict', () => {
  const plan = planThreeWayReconcile('abcdef', 'abXdef', 'abYdef')
  assert.equal(plan.ok, false)
  assert.equal(plan.kind, 'conflict')
  assert.equal(plan.merged, 'abXdef')
})

test('同一点不同插入：conflict，不猜顺序', () => {
  const plan = planThreeWayReconcile('abcdef', 'abXcdef', 'abYcdef')
  assert.equal(plan.ok, false)
  assert.equal(plan.kind, 'conflict')
})

test('Core 改区间包含原生改动：conflict', () => {
  const plan = planThreeWayReconcile('abcdef', 'aXYef', 'abZdef')
  assert.equal(plan.ok, false)
  assert.equal(plan.kind, 'conflict')
})

test('原生在 Core 改区间之前插入：映射到 Core 后右移', () => {
  const plan = planThreeWayReconcile('abcdef', 'aXbcdef', 'aYZdef')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'merged')
  // core 在 1 插入 X（+1），native 的 [1,3) 映射到 core 的 [2,4)。
  assert.equal(plan.merged, 'aXYZdef')
})

test('原生在 Core 改区间之后插入：位置不变', () => {
  const plan = planThreeWayReconcile('abcdef', 'abXdef', 'abcZdef')
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'merged')
  assert.equal(plan.merged, 'abXZdef')
})

test('合并结果同时包含双方编辑；emoji 相邻编辑不产生非法拼接', () => {
  const base = 'a😀b'
  const core = 'a😁b'
  const native = 'a😀Zb'
  const plan = planThreeWayReconcile(base, core, native)
  assert.equal(plan.ok, true)
  assert.equal(plan.kind, 'merged')
  assert.equal(plan.merged, 'a😁Zb')
})

// ── 3. 冲突会话状态机 ──
test('conflict 不会被自动错误降级成 reconciling', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlap')
  assert.equal(state.isConflict(), true)
  assert.equal(state.enterReconciling('retry'), false)
  assert.equal(state.getHealth(), 'conflict')
  assert.equal(state.getConflictDetail(), 'overlap')
})

test('只有 beginManualRetry 能重新尝试；成功后清 conflict', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlap')
  assert.equal(state.beginManualRetry('manual'), true)
  assert.equal(state.getHealth(), 'reconciling')
  // 重试期间保留原冲突原因供诊断。
  assert.equal(state.getConflictDetail(), 'overlap')
  state.confirmHealthy()
  assert.equal(state.getHealth(), 'healthy')
  assert.equal(state.getConflictDetail(), null)
})

// ── 4. 冲突材料版本哨兵 ──
test('冲突材料版本哨兵永远不可能匹配真实版本', () => {
  const sentinel = editorConflictVersionSentinel()
  assert.ok(sentinel.objectGeneration.length > 0)
  assert.equal(sentinel.contentSha256, '')
  assert.ok(sentinel.revision >= 0)
  // Core 的 expected_matches 要求 content_sha256/content_sha256 非空；
  // 空 sha 保证走冲突材料分支而不是覆盖正文。
  assert.notEqual(sentinel.contentSha256.length, 64)
})

console.log('---')
console.log(`✅ editor_reconcile_logic: ${passed} tests passed`)
