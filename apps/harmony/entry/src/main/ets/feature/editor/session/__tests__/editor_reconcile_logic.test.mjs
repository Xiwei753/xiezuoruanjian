// editor_reconcile_logic.test.mjs — 文本差量与冲突会话状态机的纯逻辑回归
// （Issue #1063 复审评论第 2、3 条）。
//
// 三方合并与 pendingEdits 逐笔重基已迁到 editor_merge_plan.ts（由
// editor_merge_plan.test.mjs 覆盖）；本文件只验证剩下的公共契约：
//   1. 单段 delta 可复现 ASCII/CRLF/emoji/空串，插入/删除/替换的区间形状正确；
//   2. 差量边界落在 UTF-16 代理对整体上，不产生半个 emoji；
//   3. conflict 不会被自动错误降级成 reconciling，只有 beginManualRetry 能重新尝试；
//   4. 屏障冲突恢复判定只认「真正恢复」或「本来就不在冲突中」。
//
// 运行：node --experimental-strip-types feature/editor/session/__tests__/editor_reconcile_logic.test.mjs

import { strict as assert } from 'node:assert'
import {
  applyTextDelta,
  computeTextDelta,
  isConflictRecoverySatisfied,
  ReconcileSessionState,
} from '../editor_reconcile_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 代理对完整性：不允许出现孤立的高/低代理（半个 emoji）。
function assertWellFormed(text, label) {
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = text.charCodeAt(i + 1)
      assert.ok(next >= 0xdc00 && next <= 0xdfff, `${label}: lone high surrogate at ${i}`)
      i += 1
    } else {
      assert.ok(code < 0xdc00 || code > 0xdfff, `${label}: lone low surrogate at ${i}`)
    }
  }
}

console.log('editor_reconcile_logic：文本差量与冲突会话状态机')
console.log('---')

// ── 1. delta 基础 ──
test('computeTextDelta/applyTextDelta 对 ASCII、CRLF、emoji、空串都能复现输入', () => {
  const cases = [
    ['abcdef', 'abXdef'],
    ['第一段\r\n第二段', '第一段\r\n第二段\n尾段'],
    ['a😀b', 'a😁b'],
    ['abc', 'abc'],
    ['', '新增'],
    ['删除', ''],
    ['前😀后', '前😀😀后'],
  ]
  for (const [base, text] of cases) {
    const delta = computeTextDelta(base, text)
    assert.equal(applyTextDelta(base, delta), text, `${base} -> ${text}`)
    assert.ok(delta.start >= 0 && delta.end >= delta.start, 'delta range must be ordered')
    assert.ok(delta.end <= base.length, 'delta end must stay in base')
  }
})

test('纯插入：start===end，replacement 是插入文本', () => {
  const middle = computeTextDelta('abcd', 'abXcd')
  assert.equal(middle.start, 2)
  assert.equal(middle.end, 2)
  assert.equal(middle.replacement, 'X')

  const append = computeTextDelta('abc', 'abcX')
  assert.equal(append.start, 3)
  assert.equal(append.end, 3)
  assert.equal(append.replacement, 'X')

  const prepend = computeTextDelta('abc', 'Xabc')
  assert.equal(prepend.start, 0)
  assert.equal(prepend.end, 0)
  assert.equal(prepend.replacement, 'X')
})

test('纯删除：replacement 为空，区间是删除范围', () => {
  const middle = computeTextDelta('abcd', 'acd')
  assert.equal(middle.start, 1)
  assert.equal(middle.end, 2)
  assert.equal(middle.replacement, '')

  const all = computeTextDelta('abc', '')
  assert.equal(all.start, 0)
  assert.equal(all.end, 3)
  assert.equal(all.replacement, '')
})

test('替换：区间同时覆盖旧文本，replacement 是新文本', () => {
  const delta = computeTextDelta('abcdef', 'abXdef')
  assert.equal(delta.start, 2)
  assert.equal(delta.end, 3)
  assert.equal(delta.replacement, 'X')
  assert.equal('abcdef'.substring(delta.start, delta.end), 'c')
})

test('相同文本：空区间且无替换', () => {
  const delta = computeTextDelta('abc', 'abc')
  assert.equal(delta.start, 3)
  assert.equal(delta.end, 3)
  assert.equal(delta.replacement, '')
  assert.equal(applyTextDelta('abc', delta), 'abc')
})

