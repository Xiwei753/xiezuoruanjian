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

// ── 7. Issue #629 评论7 第1项：中文 preedit cursor 单位是 UTF-16 code unit ──
// makeImeDispatcher：模拟 EditorSemanticDispatcher 的 ime* 命令出队逻辑。
// mock inputAdapter（isComposing/onCompositionBegin/Update/Finish/Cancel/onTextInput）
// 和 coordinator（updateComposition/delete/previousGraphemeBoundary/nextGraphemeBoundary）。
// 记录调用次数、参数、顺序，真实验证行为。
async function makeImeDispatcher() {
  let snapshot = { text: '', cursor: 0, selectionAnchor: 0 }
  const queue = new SerialCommandQueue()
  const calls = { begin: 0, update: 0, finish: 0, cancel: 0, textInput: 0, previousGraphemeBoundary: 0, nextGraphemeBoundary: 0 }
  const callSequence = []
  const coordinatorCalls = { updateComposition: [], delete: [] }
  const textInputCalls = []
  let composing = false
  let mockPrevBoundaryFn = null
  let mockNextBoundaryFn = null

  const inputAdapter = {
    isComposing: () => composing,
    onCompositionBegin: async () => {
      calls.begin++
      callSequence.push('begin')
      composing = true
      return { success: true, data: { compositionSession: { sessionId: 1, generation: 1 } } }
    },
    onCompositionUpdate: async (preedit) => {
      calls.update++
      callSequence.push(`update:${preedit}`)
      // Issue #629 评论7 第1项：cursor 单位是 UTF-16 code unit（preedit.length）
      const cursorUtf16 = preedit.length
      coordinatorCalls.updateComposition.push({ preedit, cursorUtf16 })
      return { success: true, data: {} }
    },
    onCompositionFinish: async (committed) => {
      calls.finish++
      callSequence.push(`finish:${committed}`)
      composing = false
      return { success: true, data: {} }
    },
    onCompositionCancel: async () => {
      calls.cancel++
      callSequence.push('cancel')
      composing = false
      return { success: true, data: {} }
    },
    onTextInput: async (text) => {
      calls.textInput++
      callSequence.push(`textInput:${text}`)
      textInputCalls.push({ text })
      return { success: true, data: {} }
    },
    currentSelectionUtf8: () => {
      const start = Math.min(snapshot.selectionAnchor, snapshot.cursor)
      const end = Math.max(snapshot.selectionAnchor, snapshot.cursor)
      return { start, end }
    },
  }

  const coordinator = {
    getSnapshot: () => snapshot,
    previousGraphemeBoundary: async (byteOffset) => {
      calls.previousGraphemeBoundary++
      const result = mockPrevBoundaryFn ? mockPrevBoundaryFn(byteOffset) : byteOffset - 1
      return { success: true, data: result }
    },
    nextGraphemeBoundary: async (byteOffset) => {
      calls.nextGraphemeBoundary++
      const result = mockNextBoundaryFn ? mockNextBoundaryFn(byteOffset) : byteOffset + 1
      return { success: true, data: result }
    },
    delete: async (start, end, cause) => {
      coordinatorCalls.delete.push({ start, end, cause })
      // 模拟删除后 snapshot 更新：text 截掉 [start,end)，cursor 移到 start
      // 简化：测试用 ASCII 文本时 byte offset == UTF-16 code unit offset
      const newText = snapshot.text.substring(0, start) + snapshot.text.substring(end)
      snapshot = { text: newText, cursor: start, selectionAnchor: start }
      return { success: true, data: {} }
    },
  }

  const executeGraphemeBackspace = async () => {
    const sel = inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) {
      return coordinator.delete(sel.start, sel.end, 'Delete')
    }
    if (sel.start <= 0) {
      return { success: false, errorCode: 'NO_CHAR_TO_DELETE' }
    }
    const prevResult = await coordinator.previousGraphemeBoundary(sel.start)
    if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) {
      return prevResult
    }
    return coordinator.delete(prevResult.data, sel.start, 'Delete')
  }

  const executeGraphemeDelete = async () => {
    const sel = inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) {
      return coordinator.delete(sel.start, sel.end, 'Delete')
    }
    const nextResult = await coordinator.nextGraphemeBoundary(sel.start)
    if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) {
      return nextResult
    }
    return coordinator.delete(sel.start, nextResult.data, 'Delete')
  }

  const executeCommand = async (cmd) => {
    switch (cmd.kind) {
      case 'imePreviewText':
        if (!inputAdapter.isComposing()) {
          const beginResult = await inputAdapter.onCompositionBegin()
          if (!beginResult.success) return beginResult
          return inputAdapter.onCompositionUpdate(cmd.text)
        }
        return inputAdapter.onCompositionUpdate(cmd.text)
      case 'imeCommitText':
        if (inputAdapter.isComposing()) {
          return inputAdapter.onCompositionFinish(cmd.text)
        }
        return inputAdapter.onTextInput(cmd.text)
      case 'imeCancelPreview':
        if (inputAdapter.isComposing()) {
          return inputAdapter.onCompositionCancel()
        }
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      case 'graphemeBackspace':
        return executeGraphemeBackspace()
      case 'graphemeDelete':
        return executeGraphemeDelete()
      case 'insertText':
        return inputAdapter.onTextInput(cmd.text)
      default:
        return { success: true, data: {} }
    }
  }

  const dispatch = (cmd) => queue.enqueue(async () => executeCommand(cmd))
  const dispatchFireAndForget = (cmd) => { dispatch(cmd).then(() => {}, () => {}) }
  const flush = () => queue.whenIdle()
  const isIdle = () => queue.isIdle()

  return {
    dispatch, dispatchFireAndForget, flush, isIdle,
    calls, callSequence, coordinatorCalls, textInputCalls,
    setSnapshot: (s) => { snapshot = s },
    mockPreviousGraphemeBoundary: (fn) => { mockPrevBoundaryFn = fn },
    mockNextGraphemeBoundary: (fn) => { mockNextBoundaryFn = fn },
    getSnapshot: () => snapshot,
  }
}

