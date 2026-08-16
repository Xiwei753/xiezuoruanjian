// editor_command_queue.test.mjs — SerialCommandQueue 串行命令队列纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_patch_logic.ts（纯 TS，无 ArkUI 依赖）。
// 验证 Issue #629 评论 5 第 1 部分的核心要求：
//   1. enqueue 顺序保持：命令按 enqueue 顺序执行，不 reorder
//   2. 每条出队才读 revision：thunk 在出队执行时才读当前 state，不是 enqueue 时
//   3. 前一条完成才下一条：async thunk 有不同延迟，顺序仍保持
//   4. 一条失败不阻塞后续：thunk reject 不让后续命令饿死
//   5. 快速并发命令 revision 串行：模拟 Coordinator 场景，多条命令并发 enqueue，
//      每条读到的是前一条执行后的 revision，不会拿到同一个 expectedRevision
//   6. IME composition 顺序：模拟 begin→update→update→finish 事件串行
//
// 运行：node --experimental-strip-types editor_command_queue.test.mjs

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../editor_patch_logic.ts'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

const sleep = (ms) => new Promise(r => setTimeout(r, ms))

console.log('SerialCommandQueue 串行命令队列纯逻辑单测')

// ── 1. enqueue 顺序保持 ──
await testAsync('enqueue 顺序保持：3 条同步命令按 a,b,c 顺序执行', async () => {
  const q = new SerialCommandQueue()
  const order = []
  await Promise.all([
    q.enqueue(async () => { order.push('a') }),
    q.enqueue(async () => { order.push('b') }),
    q.enqueue(async () => { order.push('c') }),
  ])
  assert.deepEqual(order, ['a', 'b', 'c'])
})