// ── 2. 代理对边界 ──
test('emoji 替换：差量边界包住整颗代理对，不切在 😀 中间', () => {
  const base = 'a😀b'
  const delta = computeTextDelta(base, 'a😁b')
  // 😀 占 base[1..3)；delta 必须整段替换它，不能只换半个 code unit。
  assert.equal(delta.start, 1)
  assert.equal(delta.end, 3)
  assert.equal(delta.replacement, '😁')
  const high = base.charCodeAt(delta.start)
  const low = base.charCodeAt(delta.end)
  assert.ok(high >= 0xd800 && high <= 0xdbff, 'start must be at the high surrogate')
  assert.ok(low >= 0xdc00 && low <= 0xdfff, 'end must be after the low surrogate')
  assertWellFormed(applyTextDelta(base, delta), 'replaced')
})

test('emoji 相邻插入：插入点落在代理对之后，结果仍是完整 emoji', () => {
  const base = 'a😀b'
  const delta = computeTextDelta(base, 'a😀😀b')
  assert.equal(delta.start, 3)
  assert.equal(delta.end, 3)
  assert.equal(delta.replacement, '😀')
  assertWellFormed(applyTextDelta(base, delta), 'inserted')
})

test('emoji 删除：删除整颗 emoji 的区间不拆代理对', () => {
  const base = 'a😀b'
  const delta = computeTextDelta(base, 'ab')
  assert.equal(delta.start, 1)
  assert.equal(delta.end, 3)
  assert.equal(delta.replacement, '')
  assertWellFormed(applyTextDelta(base, delta), 'deleted')
})

// ── 3. 冲突会话状态机 ──
test('初始为 healthy；普通错误可进入 reconciling', () => {
  const state = new ReconcileSessionState()
  assert.equal(state.getHealth(), 'healthy')
  assert.equal(state.enterReconciling('drain baseline advanced'), true)
  assert.equal(state.getHealth(), 'reconciling')
  assert.equal(state.getReconcileReason(), 'drain baseline advanced')
  assert.equal(state.getConflictDetail(), null)
})

test('conflict 不会被自动错误降级成 reconciling', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlap')
  assert.equal(state.isConflict(), true)
  assert.equal(state.enterReconciling('retry'), false)
  assert.equal(state.getHealth(), 'conflict')
  assert.equal(state.getConflictDetail(), 'overlap')
})

test('只有 beginManualRetry 能重新尝试；重试期间保留原冲突原因', () => {
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

test('非 conflict 状态下 beginManualRetry 不会伪造重试', () => {
  const state = new ReconcileSessionState()
  assert.equal(state.beginManualRetry('manual'), false)
  assert.equal(state.getHealth(), 'healthy')
  state.enterReconciling('drain')
  assert.equal(state.beginManualRetry('manual'), false)
  assert.equal(state.getHealth(), 'reconciling')
})

test('reset 是会话替换语义：清空 conflict/reconciling 与诊断原因', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlap')
  state.reset()
  assert.equal(state.getHealth(), 'healthy')
  assert.equal(state.getConflictDetail(), null)
  assert.equal(state.getReconcileReason(), '')
  assert.equal(state.isConflict(), false)
})

// ── 4. 屏障冲突恢复判定 ──
test('isConflictRecoverySatisfied：null/已恢复/NOT_IN_CONFLICT 放行，其余失败', () => {
  assert.equal(isConflictRecoverySatisfied(null), true)
  assert.equal(isConflictRecoverySatisfied({
    recovered: true, errorCode: '', detail: ''
  }), true)
  assert.equal(isConflictRecoverySatisfied({
    recovered: false, errorCode: 'NOT_IN_CONFLICT', detail: 'healthy'
  }), true)
  assert.equal(isConflictRecoverySatisfied({
    recovered: false, errorCode: 'RECONCILE_DEFERRED', detail: 'ime in flight'
  }), false)
  assert.equal(isConflictRecoverySatisfied({
    recovered: false, errorCode: 'IME_SETTLEMENT_CONFLICT', detail: 'overlap'
  }), false)
  assert.equal(isConflictRecoverySatisfied({
    recovered: false, errorCode: 'IME_SETTLEMENT_SESSION_CHANGED', detail: 'session changed'
  }), false)
})

console.log('---')
console.log(`✅ editor_reconcile_logic: ${passed} tests passed`)