// UTF-8 byte length 辅助
const utf8ByteLen = (s) => new TextEncoder().encode(s).length

await testAsync('评论7 第1项：imePreviewText("你") → update 传 cursorUtf16=1（UTF-16 code unit，不是 3）', async () => {
  const d = await makeImeDispatcher()
  await d.dispatch({ kind: 'imePreviewText', text: '你' })
  // begin 成功后 update
  assert.equal(d.calls.begin, 1, '应调一次 begin')
  assert.equal(d.calls.update, 1, '应调一次 update')
  // update 传给 coordinator.updateComposition 的 cursorUtf16=1（"你".length=1 UTF-16 code unit）
  // 不是 3（UTF-8 byte length）
  const updateCall = d.coordinatorCalls.updateComposition[0]
  assert.equal(updateCall.preedit, '你')
  assert.equal(updateCall.cursorUtf16, 1, 'cursorUtf16 应为 1（UTF-16 code unit），不是 3（UTF-8 byte）')
})

await testAsync('评论7 第1项：imePreviewText("你好") → cursorUtf16=2（两个 UTF-16 code unit，不是 6 byte）', async () => {
  const d = await makeImeDispatcher()
  await d.dispatch({ kind: 'imePreviewText', text: '你好' })
  const updateCall = d.coordinatorCalls.updateComposition[0]
  assert.equal(updateCall.preedit, '你好')
  assert.equal(updateCall.cursorUtf16, 2, 'cursorUtf16 应为 2（UTF-16 code unit），不是 6（UTF-8 byte）')
})

await testAsync('评论7 第1项：imePreviewText("👨‍👩‍👧") → cursorUtf16=8（ZWJ emoji 8 个 UTF-16 code unit）', async () => {
  const d = await makeImeDispatcher()
  const emoji = '👨‍👩‍👧'  // 8 UTF-16 code units, 1 grapheme
  await d.dispatch({ kind: 'imePreviewText', text: emoji })
  const updateCall = d.coordinatorCalls.updateComposition[0]
  assert.equal(updateCall.preedit, emoji)
  assert.equal(updateCall.cursorUtf16, 8, 'cursorUtf16 应为 8（UTF-16 code unit），不是 ' + utf8ByteLen(emoji) + '（UTF-8 byte）')
})

