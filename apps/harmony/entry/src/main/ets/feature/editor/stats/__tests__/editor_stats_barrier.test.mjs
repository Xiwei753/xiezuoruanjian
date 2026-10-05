// editor_stats_barrier.test.mjs — 写作统计上报队列的序号账本（Issue #829 评论 #5996577737 第 2 项）。
//
// 验证：
//   1. 成功路径：flushThrough(barrier) 在 barrier 以内全部成功时返回 true。
//   2. 失败不被后来的 flush 抹掉：第 1 条失败，之后再 flushThrough(同一 barrier) 仍返回 false。
//      （这正是上一版 flushFailures 在 flush 开头清零导致的漏报。）
//   3. barrier 之后的失败不影响更早的 barrier：第 2 条失败，flushThrough(1) 仍返回 true。
//      （保存只该关心自己那份正文对应的统计。）
//   4. 丢弃按不干净记账：dropped(seq) 之后 flushThrough 越过它返回 false。
//   5. 完成序号单调推进，失败也推进 —— 否则 flushThrough 会等一条永远等不到的 barrier。
//      完成序号是单调的（串行 drain 保证按序处理），所以越早的 barrier 一定被更晚的覆盖。
//
// 运行：node --experimental-strip-types editor_stats_barrier.test.mjs

import { strict as assert } from 'node:assert'
import { StatsBarrierLedger } from '../editor_stats_barrier.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_stats_barrier 序号账本测试（Issue #829 评论 #5996577737 第 2 项）')
console.log('---')

test('成功路径：flushThrough(barrier) 返回 true', () => {
  const l = new StatsBarrierLedger()
  const s1 = l.nextSeq()
  const s2 = l.nextSeq()
  l.complete(s1, true)
  l.complete(s2, true)
  assert.equal(l.currentSeq(), 2)
  assert.equal(l.isCompleteThrough(2), true)
  assert.equal(l.isCleanThrough(2), true)
})

test('失败不被后来的 flush 抹掉（上一版 flushFailures 清零导致的漏报）', () => {
  const l = new StatsBarrierLedger()
  const s1 = l.nextSeq()
  l.complete(s1, false)
  // 第一次调用看到失败
  assert.equal(l.isCleanThrough(s1), false)
  // 之后「又 flush 了一次」，失败不能被抹掉
  assert.equal(l.isCleanThrough(s1), false)
})

test('barrier 之后的失败不影响更早的 barrier', () => {
  const l = new StatsBarrierLedger()
  const s1 = l.nextSeq()
  const s2 = l.nextSeq()
  l.complete(s1, true)
  l.complete(s2, false)
  // 第 1 条干净：保存第 1 条正文时不该看到第 2 条的失败
  assert.equal(l.isCleanThrough(s1), true)
  // 第 2 条不干净
  assert.equal(l.isCleanThrough(s2), false)
})

test('丢弃按不干净记账', () => {
  const l = new StatsBarrierLedger()
  const s1 = l.nextSeq()
  const s2 = l.nextSeq()
  l.complete(s1, true)
  l.dropped(s2)
  assert.equal(l.isCleanThrough(s1), true)
  assert.equal(l.isCleanThrough(s2), false)
})

test('完成序号单调推进，失败也推进', () => {
  const l = new StatsBarrierLedger()
  const s1 = l.nextSeq()
  const s2 = l.nextSeq()
  // 还没处理任何一条
  assert.equal(l.isCompleteThrough(s2), false)
  l.complete(s1, true)
  // 只处理到第 1 条，第 2 条 barrier 还没到
  assert.equal(l.isCompleteThrough(s2), false)
  // 失败的第 2 条也算处理完：flushThrough 不会死等一条永远等不到的 barrier
  l.complete(s2, false)
  assert.equal(l.isCompleteThrough(s2), true)
  // 完成序号单调：越早的 barrier 一定被覆盖
  assert.equal(l.isCompleteThrough(s1), true)
  // 但干净与否分开判：第 2 条失败，第 1 条仍然干净
  assert.equal(l.isCleanThrough(s1), true)
  assert.equal(l.isCleanThrough(s2), false)
})

console.log('---')
console.log(`${passed} tests passed`)
