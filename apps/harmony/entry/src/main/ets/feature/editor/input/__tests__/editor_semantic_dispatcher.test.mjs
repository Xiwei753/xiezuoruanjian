// editor_semantic_dispatcher.test.mjs — EditorSemanticDispatcher 调度模式纯逻辑单测。
//
// EditorSemanticDispatcher.ets 依赖 ArkUI 无法用 Node 直接测；
// 本测试验证其调度模式的核心逻辑：语义命令经 SerialCommandQueue 串行排队，
// 出队执行时才读 Coordinator snapshot（避免并发命令拿到同一个旧 cursor/selection）。
// 生产代码 EditorSemanticDispatcher.ets 用相同模式（dispatch → queue.enqueue → executeCommand），
// 需 HarmonyOS SDK 才能端到端编译——本地无 SDK，此为已知阻塞。
//
// Issue #629 评论6 Part A：统一串行边界。
// 验证：
//   1. 语义命令按 dispatch 顺序执行（insertText → setSelection → delete）
//   2. 每条出队才读 snapshot：快速并发 dispatch 时，每条读到的是前一条执行后的 snapshot
//   3. grapheme 导航命令（graphemeBackspace 等）也经同一队列串行
//   4. fire-and-forget 不阻塞后续命令
//   5. flush 等所有排队命令完成
//
// 运行：node --experimental-strip-types editor_semantic_dispatcher.test.mjs

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../../session/editor_patch_logic.ts'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms))

console.log('EditorSemanticDispatcher 调度模式纯逻辑单测')

// 模拟 Coordinator snapshot：cursor 随 insert 命令前进
// 模拟 dispatch 模式：dispatch(cmd) → queue.enqueue(() => executeCommand(cmd))
// executeCommand 出队时才读 snapshot.cursor
async function makeDispatcher() {
  let snapshot = { text: '', cursor: 0, selectionAnchor: 0 }
  const queue = new SerialCommandQueue()
  const executedCommands = []
  const readCursors = []

  const executeCommand = async (cmd) => {
    // 出队时才读 snapshot — 这是串行队列的核心
    const cursorBefore = snapshot.cursor
    readCursors.push(cursorBefore)
    switch (cmd.kind) {
      case 'insertText':
        snapshot = {
          text: snapshot.text + cmd.text,
          cursor: snapshot.cursor + cmd.text.length,
          selectionAnchor: snapshot.cursor + cmd.text.length,
        }
        executedCommands.push(`insert:${cmd.text}@${cursorBefore}`)
        return { success: true, data: { outcome: 'applied' } }
      case 'setSelection':
        snapshot = { text: snapshot.text, cursor: cmd.utf16Offset, selectionAnchor: cmd.utf16Offset }
        executedCommands.push(`setSel:${cmd.utf16Offset}@${cursorBefore}`)
        return { success: true, data: { outcome: 'applied' } }
      case 'graphemeBackspace':
        if (snapshot.cursor <= 0) {
          return { success: false, errorCode: 'NO_CHAR_TO_DELETE' }
        }
        // 模拟 grapheme backspace：删一个字符
        const newText = snapshot.text.slice(0, -1)
        snapshot = { text: newText, cursor: snapshot.cursor - 1, selectionAnchor: snapshot.cursor - 1 }
        executedCommands.push(`graphemeBs@${cursorBefore}`)
        return { success: true, data: { outcome: 'applied' } }
      default:
        executedCommands.push(`unknown:${cmd.kind}`)
        return { success: true, data: { outcome: 'applied' } }
    }
  }

  const dispatch = (cmd) => queue.enqueue(async () => executeCommand(cmd))
  const dispatchFireAndForget = (cmd) => {
    dispatch(cmd).then(() => {}, () => {})
  }
  const flush = () => queue.whenIdle()
  const isIdle = () => queue.isIdle()
  const getSnapshot = () => snapshot
  const getExecutedCommands = () => executedCommands
  const getReadCursors = () => readCursors

  return { dispatch, dispatchFireAndForget, flush, isIdle, getSnapshot, getExecutedCommands, getReadCursors }
}

// ── 1. 语义命令按 dispatch 顺序执行 ──
await testAsync('语义命令按 dispatch 顺序执行：insert A → insert B → setSelection', async () => {
  const d = await makeDispatcher()
  await d.dispatch({ kind: 'insertText', text: 'A' })
  await d.dispatch({ kind: 'insertText', text: 'B' })
  await d.dispatch({ kind: 'setSelection', utf16Offset: 0 })
  assert.deepEqual(d.getExecutedCommands(), ['insert:A@0', 'insert:B@1', 'setSel:0@2'])
  assert.deepEqual(d.getSnapshot().text, 'AB')
  assert.equal(d.getSnapshot().cursor, 0)
})

// ── 2. 每条出队才读 snapshot（核心：避免并发命令拿到同一个旧 cursor）──
await testAsync('每条出队才读 snapshot：快速并发 dispatch 5 条 insert，每条读到不同 cursor', async () => {
  const d = await makeDispatcher()
  // 快速并发 dispatch 5 条（不 await）
  const promises = []
  for (let i = 0; i < 5; i++) {
    promises.push(d.dispatch({ kind: 'insertText', text: 'x' }))
  }
  await Promise.all(promises)
  // 关键断言：每条命令读到的 cursor 不同，严格递增
  assert.deepEqual(d.getReadCursors(), [0, 1, 2, 3, 4])
  assert.equal(d.getSnapshot().text, 'xxxxx')
  assert.equal(d.getSnapshot().cursor, 5)
})