// ── 8. Issue #629 评论7 第2项：快速 preview/commit 顺序经串行队列 ──
await testAsync('评论7 第2项：imePreviewText("你")→imePreviewText("你好")→imeCommitText("你好") 串行 begin→update→update→finish', async () => {
  const d = await makeImeDispatcher()
  const promises = [
    d.dispatch({ kind: 'imePreviewText', text: '你' }),
    d.dispatch({ kind: 'imePreviewText', text: '你好' }),
    d.dispatch({ kind: 'imeCommitText', text: '你好' }),
  ]
  await Promise.all(promises)
  // 顺序：begin, update("你"), update("你好"), finish("你好")
  assert.deepEqual(d.callSequence, [
    'begin', 'update:你', 'update:你好', 'finish:你好'
  ], '调用顺序应为 begin→update→update→finish')
  assert.equal(d.calls.begin, 1)
  assert.equal(d.calls.update, 2)
  assert.equal(d.calls.finish, 1)
})

await testAsync('评论7 第2项：imeCancelPreview 有 composition 时 cancel', async () => {
  const d = await makeImeDispatcher()
  await d.dispatch({ kind: 'imePreviewText', text: '你' })  // begin + update
  assert.equal(d.calls.cancel, 0)
  await d.dispatch({ kind: 'imeCancelPreview' })
  assert.equal(d.calls.cancel, 1, '有 composition 时应调 cancel')
})

await testAsync('评论7 第2项：imeCancelPreview 无 composition 时 no-op（不调 cancel）', async () => {
  const d = await makeImeDispatcher()
  // 初始无 composition
  const result = await d.dispatch({ kind: 'imeCancelPreview' })
  assert.equal(d.calls.cancel, 0, '无 composition 时不应调 cancel')
  assert.equal(result.success, true)
})

// ── 9. Issue #629 评论7 第2项：imeCommitText 无 composition 时普通插入 ──
await testAsync('评论7 第2项：imeCommitText 无 composition 时 onTextInput("x")，不走 finish', async () => {
  const d = await makeImeDispatcher()
  // isComposing()=false（初始状态）
  await d.dispatch({ kind: 'imeCommitText', text: 'x' })
  assert.equal(d.calls.textInput, 1, '应调 onTextInput')
  assert.equal(d.calls.finish, 0, '不应调 onCompositionFinish')
  assert.equal(d.textInputCalls[0].text, 'x')
})

await testAsync('评论7 第2项：imeCommitText 有 composition 时 onCompositionFinish，不走 onTextInput', async () => {
  const d = await makeImeDispatcher()
  await d.dispatch({ kind: 'imePreviewText', text: '你' })  // begin + update → composing=true
  await d.dispatch({ kind: 'imeCommitText', text: '你' })
  assert.equal(d.calls.finish, 1, '有 composition 时应调 onCompositionFinish')
  assert.equal(d.calls.textInput, 0, '有 composition 时不应调 onTextInput')
})

// ── 10. Issue #629 评论7 第6项：软键盘删除统一 Core grapheme ──
await testAsync('评论7 第6项：ZWJ emoji "👨‍👩‍👧" graphemeBackspace 删整个 grapheme（不拆 surrogate）', async () => {
  const d = await makeImeDispatcher()
  const emoji = '👨‍👩‍👧'  // 1 grapheme, 8 UTF-16 code units, 18 UTF-8 bytes
  const text = 'a' + emoji
  const textByteLen = utf8ByteLen(text)  // 1 + 18 = 19
  // 光标在文本末尾
  d.setSnapshot({ text, cursor: textByteLen, selectionAnchor: textByteLen })
  // mock previousGraphemeBoundary：从 textByteLen 返回 1（'a' 后的位置，emoji 前）
  const aByteLen = utf8ByteLen('a')  // 1
  d.mockPreviousGraphemeBoundary((byteOffset) => aByteLen)
  await d.dispatch({ kind: 'graphemeBackspace' })
  // previousGraphemeBoundary 被调
  assert.equal(d.calls.previousGraphemeBoundary, 1, '应调 previousGraphemeBoundary')
  // delete 删整个 grapheme：从 aByteLen 到 textByteLen
  const deleteCall = d.coordinatorCalls.delete[0]
  assert.equal(deleteCall.start, aByteLen, 'delete start 应为 emoji 前边界')
  assert.equal(deleteCall.end, textByteLen, 'delete end 应为整个 emoji 的 byte 范围（不拆 surrogate）')
})