await testAsync('enqueue 顺序保持：10 条命令按 0..9 顺序执行', async () => {
  const q = new SerialCommandQueue()
  const order = []
  const promises = []
  for (let i = 0; i < 10; i++) {
    promises.push(q.enqueue(async () => { order.push(i) }))
  }
  await Promise.all(promises)
  assert.deepEqual(order, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
})

// ── 2. 每条出队才读 revision（核心：避免并发命令拿到同一个 expectedRevision）──
await testAsync('每条出队才读 revision：3 条并发命令读到 1,2,3 而非 1,1,1', async () => {
  const q = new SerialCommandQueue()
  let revision = 1
  const readRevisions = []
  await Promise.all([
    q.enqueue(async () => { readRevisions.push(revision); revision += 1 }),
    q.enqueue(async () => { readRevisions.push(revision); revision += 1 }),
    q.enqueue(async () => { readRevisions.push(revision); revision += 1 }),
  ])
  // 关键断言：每条读到的 revision 不同，是前一条执行后的新值
  assert.deepEqual(readRevisions, [1, 2, 3])
})

await testAsync('每条出队才读 revision：5 条并发命令读到 1,2,3,4,5', async () => {
  const q = new SerialCommandQueue()
  let revision = 1
  const readRevisions = []
  const promises = []
  for (let i = 0; i < 5; i++) {
    promises.push(q.enqueue(async () => { readRevisions.push(revision); revision += 1 }))
  }
  await Promise.all(promises)
  assert.deepEqual(readRevisions, [1, 2, 3, 4, 5])
})

// ── 3. 前一条完成才下一条（async thunk 有不同延迟，顺序仍保持）──
await testAsync('前一条完成才下一条：slow(50ms) 先于 fast(0ms) 完成', async () => {
  const q = new SerialCommandQueue()
  const order = []
  await Promise.all([
    q.enqueue(async () => { await sleep(50); order.push('slow') }),
    q.enqueue(async () => { order.push('fast') }),
  ])
  assert.deepEqual(order, ['slow', 'fast'])
})

await testAsync('前一条完成才下一条：交错延迟，顺序仍保持', async () => {
  const q = new SerialCommandQueue()
  const order = []
  await Promise.all([
    q.enqueue(async () => { await sleep(30); order.push('a') }),
    q.enqueue(async () => { await sleep(10); order.push('b') }),
    q.enqueue(async () => { await sleep(40); order.push('c') }),
    q.enqueue(async () => { await sleep(5); order.push('d') }),
  ])
  assert.deepEqual(order, ['a', 'b', 'c', 'd'])
})

// ── 4. 一条失败不阻塞后续 ──
await testAsync('一条失败不阻塞后续：第一条 reject，第二条仍执行', async () => {
  const q = new SerialCommandQueue()
  const order = []
  const p1 = q.enqueue(async () => { order.push('a'); throw new Error('boom') })
  const p2 = q.enqueue(async () => { order.push('b'); return 'ok' })
  await p1.catch(() => {})
  const r2 = await p2
  assert.deepEqual(order, ['a', 'b'])
  assert.equal(r2, 'ok')
})

await testAsync('一条失败不阻塞后续：中间 reject，前后都执行', async () => {
  const q = new SerialCommandQueue()
  const order = []
  const p1 = q.enqueue(async () => { order.push('a'); return 1 })
  const p2 = q.enqueue(async () => { order.push('b'); throw new Error('middle') })
  const p3 = q.enqueue(async () => { order.push('c'); return 3 })
  const r1 = await p1
  await p2.catch(() => {})
  const r3 = await p3
  assert.deepEqual(order, ['a', 'b', 'c'])
  assert.equal(r1, 1)
  assert.equal(r3, 3)
})

// ── 5. 快速并发命令 revision 串行（模拟 Coordinator 场景）──
await testAsync('快速并发命令 revision 串行：模拟 Coordinator insert 命令，每条 expectedRevision 递增', async () => {
  // 模拟 EditorSessionCoordinator 的串行命令场景：
  // - state.revision 初始为 1
  // - 每条 insert 命令 enqueue 一个 thunk，thunk 出队时读 state.revision 作为 expectedRevision
  // - bridge.insert 返回 newRevision = expectedRevision + 1
  // - state.revision 更新为 newRevision
  // 快速并发 enqueue 5 条 insert，每条应读到不同的 expectedRevision（1,2,3,4,5）
  const q = new SerialCommandQueue()
  let stateRevision = 1
  const expectedRevisions = []
  const newRevisions = []

  const insertThunk = async (text) => {
    // 出队时才读 revision — 这是串行队列的核心
    const expectedRev = stateRevision
    expectedRevisions.push(expectedRev)
    // 模拟 bridge.insert：返回 newRevision = expectedRev + 1
    const newRev = expectedRev + 1
    stateRevision = newRev
    newRevisions.push(newRev)
    return { success: true, data: { outcome: 'applied', newRevision: newRev } }
  }

  // 快速并发 enqueue 5 条 insert（模拟用户快速打字）
  const promises = []
  for (let i = 0; i < 5; i++) {
    const text = `char${i}`
    promises.push(q.enqueue(() => insertThunk(text)))
  }
  await Promise.all(promises)

  // 关键断言：每条命令读到的 expectedRevision 不同，严格递增
  assert.deepEqual(expectedRevisions, [1, 2, 3, 4, 5])
  assert.deepEqual(newRevisions, [2, 3, 4, 5, 6])
  assert.equal(stateRevision, 6)
})

await testAsync('快速并发命令 revision 串行：模拟旧 Coordinator 的 bug（Promise 启动前读 revision）会拿到同一个 revision', async () => {
  // 这个测试验证：如果不串行（旧实现），并发命令会拿到同一个 revision。
  // 旧实现：insert() 在函数被调用时（Promise 启动前）就读 this.state.snapshot.revision，
  // 然后把已启动的 Promise 传给 applyEdit()。快速调用 insert 5 次（不 await），
  // 5 次调用都在同一个同步栈中执行，每次读 stateRevision 都是 1（state 要等 await bridge 返回后才更新）。
  let stateRevision = 1
  const oldExpectedRevisions = []
  const oldPromises = []
  // 旧实现：5 次 insert 调用，每次在调用时（Promise 启动前）读 revision
  for (let i = 0; i < 5; i++) {
    const expectedRev = stateRevision  // 在 Promise 启动前读，都是 1（stateRevision 还没更新）
    oldExpectedRevisions.push(expectedRev)
    oldPromises.push((async () => {
      // bridge.insert 用 expectedRev（已读到的）；模拟 bridge 返回后更新 state
      await sleep(1)  // 模拟 bridge 调用异步
      stateRevision = expectedRev + 1  // 5 个并发写都是 1+1=2
    })())
  }
  await Promise.all(oldPromises)
  // 旧实现的 bug：所有命令读到 revision=1
  assert.deepEqual(oldExpectedRevisions, [1, 1, 1, 1, 1])
  // 旧实现最终 revision 只 +1（不是 +5），因为 5 个并发写都是 1+1=2
  assert.equal(stateRevision, 2)

  // 新实现（SerialCommandQueue）：每条出队才读 revision，读到不同值
  const q = new SerialCommandQueue()
  let newStateRevision = 1
  const newExpectedRevisions = []
  await Promise.all(Array.from({ length: 5 }, () =>
    q.enqueue(async () => {
      const expectedRev = newStateRevision  // 出队时才读
      newExpectedRevisions.push(expectedRev)
      await sleep(1)
      newStateRevision = expectedRev + 1
    })
  ))
  assert.deepEqual(newExpectedRevisions, [1, 2, 3, 4, 5])
  assert.equal(newStateRevision, 6)
})

// ── 6. IME composition 顺序：begin→update→update→finish 串行 ──
await testAsync('IME composition 顺序：begin→update→update→finish 按事件顺序执行', async () => {
  // 模拟 HarmonyImeConnection 的 IME 事件串行
  const q = new SerialCommandQueue()
  const events = []
  let composing = false
  let compositionSession = null

  // onSetPreviewText 第一次触发 begin，后续触发 update
  const onSetPreviewText = (text) => q.enqueue(async () => {
    if (!composing) {
      // begin
      events.push(`begin:${text}`)
      composing = true
      compositionSession = { id: 1, generation: 1 }
      return { success: true, data: { compositionSession } }
    }
    // update
    events.push(`update:${text}`)
    compositionSession.generation += 1
    return { success: true, data: { compositionSession } }
  })

  // onInsertText 在 composing 时触发 finish
  const onInsertText = (text) => q.enqueue(async () => {
    if (composing) {
      events.push(`finish:${text}`)
      composing = false
      compositionSession = null
      return { success: true }
    }
    events.push(`insert:${text}`)
    return { success: true }
  })

  // 模拟 IME 事件序列：setPreviewText("你") → setPreviewText("你好") → insertText("你好")
  await onSetPreviewText('你').then(r => r)
  // 注意：第一次 begin 必须 await 成功后才能 update（评论要求）
  // 在串行队列下，第二个 setPreviewText 会等第一个完成
  await Promise.all([
    onSetPreviewText('你好'),
    onInsertText('你好'),
  ])

  assert.deepEqual(events, ['begin:你', 'update:你好', 'finish:你好'])
})

await testAsync('IME composition 顺序：begin 失败时不 update（不让 update 读到 sessionId=0）', async () => {
  // 模拟 onSetPreviewText 第一次 begin 失败，不应 update
  const q = new SerialCommandQueue()
  const events = []
  let composing = false

  const onSetPreviewText = (text) => q.enqueue(async () => {
    if (!composing) {
      // begin 失败
      events.push(`beginFail:${text}`)
      return { success: false, errorCode: 'BEGIN_FAILED' }
      // 不设 composing = true
    }
    events.push(`update:${text}`)
    return { success: true }
  })

  // 第一次 setPreviewText begin 失败
  const r1 = await onSetPreviewText('你')
  assert.equal(r1.success, false)
  // composing 仍为 false
  assert.equal(composing, false)
  // 没有 update 事件
  assert.deepEqual(events, ['beginFail:你'])
})

// ── 7. size/isIdle/stats ──
await testAsync('size/isIdle：空队列 isIdle=true, size=0', () => {
  const q = new SerialCommandQueue()
  assert.equal(q.size(), 0)
  assert.equal(q.isIdle(), true)
  const s = q.stats()
  assert.equal(s.pending, 0)
  assert.equal(s.running, false)
})

await testAsync('size/isIdle：执行中 isIdle=false', async () => {
  const q = new SerialCommandQueue()
  const p = q.enqueue(async () => { await sleep(30); return 1 })
  assert.equal(q.isIdle(), false)
  await p
  assert.equal(q.isIdle(), true)
})

console.log('---')
console.log(`✅ editor_command_queue: ${passed} tests passed`)
