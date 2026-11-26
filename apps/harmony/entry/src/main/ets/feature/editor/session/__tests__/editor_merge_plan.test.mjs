// editor_merge_plan.test.mjs — pendingEdits 逐笔重基的纯逻辑回归
// （Issue #1063 复审评论第 2、3 条）。
//
// 直接 import 生产模块 session/editor_merge_plan.ts（它内部再 import 真实的
// editor_reconcile_logic.ts 的 computeTextDelta / applyTextDelta），不写镜像实现。
// 验证：
//   1. Core 未前进时三笔编辑（追加、中间插入、删除）逐笔映射，resultText===nativeText；
//   2. 复审示例 base=甲乙丙 / Core=戊乙丙 / 原生=甲乙丁 映射成戊乙丁；
//   3. 两笔离散编辑 + Core 在另一位置前进：两笔都保留、顺序不变；
//   4. Core 改动与编辑区间重叠 → ok:false（reason 含两端区间），不猜测；
//   5. beforeText 不接链 / oldText 不匹配 / nativeText 与模拟结果不等 / 代理对中间位置 → ok:false；
//   6. Core 在编辑之前插入/删除时，编辑区间按净长度右移/左移；
//   7. 空 base、空 native（删除全部）、emoji 编辑不拆代理对；
//   8. 无 pendingEdits 却有原生差量 → ok:false 'unmirrored native delta'。
//
// 运行：node --experimental-strip-types feature/editor/session/__tests__/editor_merge_plan.test.mjs

import { strict as assert } from 'node:assert'
import { applyTextDelta } from '../editor_reconcile_logic.ts'
import { planPendingEditRebase } from '../editor_merge_plan.ts'

let passed = 0
let failed = 0
const test = (name, fn) => {
  try {
    fn()
    passed++
    console.log(`  [PASS] ${name}`)
  } catch (error) {
    failed++
    console.error(`  [FAIL] ${name}`)
    console.error(error instanceof Error ? error.message : String(error))
  }
}

// 按队列顺序把 mapped edits 提交到 Core 正文（mapped 坐标是「逐笔提交时」的坐标）。
function applyMapped(coreText, mapped) {
  let text = coreText
  for (const edit of mapped) {
    text = text.slice(0, edit.startUtf16) + edit.insertedText + text.slice(edit.endUtf16)
  }
  return text
}