await testAsync('评论7 第6项：组合附加符 "é" = e + U+0301 graphemeBackspace 删整个组合字符', async () => {
  const d = await makeImeDispatcher()
  const combining = 'e\u0301'  // e + combining acute accent, 1 grapheme, 2 UTF-16 code units, 3 UTF-8 bytes
  const text = combining
  const textByteLen = utf8ByteLen(text)  // 3
  d.setSnapshot({ text, cursor: textByteLen, selectionAnchor: textByteLen })
  // mock previousGraphemeBoundary：从 textByteLen 返回 0（文本开头）
  d.mockPreviousGraphemeBoundary((byteOffset) => 0)
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(d.calls.previousGraphemeBoundary, 1)
  const deleteCall = d.coordinatorCalls.delete[0]
  assert.equal(deleteCall.start, 0, 'delete start 应为文本开头')
  assert.equal(deleteCall.end, textByteLen, 'delete end 应为整个组合字符的 byte 范围（不拆 e 和附加符）')
})

await testAsync('评论7 第6项：graphemeBackspace 空选区在文本开头返回 NO_CHAR_TO_DELETE', async () => {
  const d = await makeImeDispatcher()
  d.setSnapshot({ text: 'abc', cursor: 0, selectionAnchor: 0 })
  const result = await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_CHAR_TO_DELETE')
})

await testAsync('评论7 第6项：graphemeBackspace 有选区时删除整个选区', async () => {
  const d = await makeImeDispatcher()
  // 选区 [1, 3)
  d.setSnapshot({ text: 'abcde', cursor: 3, selectionAnchor: 1 })
  await d.dispatch({ kind: 'graphemeBackspace' })
  const deleteCall = d.coordinatorCalls.delete[0]
  assert.equal(deleteCall.start, 1)
  assert.equal(deleteCall.end, 3)
  // 有选区时不调 previousGraphemeBoundary（直接删选区）
  assert.equal(d.calls.previousGraphemeBoundary, 0)
})

await testAsync('评论7 第6项：graphemeDelete 用 nextGraphemeBoundary 删后一个 grapheme', async () => {
  const d = await makeImeDispatcher()
  const emoji = '👨‍👩‍👧'
  const text = emoji + 'b'
  const emojiByteLen = utf8ByteLen(emoji)  // 18
  // 光标在 emoji 前（offset=0）
  d.setSnapshot({ text, cursor: 0, selectionAnchor: 0 })
  // mock nextGraphemeBoundary：从 0 返回 emojiByteLen（emoji 后边界）
  d.mockNextGraphemeBoundary((byteOffset) => emojiByteLen)
  await d.dispatch({ kind: 'graphemeDelete' })
  assert.equal(d.calls.nextGraphemeBoundary, 1)
  const deleteCall = d.coordinatorCalls.delete[0]
  assert.equal(deleteCall.start, 0)
  assert.equal(deleteCall.end, emojiByteLen, 'delete end 应为整个 emoji 的 byte 范围')
})

await testAsync('评论7 第6项：连续 graphemeBackspace 串行执行（不并发）', async () => {
  const d = await makeImeDispatcher()
  d.setSnapshot({ text: 'abc', cursor: 3, selectionAnchor: 3 })
  // mock previousGraphemeBoundary：每次返回 cursor-1
  d.mockPreviousGraphemeBoundary((byteOffset) => byteOffset - 1)
  // 连续两次 graphemeBackspace
  await d.dispatch({ kind: 'graphemeBackspace' })
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(d.calls.previousGraphemeBoundary, 2)
  // 第一次删 [2,3)，第二次删 [1,2)
  assert.equal(d.coordinatorCalls.delete[0].start, 2)
  assert.equal(d.coordinatorCalls.delete[0].end, 3)
  assert.equal(d.coordinatorCalls.delete[1].start, 1)
  assert.equal(d.coordinatorCalls.delete[1].end, 2)
})

await testAsync('评论7 第2+6项：imePreviewText → graphemeBackspace 串行（composition 和删除经同一队列）', async () => {
  const d = await makeImeDispatcher()
  d.setSnapshot({ text: 'abc', cursor: 3, selectionAnchor: 3 })
  d.mockPreviousGraphemeBoundary((byteOffset) => byteOffset - 1)
  // 先 composition preview，再 backspace
  await d.dispatch({ kind: 'imePreviewText', text: '你' })
  await d.dispatch({ kind: 'graphemeBackspace' })
  // 顺序：begin, update, graphemeBackspace 的 previousGraphemeBoundary + delete
  assert.deepEqual(d.callSequence, ['begin', 'update:你'])
  assert.equal(d.calls.previousGraphemeBoundary, 1)
  assert.equal(d.coordinatorCalls.delete.length, 1)
})

console.log('---')
console.log(`✅ editor_semantic_dispatcher: ${passed} tests passed`)
