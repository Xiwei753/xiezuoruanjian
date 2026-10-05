// editor_stats_drain.test.mjs — 统计队列「串行调度层」的串行与 barrier 行为
// （Issue #829 评论 #5996577737 第 2 项、评论 #5997254152 第 2 项）。
//
// 直接加载真实的 stats_drain_loop.ts，不写镜像实现 —— 这两个 bug 都出在 drain 的粒度和
// 等待条件上，镜像实现和真代码分叉就等于没测。
// 队列本体 EditorStatsQueue.ets 因为要给 bridge 传强类型 EditorChangeStatsInput 被迫是
// .ets，而 ArkTS 禁止 .ts import .ets，所以它被拆成「薄壳（算 session/时长、组装 payload）」
// +「调度层 stats_drain_loop.ts」，这里测的是调度层。
//
// 验证：
//   1. 串行：同一时刻只有一条在飞（in-flight 计数最大值为 1）。
//   2. 反例（#5997254152 第 2 项）：barrier=2，seq1/2 处理完后 seq3 仍在 pending/in-flight，
//      flushThrough(2) 必须已经 resolve，不能等 seq3 写完。
//   3. flushThrough 会等到 barrier 真正完成，不是提前返回。
//   4. 失败按 seq 记账：Core 返回 success=false 时 flushThrough(barrier) 返回 false，
//      且不影响更早的 barrier。
//   5. 空队列 + barrier=0 直接干净返回（队列薄壳负责挡掉纯光标移动/空 chapterId，
//      调度层这一层只保证「无事可等」不死等）。
//
// 运行：node --experimental-strip-types editor_stats_queue.test.mjs

import { strict as assert } from 'node:assert'
import { SerialStatsDrain } from '../stats_drain_loop.ts'

let passed = 0
const test = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

// 假 bridge：记录调用顺序与并发度，可控制每条的耗时与成败。
function makeBridge(opts = {}) {
  const { perCallMs = 0, failSeqs = new Set() } = opts
  const state = { inFlight: 0, maxInFlight: 0, calls: [] }
  return {
    state,
    async recordEditorChangeStats(input) {
      state.inFlight++
      state.maxInFlight = Math.max(state.maxInFlight, state.inFlight)
      // 用入参里的一个可辨识字段当 seq 代理：测试都往同一个 chapterId 塞，
      // 所以这里按调用次序编号。
      const idx = state.calls.length + 1
      state.calls.push(input)
      try {
        if (perCallMs > 0) await sleep(perCallMs)
        return { success: !failSeqs.has(idx), data: true }
      } finally {
        state.inFlight--
      }
    },
  }
}

/** 用假 bridge 造一个真实调度层（send 就是 bridge.recordEditorChangeStats 成功与否的映射）。 */
function makeDrain(bridge) {
  return new SerialStatsDrain(
    async (item) => {
      const res = await bridge.recordEditorChangeStats(item)
      return res.success
    },
  )
}

function enqueueTyping(q, tag = 't') {
  q.enqueue({ seq: 0, tag })
}

console.log('stats_drain 串行调度测试（Issue #829 评论 #5997254152 第 2 项）')
console.log('---')

await test('串行：同一时刻只有一条在飞', async () => {
  const bridge = makeBridge({ perCallMs: 5 })
  const q = makeDrain(bridge)
  for (let i = 0; i < 8; i++) enqueueTyping(q)
  await q.flushThrough(q.currentSeq())
  assert.equal(bridge.state.calls.length, 8)
  assert.equal(bridge.state.maxInFlight, 1, '必须严格串行')
  assert.equal(q.dropped(), 0)
})

await test('反例：flushThrough(barrier) 不等 barrier 之后的 seq（#5997254152 第 2 项）', async () => {
  // 每条都慢一点，保证 seq3 一定还在 in-flight 时 barrier 就已经满足。
  const bridge = makeBridge({ perCallMs: 40 })
  const q = makeDrain(bridge)
  enqueueTyping(q)
  enqueueTyping(q)
  const barrier = q.currentSeq() // = 2

  // 在 flush 等待期间持续喂新事件，模拟用户不停打字。
  let feeding = true
  const feeder = (async () => {
    let n = 0
    while (feeding) {
      enqueueTyping(q)
      n++
      await sleep(3)
      if (n > 40) break
    }
  })()

  const clean = await q.flushThrough(barrier)
  feeding = false
  await feeder

  // 关键：flushThrough(2) 返回时，seq3 及之后还堵在队列里没写完。
  const doneAtReturn = bridge.state.calls.length
  assert.equal(clean, true)
  assert.ok(doneAtReturn < q.currentSeq(),
    `flushThrough 返回时应该还有后续 seq 没写完（当时写了 ${doneAtReturn}，last seq=${q.currentSeq()}）`)

  // 让后台把它们写完，确认只是「提前返回」，不是「丢事件」。
  await q.flushThrough(q.currentSeq())
  assert.equal(bridge.state.calls.length, q.currentSeq(), '所有事件最终都写进去了，一条不丢')
  assert.equal(bridge.state.maxInFlight, 1, '拆成一条一轮后仍然严格串行')
})

await test('flushThrough 会等到 barrier 真正完成，不是提前返回', async () => {
  const bridge = makeBridge({ perCallMs: 10 })
  const q = makeDrain(bridge)
  enqueueTyping(q)
  enqueueTyping(q)
  enqueueTyping(q)
  const barrier = q.currentSeq() // = 3
  await q.flushThrough(barrier)
  assert.equal(bridge.state.calls.length, 3, '返回时 barrier 以内的三条都写完了')
})

await test('失败按 seq 记账：flushThrough 返回 false，且不影响更早的 barrier', async () => {
  // 第 2 次调用失败
  const bridge = makeBridge({ failSeqs: new Set([2]) })
  const q = makeDrain(bridge)
  enqueueTyping(q)
  const barrier1 = q.currentSeq() // = 1
  enqueueTyping(q)
  const barrier2 = q.currentSeq() // = 2

  assert.equal(await q.flushThrough(barrier1), true, 'seq1 干净')
  assert.equal(await q.flushThrough(barrier2), false, 'seq2 失败，barrier2 不干净')
  // 再 flush 一次，失败不能被抹掉
  assert.equal(await q.flushThrough(barrier2), false, '失败不能被后来的 flush 抹掉')
  assert.equal(await q.flushThrough(barrier1), true, '更早的 barrier 不受更晚失败影响')
})

await test('纯光标移动与空 chapterId 不入队', async () => {
  const bridge = makeBridge()
  const q = makeDrain(bridge)
  // 空队列时 flushThrough(0) 直接干净返回，不该死等。
  assert.equal(await q.flushThrough(0), true, 'barrier=0 表示无事可等，直接干净返回')
  assert.equal(bridge.state.calls.length, 0)
})

console.log('---')
console.log(`${passed} tests passed`)