// 失败结果的公共契约：不提交任何事务、resultText 保持 Core 当前正文、coreDelta 仍可复现 Core。
function assertRejected(plan, baseText, coreText) {
  assert.equal(plan.ok, false, 'plan.ok must be false')
  assert.ok(plan.reason.length > 0, 'rejected plan must carry a reason')
  assert.equal(plan.edits.length, 0, 'rejected plan must not expose any commit table')
  assert.equal(plan.resultText, coreText, 'rejected plan must keep current core text')
  assert.equal(applyTextDelta(baseText, plan.coreDelta), coreText)
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

console.log('editor_merge_plan：pendingEdits 逐笔重基到已前进的 Core 正文')
console.log('---')

// ── 1. Core 未前进：多笔编辑逐笔映射 ──
test('Core 未前进：追加、中间插入、删除三笔编辑逐笔映射且 resultText===nativeText', () => {
  // 原生链：hello world → hello world! → hello beautiful world! → hello beautiful!
  const base = 'hello world'
  const native = 'hello beautiful!'
  const edits = [
    { queueSequence: 1, beforeText: 'hello world', startUtf16: 11, endUtf16: 11, insertedText: '!', oldText: '' },
    {
      queueSequence: 2, beforeText: 'hello world!', startUtf16: 5, endUtf16: 5,
      insertedText: ' beautiful', oldText: ''
    },
    {
      queueSequence: 3, beforeText: 'hello beautiful world!', startUtf16: 15, endUtf16: 21,
      insertedText: '', oldText: ' world'
    },
  ]
  const plan = planPendingEditRebase(base, base, native, edits)
  assert.equal(plan.ok, true)
  assert.equal(plan.reason, '')
  assert.equal(plan.edits.length, 3, '每笔编辑都要保留，不能被压成一段')
  assert.deepEqual(plan.edits.map((e) => e.queueSequence), [1, 2, 3], '映射顺序必须等于队列顺序')
  assert.deepEqual(
    plan.edits.map((e) => [e.startUtf16, e.endUtf16]),
    [[11, 11], [5, 5], [15, 21]]
  )
  assert.equal(plan.resultText, native)
  assert.equal(applyMapped(base, plan.edits), native)
  // Core 未前进时 coreDelta 是空区间：start===end 且 replacement===''。
  assert.equal(plan.coreDelta.start, base.length)
  assert.equal(plan.coreDelta.end, base.length)
  assert.equal(plan.coreDelta.replacement, '')
})

// ── 2. 复审示例 ──
// 复审评论里的例子是「共同基线 甲乙丙，Core 戊乙丙，原生 甲乙丁」：原生把尾字 丙 换成 丁，
// 映射到 Core 后区间仍是 [2, 3)，结果是 戊乙丁。
test('复审示例：Core 改首字、原生把尾字改成丁 → 映射到 Core [2,3)，结果戊乙丁', () => {
  const plan = planPendingEditRebase('甲乙丙', '戊乙丙', '甲乙丁', [
    { queueSequence: 11, beforeText: '甲乙丙', startUtf16: 2, endUtf16: 3, insertedText: '丁', oldText: '丙' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.resultText, '戊乙丁')
  assert.equal(plan.edits.length, 1)
  assert.equal(plan.edits[0].queueSequence, 11)
  assert.equal(plan.edits[0].startUtf16, 2)
  assert.equal(plan.edits[0].endUtf16, 3)
  assert.equal(applyMapped('戊乙丙', plan.edits), '戊乙丁')
})

// 同一基线上的纯插入（丁 插在 乙 与 丙 之间）映射到 Core 的 start=end=2。
test('复审示例的纯插入变体：同点插入映射到 Core start=end=2', () => {
  const plan = planPendingEditRebase('甲乙丙', '戊乙丙', '甲乙丁丙', [
    { queueSequence: 12, beforeText: '甲乙丙', startUtf16: 2, endUtf16: 2, insertedText: '丁', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits.length, 1)
  assert.equal(plan.edits[0].startUtf16, 2)
  assert.equal(plan.edits[0].endUtf16, 2)
  assert.equal(plan.resultText, '戊乙丁丙')
})

// 真正的「追加 丁」是 [3,3) 插入：它映射到 Core 末尾 [3,3)，Core 的尾字 丙 保留，
// 结果是 戊乙丙丁 —— 与上面「尾字被替换」是两笔不同的事实，不能混为一谈。
test('真正的 append（丁 追加到末尾）映射到 Core start=end=3，结果是戊乙丙丁', () => {
  const plan = planPendingEditRebase('甲乙丙', '戊乙丙', '甲乙丙丁', [
    { queueSequence: 13, beforeText: '甲乙丙', startUtf16: 3, endUtf16: 3, insertedText: '丁', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits[0].startUtf16, 3)
  assert.equal(plan.edits[0].endUtf16, 3)
  assert.equal(plan.resultText, '戊乙丙丁')
})

// ── 3. 两笔离散编辑 + Core 在另一位置前进 ──
test('两笔离散编辑 + Core 在前部另一位置改动：两笔都保留、顺序不变、结果同时含三方改动', () => {
  // Core：首字 a→A；原生：b→B（前部）、删除 f（后部）。两者互不重叠。
  const base = 'abcdefg'
  const core = 'Abcdefg'
  const native = 'aBcdeg'
  const edits = [
    { queueSequence: 1, beforeText: 'abcdefg', startUtf16: 1, endUtf16: 2, insertedText: 'B', oldText: 'b' },
    { queueSequence: 2, beforeText: 'aBcdefg', startUtf16: 5, endUtf16: 6, insertedText: '', oldText: 'f' },
  ]
  const plan = planPendingEditRebase(base, core, native, edits)
  assert.equal(plan.ok, true)
  assert.equal(plan.edits.length, 2)
  assert.deepEqual(plan.edits.map((e) => e.queueSequence), [1, 2])
  assert.equal(plan.edits[0].startUtf16, 1)
  assert.equal(plan.edits[1].startUtf16, 5)
  assert.equal(plan.resultText, 'ABcdeg')
  // Core 的 A、第一笔的 B 都在，第二笔删掉的 f 不在。
  assert.ok(plan.resultText.startsWith('A'), 'Core 改动必须保留')
  assert.ok(plan.resultText.includes('B'), '第一笔编辑必须保留')
  assert.ok(!plan.resultText.includes('f'), '第二笔删除必须生效')
  assert.equal(applyMapped(core, plan.edits), 'ABcdeg')
})

// ── 4. 重叠 → 失败 ──
test('Core 改动与原生编辑区间重叠 → ok:false，reason 含两端区间', () => {
  // Core 改 [2,4)；原生改 [3,5)：区间相交，谁先谁后没有事实依据。
  const plan = planPendingEditRebase('abcdef', 'abXYef', 'abcZf', [
    { queueSequence: 1, beforeText: 'abcdef', startUtf16: 3, endUtf16: 5, insertedText: 'Z', oldText: 'de' },
  ])
  assertRejected(plan, 'abcdef', 'abXYef')
  assert.match(plan.reason, /\[3, 5\)/)
  assert.match(plan.reason, /\[2, 4\)/)
})

// ── 5. 各类不合法输入 → 失败 ──
test('beforeText 不接链 → ok:false', () => {
  const plan = planPendingEditRebase('abc', 'abc', 'Xabc', [
    { queueSequence: 1, beforeText: 'abd', startUtf16: 0, endUtf16: 0, insertedText: 'X', oldText: '' },
  ])
  assertRejected(plan, 'abc', 'abc')
  assert.match(plan.reason, /beforeText/)
})

test('oldText 与区间内容不匹配 → ok:false', () => {
  const plan = planPendingEditRebase('abc', 'abc', 'aXc', [
    { queueSequence: 1, beforeText: 'abc', startUtf16: 1, endUtf16: 2, insertedText: 'X', oldText: 'z' },
  ])
  assertRejected(plan, 'abc', 'abc')
  assert.match(plan.reason, /oldText/)
})

test('nativeText 与模拟结果不等（链条不完整）→ ok:false', () => {
  const plan = planPendingEditRebase('abc', 'abc', 'abcX', [
    { queueSequence: 1, beforeText: 'abc', startUtf16: 0, endUtf16: 0, insertedText: 'X', oldText: '' },
  ])
  assertRejected(plan, 'abc', 'abc')
  assert.match(plan.reason, /native text does not match/)
})

test('UTF-16 代理对中间位置 → ok:false（区间端点都不能落在半个 emoji 上）', () => {
  // a😀b 的 code unit：[0]=a、[1]=高代理、[2]=低代理、[3]=b；位置 2 落在 😀 中间。
  const middleStart = planPendingEditRebase('a😀b', 'a😀b', 'a😀Xb', [
    { queueSequence: 1, beforeText: 'a😀b', startUtf16: 2, endUtf16: 2, insertedText: 'X', oldText: '' },
  ])
  assertRejected(middleStart, 'a😀b', 'a😀b')
  assert.match(middleStart.reason, /surrogate/)
  // 区间终点切在代理对中间同样必须失败。
  const middleEnd = planPendingEditRebase('a😀b', 'a😀b', 'aXb', [
    { queueSequence: 1, beforeText: 'a😀b', startUtf16: 1, endUtf16: 2, insertedText: 'X', oldText: '😀' },
  ])
  assertRejected(middleEnd, 'a😀b', 'a😀b')
  assert.match(middleEnd.reason, /surrogate/)
})

// ── 6. Core 改动在编辑之前：位置按净长度平移 ──
test('Core 在前部插入：之后的原生插入右移（+1）', () => {
  const plan = planPendingEditRebase('abcdef', 'abcXdef', 'abcdeYf', [
    { queueSequence: 1, beforeText: 'abcdef', startUtf16: 5, endUtf16: 5, insertedText: 'Y', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits[0].startUtf16, 6)
  assert.equal(plan.edits[0].endUtf16, 6)
  assert.equal(plan.resultText, 'abcXdeYf')
  assert.equal(applyMapped('abcXdef', plan.edits), 'abcXdeYf')
})

test('Core 在前部删除：之后的原生插入左移（-1）', () => {
  const plan = planPendingEditRebase('abcdefg', 'acdefg', 'abcdeZfg', [
    { queueSequence: 1, beforeText: 'abcdefg', startUtf16: 5, endUtf16: 5, insertedText: 'Z', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits[0].startUtf16, 4)
  assert.equal(plan.edits[0].endUtf16, 4)
  assert.equal(plan.resultText, 'acdeZfg')
  assert.equal(applyMapped('acdefg', plan.edits), 'acdeZfg')
})

test('Core 在前部插入：之后的原生删除区间右移（+2）', () => {
  const plan = planPendingEditRebase('abcdefg', 'abXXcdefg', 'abcdeg', [
    { queueSequence: 1, beforeText: 'abcdefg', startUtf16: 5, endUtf16: 6, insertedText: '', oldText: 'f' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits[0].startUtf16, 7)
  assert.equal(plan.edits[0].endUtf16, 8)
  assert.equal(plan.resultText, 'abXXcdeg')
  assert.equal(applyMapped('abXXcdefg', plan.edits), 'abXXcdeg')
})

// ── 7. 边界文本 ──
test('空 base：整篇新建按队列顺序映射到 Core（不崩、不越界）', () => {
  const plan = planPendingEditRebase('', '核心', 'abc', [
    { queueSequence: 1, beforeText: '', startUtf16: 0, endUtf16: 0, insertedText: 'abc', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits.length, 1)
  assert.equal(plan.edits[0].startUtf16, 0)
  assert.equal(plan.edits[0].endUtf16, 0)
  assert.equal(plan.resultText, 'abc核心')
  assert.equal(applyMapped('核心', plan.edits), 'abc核心')
})

test('空 native：删除全部成长度 0', () => {
  const plan = planPendingEditRebase('abc', 'abc', '', [
    { queueSequence: 1, beforeText: 'abc', startUtf16: 0, endUtf16: 3, insertedText: '', oldText: 'abc' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits.length, 1)
  assert.equal(plan.edits[0].startUtf16, 0)
  assert.equal(plan.edits[0].endUtf16, 3)
  assert.equal(plan.resultText, '')
})

test('emoji 编辑：插入整颗 😀，结果与所有 mapped 内容都不拆代理对', () => {
  const plan = planPendingEditRebase('a😀b', 'a😀B', 'a😀😀b', [
    { queueSequence: 1, beforeText: 'a😀b', startUtf16: 3, endUtf16: 3, insertedText: '😀', oldText: '' },
  ])
  assert.equal(plan.ok, true)
  assert.equal(plan.edits.length, 1)
  assert.equal(plan.edits[0].startUtf16, 3)
  assert.equal(plan.edits[0].endUtf16, 3)
  assert.equal(plan.resultText, 'a😀😀B')
  assert.equal(applyMapped('a😀B', plan.edits), 'a😀😀B')
  assertWellFormed(plan.resultText, 'resultText')
  for (const edit of plan.edits) {
    assertWellFormed(edit.insertedText, 'insertedText')
    assertWellFormed(edit.oldText, 'oldText')
  }
})

// ── 8. 没有原始编辑事实 ──
test('无 pendingEdits 且原生有差量 → ok:false unmirrored native delta', () => {
  const plan = planPendingEditRebase('base', 'core-new', 'base-changed', [])
  assert.equal(plan.ok, false)
  assert.equal(plan.reason, 'unmirrored native delta')
  assert.equal(plan.edits.length, 0)
  assert.equal(plan.resultText, 'core-new')
})

test('无 pendingEdits 且原生仍等于基线：Core 前进照常返回可提交结果', () => {
  const plan = planPendingEditRebase('base', 'core-new', 'base', [])
  assert.equal(plan.ok, true)
  assert.equal(plan.reason, '')
  assert.equal(plan.edits.length, 0)
  assert.equal(plan.resultText, 'core-new')
  assert.equal(applyTextDelta('base', plan.coreDelta), 'core-new')
})

test('同一点零长度插入（Core 插丙、原生插丁）必须明确冲突', () => {
  // Issue #1063 复审评论 #7194530106：base=甲乙、Core=甲丙乙、native=甲丁乙，
  // 两侧都在 UTF-16 位置 1 做零长度插入；没有共同事务身份，不能默认丁在丙前。
  const plan = planPendingEditRebase('甲乙', '甲丙乙', '甲丁乙', [
    { queueSequence: 1, beforeText: '甲乙', startUtf16: 1, endUtf16: 1, insertedText: '丁', oldText: '' },
  ])
  assert.equal(plan.ok, false)
  assert.match(plan.reason, /same point/)
  assert.equal(plan.edits.length, 0)
  assert.equal(plan.resultText, '甲丙乙')
})

console.log('---')
console.log(`✅ editor_merge_plan: ${passed} passed, ${failed} failed`)
if (failed > 0) {
  process.exitCode = 1
}