await testAsync('每条出队才读 snapshot：旧实现（dispatch 时读 cursor）会拿到同一个 0', async () => {
  // 验证：如果不串行（旧实现），并发命令会拿到同一个 cursor=0
  let cursor = 0
  const oldReadCursors = []
  const oldPromises = []
  for (let i = 0; i < 5; i++) {
    const c = cursor  // dispatch 时读，都是 0
    oldReadCursors.push(c)
    oldPromises.push((async () => {
      await sleep(1)
      cursor = c + 1  // 5 个并发写都是 0+1=1
    })())
  }
  await Promise.all(oldPromises)
  assert.deepEqual(oldReadCursors, [0, 0, 0, 0, 0])
  assert.equal(cursor, 1)  // 旧实现最终 cursor 只 +1

  // 新实现（SerialCommandQueue）：每条出队才读 cursor，读到不同值
  const d = await makeDispatcher()
  await Promise.all(Array.from({ length: 5 }, () => d.dispatch({ kind: 'insertText', text: 'y' })))
  assert.deepEqual(d.getReadCursors(), [0, 1, 2, 3, 4])
  assert.equal(d.getSnapshot().cursor, 5)
})

// ── 3. grapheme 导航命令也经同一队列串行 ──
await testAsync('grapheme 导航命令经同一队列串行：insert → graphemeBackspace → insert', async () => {
  const d = await makeDispatcher()
  await d.dispatch({ kind: 'insertText', text: 'abc' })
  await d.dispatch({ kind: 'graphemeBackspace' })
  await d.dispatch({ kind: 'insertText', text: 'X' })
  assert.deepEqual(d.getExecutedCommands(), ['insert:abc@0', 'graphemeBs@3', 'insert:X@2'])
  assert.equal(d.getSnapshot().text, 'abX')
  assert.equal(d.getSnapshot().cursor, 3)
})

await testAsync('graphemeBackspace 在空文本时返回 NO_CHAR_TO_DELETE', async () => {
  const d = await makeDispatcher()
  const result = await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_CHAR_TO_DELETE')
})

// ── 4. fire-and-forget 不阻塞后续命令 ──
await testAsync('fire-and-forget 不阻塞后续命令', async () => {
  const d = await makeDispatcher()
  d.dispatchFireAndForget({ kind: 'insertText', text: 'A' })
  d.dispatchFireAndForget({ kind: 'insertText', text: 'B' })
  d.dispatchFireAndForget({ kind: 'insertText', text: 'C' })
  // 等所有 fire-and-forget 命令完成
  await d.flush()
  assert.equal(d.getSnapshot().text, 'ABC')
  assert.equal(d.isIdle(), true)
})

// ── 5. flush 等所有排队命令完成 ──
await testAsync('flush 等所有排队命令完成', async () => {
  const d = await makeDispatcher()
  d.dispatch({ kind: 'insertText', text: 'a' })
  d.dispatch({ kind: 'insertText', text: 'b' })
  d.dispatch({ kind: 'insertText', text: 'c' })
  assert.equal(d.isIdle(), false)
  await d.flush()
  assert.equal(d.isIdle(), true)
  assert.equal(d.getSnapshot().text, 'abc')
})

await testAsync('flush 期间又有新命令排入，递归等待', async () => {
  // 自定义 dispatcher：第一条命令慢（30ms），flush 期间排入第二条
  let snapshot = { text: '', cursor: 0 }
  const queue = new SerialCommandQueue()
  const dispatch = (cmd, delay = 0) => queue.enqueue(async () => {
    if (delay > 0) { await sleep(delay) }
    snapshot = { text: snapshot.text + cmd.text, cursor: snapshot.cursor + cmd.text.length }
  })
  const flush = () => queue.whenIdle()
  const isIdle = () => queue.isIdle()
  const getText = () => snapshot.text

  dispatch({ kind: 'insertText', text: 'a' }, 30)
  // 在 flush await 期间排入新命令（10ms 后，此时第一条还在执行）
  setTimeout(() => {
    dispatch({ kind: 'insertText', text: 'b' }, 10)
  }, 10)
  await flush()
  assert.equal(isIdle(), true)
  assert.equal(getText(), 'ab')
})

// ── 6. IME composition 顺序：begin → update → finish 经同一队列串行 ──
await testAsync('IME composition 顺序：begin → update → finish 经同一队列串行', async () => {
  const d = await makeDispatcher()
  const events = []
  let composing = false
  // 模拟 HarmonyImeConnection.onSetPreviewText：第一次 begin，后续 update
  // 经 dispatcher 派发，串行队列保证顺序
  const queue = new SerialCommandQueue()
  const onSetPreviewText = (text) => queue.enqueue(async () => {
    if (!composing) {
      events.push(`begin:${text}`)
      composing = true
      return { success: true, data: { compositionSession: { id: 1, generation: 1 } } }
    }
    events.push(`update:${text}`)
    return { success: true }
  })
  const onInsertText = (text) => queue.enqueue(async () => {
    if (composing) {
      events.push(`finish:${text}`)
      composing = false
      return { success: true }
    }
    events.push(`insert:${text}`)
    return { success: true }
  })
  await onSetPreviewText('你')
  await Promise.all([onSetPreviewText('你好'), onInsertText('你好')])
  assert.deepEqual(events, ['begin:你', 'update:你好', 'finish:你好'])
})

console.log('---')
console.log(`✅ editor_semantic_dispatcher: ${passed} tests passed`)
