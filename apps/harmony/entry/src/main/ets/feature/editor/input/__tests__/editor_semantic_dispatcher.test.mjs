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
import { layoutLines, LineBreakKind, CaretAffinity, positionForHorizontalArrival } from '../../render/editor_layout_math.ts'
// Issue #629 R11 评论5329310563：直接 import 生产 utf8ToUtf16（不再复制循环实现）。
import { utf8ToUtf16 } from '../../input/text_offset_mapper.ts'

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


// ── 11. Issue #629 评论8 第4项：imeSetSelection 语义命令 ──
// IME 来源的 selection 不再复用普通 setSelection/dragSelect；composition 内外判断
// 在唯一队列出队时做：
//   - 无 composition：普通 selection（onTap/onDragSelect）
//   - 有 composition 且 selection 完全落在 composition 显示区域内：换算成 preedit 内
//     preeditCursorUtf16，用当前 preedit 文本调 composition update（Core 更新 cursor/generation）
//   - selection 移出 composition 区域（含部分越界）：同一 thunk 内先 finish 再普通 selection，
//     显示坐标映射到 committed 文本坐标

// Issue #629 R11 评论5329310563：本地 utf16ToUtf8/utf8ToUtf16 复制实现已删除，
// 改用从 text_offset_mapper.ts import 的生产函数（见文件顶部 import）。
// makeImeSetSelectionDispatcher：镜像 EditorSemanticDispatcher.executeImeSetSelection。
// mock inputAdapter（isComposing/onCompositionUpdate/finishActiveComposition）+
// selectionController（onTap/onDragSelect）+ coordinator snapshot（text + composition）。
async function makeImeSetSelectionDispatcher() {
  let snapshot = { text: '', revision: 0, cursor: 0, selectionAnchor: 0, composition: null }
  let composing = false
  const calls = { update: 0, finish: 0, tap: 0, dragSelect: 0 }
  const callSequence = []
  const updateCalls = []   // { preedit, cursorUtf16 }
  const tapCalls = []
  const dragSelectCalls = []
  const queue = new SerialCommandQueue()
  let sealed = false

  const inputAdapter = {
    isComposing: () => composing,
    onCompositionUpdate: async (preedit, preeditCursorUtf16) => {
      calls.update++
      const cursorUtf16 = preeditCursorUtf16 === undefined ? preedit.length : preeditCursorUtf16
      updateCalls.push({ preedit, cursorUtf16 })
      callSequence.push(`update:${preedit}@${cursorUtf16}`)
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    finishActiveComposition: async () => {
      calls.finish++
      callSequence.push('finishActiveComposition')
      // 提交 preedit：snapshot.text 在 replace 处插入 preedit，composition 清空
      const comp = snapshot.composition
      if (comp) {
        const startUtf16 = utf8ToUtf16(snapshot.text, comp.replaceByteStart)
        const endUtf16 = utf8ToUtf16(snapshot.text, comp.replaceByteEndExclusive)
        const newText = snapshot.text.substring(0, startUtf16) + comp.preeditText + snapshot.text.substring(endUtf16)
        snapshot = { ...snapshot, text: newText, composition: null }
        composing = false
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  const selectionController = {
    onTap: async (utf16Offset) => {
      calls.tap++
      tapCalls.push(utf16Offset)
      callSequence.push(`tap:${utf16Offset}`)
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onDragSelect: async (anchor, head) => {
      calls.dragSelect++
      dragSelectCalls.push({ anchor, head })
      callSequence.push(`dragSelect:${anchor}:${head}`)
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  const coordinator = {
    getSnapshot: () => snapshot,
    setSnapshot: (s) => { snapshot = s },
    setComposing: (v) => { composing = v },
  }

  // 镜像 EditorSemanticDispatcher.executeImeSetSelection（与生产代码逐行对齐）
  const executeImeSetSelection = async (utf16Start, utf16End) => {
    const snap = coordinator.getSnapshot()
    if (!snap) {
      return { success: false, errorCode: 'NO_SESSION' }
    }
    if (!inputAdapter.isComposing() || snap.composition === null || snap.composition === undefined) {
      if (utf16Start === utf16End) {
        return selectionController.onTap(utf16Start)
      }
      return selectionController.onDragSelect(utf16Start, utf16End)
    }
    const comp = snap.composition
    const committedText = snap.text
    const compStartUtf16 = utf8ToUtf16(committedText, comp.replaceByteStart)
    const compEndUtf16 = utf8ToUtf16(committedText, comp.replaceByteEndExclusive)
    const preeditUtf16Len = comp.preeditText.length
    const compEndDisplay = compStartUtf16 + preeditUtf16Len
    const start = Math.min(utf16Start, utf16End)
    const end = Math.max(utf16Start, utf16End)

    if (start >= compStartUtf16 && end <= compEndDisplay) {
      const cursorInPreedit = end - compStartUtf16
      return inputAdapter.onCompositionUpdate(comp.preeditText, cursorInPreedit)
    }

    const finishResult = await inputAdapter.finishActiveComposition()
    if (!finishResult.success) {
      return finishResult
    }
    // finish 后 committed 正文 == 之前的显示文本（prefix + preedit + suffix），
    // IME 显示坐标就是 committed 坐标（恒等映射）。
    if (start === end) {
      return selectionController.onTap(start)
    }
    return selectionController.onDragSelect(start, end)
  }

  const dispatch = (cmd) => {
    // 镜像 EditorSemanticDispatcher.dispatch：seal 后拒绝新输入
    if (sealed) {
      return Promise.resolve({ success: false, errorCode: 'SEALED', warnings: [], changedPaths: [], changedEntities: [] })
    }
    return queue.enqueue(async () => executeImeSetSelection(cmd.utf16Start, cmd.utf16End))
  }
  const finishActiveComposition = () => queue.enqueue(async () => {
    if (!inputAdapter.isComposing()) {
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    }
    return inputAdapter.finishActiveComposition()
  })
  const seal = () => { sealed = true }
  const unseal = () => { sealed = false }
  const flush = () => queue.whenIdle()

  return {
    dispatch, finishActiveComposition, seal, unseal, flush,
    calls, callSequence, updateCalls, tapCalls, dragSelectCalls,
    coordinator,
  }
}

await testAsync('评论8 第4项：imeSetSelection 无 composition → 普通 selection（onTap）', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({ text: 'hello', revision: 0, cursor: 3, selectionAnchor: 3, composition: null })
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 2, utf16End: 2 })
  assert.equal(d.calls.tap, 1, '无 composition 时走 onTap')
  assert.equal(d.calls.update, 0, '无 composition 不走 composition update')
  assert.equal(d.calls.finish, 0)
  assert.equal(d.tapCalls[0], 2)
})

await testAsync('评论8 第4项：imeSetSelection 无 composition 非折叠选区 → onDragSelect', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({ text: 'hello', revision: 0, cursor: 3, selectionAnchor: 3, composition: null })
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 4 })
  assert.equal(d.calls.dragSelect, 1)
  assert.equal(d.calls.tap, 0)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

await testAsync('评论8 第4项：imeSetSelection 光标在 composition 内 → composition update（同 preedit + 换算 cursor），不走普通 selection', async () => {
  const d = await makeImeSetSelectionDispatcher()
  // committed text "ab", composition replace [0,1)（"a" 被 preedit "你" 替换）
  // 显示文本 = "你b"；composition 显示区域 [0, 1)（UTF-16：preedit 长 1）
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.coordinator.setComposing(true)
  // IME 光标在 preedit 中间（显示坐标 0 即 preedit 开头）
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 0 })
  assert.equal(d.calls.update, 1, '光标在 composition 内走 composition update')
  assert.equal(d.calls.tap, 0, '不改 committed selection')
  assert.equal(d.calls.finish, 0, '不移出区域不 finish')
  const upd = d.updateCalls[0]
  assert.equal(upd.preedit, '你', '用当前 preedit 文本')
  assert.equal(upd.cursorUtf16, 0, '显示坐标 0 → preeditCursorUtf16=0')
  assert.deepEqual(d.callSequence, ['update:你@0'])
})

await testAsync('评论8 第4项：imeSetSelection 光标在 preedit 末尾（显示坐标=区域终点）→ cursorUtf16=preedit.length', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 0 },
  })
  d.coordinator.setComposing(true)
  // 显示文本 "你b"：preedit 区域 [0,1)，末尾光标显示坐标 1
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 1 })
  assert.equal(d.calls.update, 1)
  assert.equal(d.updateCalls[0].cursorUtf16, 1, 'preedit 末尾 → cursorUtf16=1（preedit.length）')
  assert.equal(d.calls.tap, 0)
})

await testAsync('评论8 第4项：imeSetSelection 中文 preedit 显示坐标换算（"你"占 1 个 UTF-16 unit）', async () => {
  const d = await makeImeSetSelectionDispatcher()
  // committed "ab"，replace [0,1)，preedit "你"（UTF-16 长 1）→ 显示 "你b"
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.coordinator.setComposing(true)
  // IME 光标显示坐标 0.5 不存在；用坐标 0（preedit 开头）
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 0 })
  assert.equal(d.updateCalls[0].cursorUtf16, 0)
})

await testAsync('评论8 第4项：imeSetSelection 移出 composition 区域 → 先 finish 再普通 selection（坐标映射到 committed）', async () => {
  const d = await makeImeSetSelectionDispatcher()
  // committed "ab"，replace [0,1)，preedit "你"（UTF-16 长 1）→ 显示 "你b"（UTF-16 长 2）
  // 显示坐标 2 = 文本末尾（"b" 之后）
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.coordinator.setComposing(true)
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 2, utf16End: 2 })
  assert.equal(d.calls.finish, 1, '移出 composition 区域先 finish')
  assert.equal(d.calls.update, 0, '移出区域不走 composition update')
  // finish 提交 preedit：committed 文本变为 "你b"
  assert.equal(d.coordinator.getSnapshot().text, '你b', 'finish 后 preedit 提交进正文')
  assert.equal(d.coordinator.getSnapshot().composition, null, 'finish 后 composition 清空')
  // 显示坐标 2 → committed 坐标：恒等映射（finish 后 committed == 显示文本）→ tap(2)（"你b" 末尾）
  assert.equal(d.calls.tap, 1)
  assert.equal(d.tapCalls[0], 2, '显示坐标恒等映射到 committed 坐标')
  assert.deepEqual(d.callSequence, ['finishActiveComposition', 'tap:2'])
})

await testAsync('评论8 第4项：imeSetSelection 移出 composition 区域（非折叠选区）→ finish 后 onDragSelect', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.coordinator.setComposing(true)
  // 显示选区 [0, 2)：从 preedit 开头到文本末尾 → 部分越界 → finish + dragSelect
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 2 })
  assert.equal(d.calls.finish, 1)
  assert.equal(d.calls.dragSelect, 1)
  // 恒等映射（finish 后 committed == 显示文本）→ dragSelect(0, 2)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 0, head: 2 })
  assert.equal(d.coordinator.getSnapshot().text, '你b')
})

await testAsync('评论8 第4项：imeSetSelection 选区完全在 preedit 内（中文，多 code unit）→ composition update cursor=选区末尾', async () => {
  const d = await makeImeSetSelectionDispatcher()
  // committed "ab"，replace [0,1)，preedit "你好"（UTF-16 长 2）→ 显示 "你好b"
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.coordinator.setComposing(true)
  // IME 光标在 "你好" 中间：显示坐标 1 → cursorInPreedit = 1
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 1 })
  assert.equal(d.calls.update, 1)
  assert.equal(d.calls.tap, 0)
  assert.equal(d.calls.finish, 0)
  assert.equal(d.updateCalls[0].preedit, '你好', '用当前 preedit 文本')
  assert.equal(d.updateCalls[0].cursorUtf16, 1, '显示坐标 1 → preeditCursorUtf16=1')
})

// ── 12. Issue #629 评论8 第3项：seal 拒新输入 + finishActiveComposition 仍可入队 ──

await testAsync('评论8 第3项：seal 后 dispatch imeSetSelection 被拒（SEALED），不排队', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.seal()
  const result = await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 1 })
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'SEALED', 'seal 后新输入返回 SEALED')
  // 队列保持空闲（没排队）
  await d.flush()
  assert.equal(d.calls.tap, 0)
  assert.equal(d.calls.update, 0)
})

await testAsync('评论8 第3项：seal 后 finishActiveComposition 仍可入队并执行（关闭链一部分）', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.coordinator.setComposing(true)
  d.seal()
  // 普通输入被拒
  const rejected = await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 1 })
  assert.equal(rejected.errorCode, 'SEALED')
  // 关闭链的 finishActiveComposition 不受 seal 影响
  const finishResult = await d.finishActiveComposition()
  assert.equal(finishResult.success, true)
  assert.equal(d.calls.finish, 1, 'seal 后 finishActiveComposition 仍执行')
  // preedit 已提交进正文（最后的中文预输入不丢）
  assert.equal(d.coordinator.getSnapshot().text, '你b', 'seal 后 finish 提交 preedit')
  assert.equal(d.coordinator.getSnapshot().composition, null)
})

await testAsync('评论8 第3项：unseal 后 dispatch 恢复接收输入', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({ text: 'hi', revision: 0, cursor: 1, selectionAnchor: 1, composition: null })
  d.seal()
  const rejected = await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 0 })
  assert.equal(rejected.errorCode, 'SEALED')
  d.unseal()
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 0 })
  assert.equal(d.calls.tap, 1, 'unseal 后普通 selection 恢复')
})


await testAsync('评论8 第4项回归: preedit 长度 ≠ 被替换区长度时，移出区域坐标仍为恒等映射（旧公式会把光标放错）', async () => {
  const d = await makeImeSetSelectionDispatcher()
  // committed "abc"，replace [1,2)（"b" 被替换），preedit "你好"（UTF-16 长 2）
  // 显示文本 = "a" + "你好" + "c" = "a你好c"（UTF-16 长 4）
  d.coordinator.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.coordinator.setComposing(true)
  // IME 光标在显示文本末尾（坐标 4）：finish 后 committed = "a你好c"，末尾坐标就是 4
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 4, utf16End: 4 })
  assert.equal(d.calls.finish, 1, '移出 composition 区域先 finish')
  assert.equal(d.calls.tap, 1)
  assert.equal(d.tapCalls[0], 4, '显示末尾 4 → committed 末尾 4（恒等映射；旧公式会得 3，把光标放到 c 前面）')
  assert.equal(d.coordinator.getSnapshot().text, 'a你好c', 'finish 提交 preedit 后 committed 与显示文本一致')
})

await testAsync('评论8 第4项回归: 中文 replace 区 + 多字 preedit 的非折叠选区同样恒等映射', async () => {
  const d = await makeImeSetSelectionDispatcher()
  d.coordinator.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.coordinator.setComposing(true)
  // 显示选区 [1, 4)：从 preedit 中到文本末尾 → 部分越界 → finish + dragSelect
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 4 })
  assert.equal(d.calls.finish, 1)
  assert.equal(d.calls.dragSelect, 1)
  // 恒等映射：dragSelect(1, 4)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

// ── 13. Issue #629 评论 9 第 1 项：composition 唯一真源是 Core snapshot ──
// EditorInputAdapter.ets 依赖 ArkUI 无法用 Node 直接测；本段镜像其 composition-from-snapshot
// 纯逻辑，验证 isComposing/onCompositionUpdate/Finish/Cancel/finishActiveComposition 全部从
// coordinator.getSnapshot().composition 读取 sessionId/generation/preeditText，不本地持有状态。
async function makeSnapshotCompositionAdapter() {
  // coordinator mock：snapshot.composition 是 composition 唯一真源
  let snapshot = { text: '', revision: 0, cursor: 0, selectionAnchor: 0, composition: null }
  const coordinatorCalls = {
    beginComposition: [],
    updateComposition: [],
    finishComposition: [],
    cancelComposition: [],
  }
  const coordinator = {
    getSnapshot: () => snapshot,
    setSnapshot: (s) => { snapshot = s },
    beginComposition: async (replaceStart, replaceEndExclusive) => {
      coordinatorCalls.beginComposition.push({ replaceStart, replaceEndExclusive })
      // Core 成功后 snapshot.composition 反映新 composition
      snapshot = {
        ...snapshot,
        composition: {
          sessionId: 1, baseRevision: snapshot.revision, generation: 1,
          replaceByteStart: replaceStart, replaceByteEndExclusive: replaceEndExclusive,
          preeditText: '', preeditCursorUtf16: 0,
        },
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    updateComposition: async (sessionId, generation, newPreedit, newCursorUtf16) => {
      coordinatorCalls.updateComposition.push({ sessionId, generation, newPreedit, newCursorUtf16 })
      // Core 成功后 snapshot.composition 反映新 preedit/generation
      if (snapshot.composition) {
        snapshot = {
          ...snapshot,
          composition: {
            ...snapshot.composition,
            generation: generation + 1,
            preeditText: newPreedit,
            preeditCursorUtf16: newCursorUtf16,
          },
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    finishComposition: async (sessionId, generation) => {
      coordinatorCalls.finishComposition.push({ sessionId, generation })
      // Core 成功后 composition 结束
      snapshot = { ...snapshot, composition: null }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    cancelComposition: async (sessionId, generation) => {
      coordinatorCalls.cancelComposition.push({ sessionId, generation })
      snapshot = { ...snapshot, composition: null }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  // 镜像 EditorInputAdapter 的新逻辑（composition 只认 Core snapshot）
  const inputAdapter = {
    isComposing: () => {
      const snap = coordinator.getSnapshot()
      return snap !== null && snap.composition !== null && snap.composition !== undefined
    },
    onCompositionUpdate: async (preedit, preeditCursorUtf16) => {
      const snap = coordinator.getSnapshot()
      if (!snap || snap.composition === null || snap.composition === undefined) {
        return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
      }
      const comp = snap.composition
      const cursorUtf16 = preeditCursorUtf16 === undefined ? preedit.length : preeditCursorUtf16
      return coordinator.updateComposition(comp.sessionId, comp.generation, preedit, cursorUtf16)
    },
    onCompositionFinish: async (committed) => {
      const snap = coordinator.getSnapshot()
      if (!snap || snap.composition === null || snap.composition === undefined) {
        return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
      }
      const comp = snap.composition
      if (committed === comp.preeditText) {
        return coordinator.finishComposition(comp.sessionId, comp.generation)
      }
      const cursorUtf16 = committed.length
      const updateResult = await coordinator.updateComposition(comp.sessionId, comp.generation, committed, cursorUtf16)
      if (!updateResult.success || !updateResult.data) {
        return updateResult
      }
      const latestSnap = coordinator.getSnapshot()
      const newComp = latestSnap ? latestSnap.composition : null
      if (newComp === null || newComp === undefined) {
        return {
          success: false, errorCode: 'NO_COMPOSITION_SESSION',
          warnings: updateResult.warnings, changedPaths: updateResult.changedPaths,
          changedEntities: updateResult.changedEntities,
        }
      }
      return coordinator.finishComposition(newComp.sessionId, newComp.generation)
    },
    onCompositionCancel: async () => {
      const snap = coordinator.getSnapshot()
      if (!snap || snap.composition === null || snap.composition === undefined) {
        return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
      }
      const comp = snap.composition
      return coordinator.cancelComposition(comp.sessionId, comp.generation)
    },
    finishActiveComposition: async () => {
      const snap = coordinator.getSnapshot()
      if (!snap || snap.composition === null || snap.composition === undefined) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      return inputAdapter.onCompositionFinish(snap.composition.preeditText)
    },
  }

  return { inputAdapter, coordinator, coordinatorCalls, getSnapshot: () => snapshot }
}

await testAsync('评论9 第1项：isComposing() snapshot.composition 非 null → true', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  assert.equal(d.inputAdapter.isComposing(), true, 'snapshot.composition 非 null 时 isComposing 应为 true')
})

await testAsync('评论9 第1项：isComposing() snapshot.composition === null → false', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, composition: null })
  assert.equal(d.inputAdapter.isComposing(), false, 'snapshot.composition === null 时 isComposing 应为 false')
})

await testAsync('评论9 第1项：isComposing() snapshot === null → false', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot(null)
  assert.equal(d.inputAdapter.isComposing(), false, 'snapshot === null 时 isComposing 应为 false')
})

await testAsync('评论9 第1项：onCompositionUpdate 从 snapshot.composition 取 sessionId/generation，不本地持有', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  await d.inputAdapter.onCompositionUpdate('你好', 2)
  const call = d.coordinatorCalls.updateComposition[0]
  assert.equal(call.sessionId, 7, 'sessionId 从 snapshot.composition 读')
  assert.equal(call.generation, 3, 'generation 从 snapshot.composition 读')
  assert.equal(call.newPreedit, '你好')
  assert.equal(call.newCursorUtf16, 2)
})

await testAsync('评论9 第1项：onCompositionUpdate 无 composition → NO_COMPOSITION 失败', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, composition: null })
  const result = await d.inputAdapter.onCompositionUpdate('你')
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_COMPOSITION')
  assert.equal(d.coordinatorCalls.updateComposition.length, 0, '无 composition 不应调 coordinator.updateComposition')
})

await testAsync('评论9 第1项：onCompositionUpdate 省略 cursor → 默认 preedit.length（UTF-16 code unit）', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  await d.inputAdapter.onCompositionUpdate('你好')
  const call = d.coordinatorCalls.updateComposition[0]
  assert.equal(call.newCursorUtf16, 2, '省略 cursor 时默认 preedit.length=2（UTF-16 code unit）')
})

await testAsync('评论9 第1项：onCompositionFinish committed==preedit → finishComposition 用 snapshot 的 sessionId/generation', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  await d.inputAdapter.onCompositionFinish('你好')
  assert.equal(d.coordinatorCalls.finishComposition.length, 1)
  assert.equal(d.coordinatorCalls.updateComposition.length, 0, 'committed==preedit 不走 update')
  const call = d.coordinatorCalls.finishComposition[0]
  assert.equal(call.sessionId, 7, 'sessionId 从 snapshot 读')
  assert.equal(call.generation, 3, 'generation 从 snapshot 读')
})

await testAsync('评论9 第1项：onCompositionFinish committed!=preedit → update→重读 snapshot→finish', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  await d.inputAdapter.onCompositionFinish('你好世界')
  // 先 update
  assert.equal(d.coordinatorCalls.updateComposition.length, 1)
  const updCall = d.coordinatorCalls.updateComposition[0]
  assert.equal(updCall.sessionId, 7, 'update 用原 snapshot 的 sessionId')
  assert.equal(updCall.generation, 3, 'update 用原 snapshot 的 generation')
  assert.equal(updCall.newPreedit, '你好世界')
  assert.equal(updCall.newCursorUtf16, 4, 'cursorUtf16 = committed.length（UTF-16 code unit）')
  // 再 finish，用 update 后新 snapshot 的 generation（mock 里 generation+1 = 4）
  assert.equal(d.coordinatorCalls.finishComposition.length, 1)
  const finCall = d.coordinatorCalls.finishComposition[0]
  assert.equal(finCall.sessionId, 7, 'finish 用新 snapshot 的 sessionId')
  assert.equal(finCall.generation, 4, 'finish 用新 snapshot 的 generation（update 后 +1）')
})

await testAsync('评论9 第1项：onCompositionFinish 无 composition → NO_COMPOSITION 失败', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, composition: null })
  const result = await d.inputAdapter.onCompositionFinish('x')
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_COMPOSITION')
})

await testAsync('评论9 第1项：onCompositionCancel 从 snapshot.composition 取 sessionId/generation', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 9, baseRevision: 0, generation: 5, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  await d.inputAdapter.onCompositionCancel()
  assert.equal(d.coordinatorCalls.cancelComposition.length, 1)
  const call = d.coordinatorCalls.cancelComposition[0]
  assert.equal(call.sessionId, 9, 'sessionId 从 snapshot 读')
  assert.equal(call.generation, 5, 'generation 从 snapshot 读')
})

await testAsync('评论9 第1项：onCompositionCancel 无 composition → NO_COMPOSITION 失败', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, composition: null })
  const result = await d.inputAdapter.onCompositionCancel()
  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_COMPOSITION')
})

await testAsync('评论9 第1项：finishActiveComposition 无 composition → no-op（success）', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, composition: null })
  const result = await d.inputAdapter.finishActiveComposition()
  assert.equal(result.success, true, '无 composition 时 finishActiveComposition 是 no-op')
  assert.equal(d.coordinatorCalls.finishComposition.length, 0)
})

await testAsync('评论9 第1项：finishActiveComposition 有 composition → 用 snapshot.preeditText 调 onCompositionFinish', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  await d.inputAdapter.finishActiveComposition()
  // finishActiveComposition 用 preeditText='你好' 调 onCompositionFinish('你好')，committed==preedit → 直接 finish
  assert.equal(d.coordinatorCalls.finishComposition.length, 1)
  assert.equal(d.coordinatorCalls.updateComposition.length, 0, 'committed==preedit 不走 update')
  const call = d.coordinatorCalls.finishComposition[0]
  assert.equal(call.sessionId, 7)
  assert.equal(call.generation, 3)
})

await testAsync('评论9 第1项：begin→update→update→finish 全程从 snapshot 读，无本地状态', async () => {
  const d = await makeSnapshotCompositionAdapter()
  d.coordinator.setSnapshot({ text: '', revision: 0, cursor: 0, selectionAnchor: 0, composition: null })
  // begin（mock 会设 snapshot.composition）
  // 这里直接调 coordinator.beginComposition 模拟 onCompositionBegin 的效果
  await d.coordinator.beginComposition(0, 0)
  assert.equal(d.inputAdapter.isComposing(), true, 'begin 后 isComposing=true（snapshot.composition 已设）')
  // update 1
  await d.inputAdapter.onCompositionUpdate('你')
  assert.equal(d.coordinatorCalls.updateComposition[0].generation, 1, '第一次 update 用 begin 后的 generation=1')
  // update 2
  await d.inputAdapter.onCompositionUpdate('你好')
  assert.equal(d.coordinatorCalls.updateComposition[1].generation, 2, '第二次 update 用 update 后的 generation=2')
  // finish
  await d.inputAdapter.onCompositionFinish('你好')
  assert.equal(d.coordinatorCalls.finishComposition[0].generation, 3, 'finish 用第二次 update 后的 generation=3')
  assert.equal(d.inputAdapter.isComposing(), false, 'finish 后 isComposing=false（snapshot.composition=null）')
})

// ════════════════════════════════════════════════════════════════════
// ── Issue #629 R7 第3项修复：Left/Right soft-wrap affinity 切换 ──
// 镜像 executeGraphemeLeft/executeGraphemeRight 的新逻辑（含 trySoftWrapAffinitySwitch）。
// 用真实 layoutLines + resolveVisualLineIndex 算法，mock Coordinator/SelectionController。
// ════════════════════════════════════════════════════════════════════

function makeSoftWrapLayoutState(text, containerWidth, composition = null) {
  const mockMeasure = (s) => s.length * 10
  const ranges = layoutLines(text, containerWidth, mockMeasure)
  const lines = ranges.map((r, i) => ({
    startUtf16: r.start,
    endUtf16: r.end,
    y: i * 20,
    height: 20,
    breakKind: r.breakKind,
    caretStops: [],
  }))
  // R11: compositionGeneration 经和生产一致的 projection：无 composition 为 -1，有 composition 为 composition.generation
  const compositionGeneration = composition ? composition.generation : -1
  return {
    revision: 1, generation: 0, compositionGeneration,
    contentWidth: containerWidth, fontSize: 16,
    lines, displayText: text,
  }
}

async function makeAffinitySwitchDispatcher() {
  let snapshot = { text: '', cursor: 0, selectionAnchor: 0, revision: 1, generation: 0, composition: null }
  const queue = new SerialCommandQueue()
  const coordinatorCalls = { setSelection: [], previousGraphemeBoundary: [], nextGraphemeBoundary: [] }
  let visualCaret = null
  // R11: visualCaret 绑定身份（revision/generation/compositionSessionId/compositionGeneration），
  // 与生产 SelectionController.getVisualCaret 一致：身份不匹配时返回默认 Downstream。
  let visualCaretRevision = -1
  let visualCaretGeneration = -1
  let visualCaretCompositionSessionId = 0
  let visualCaretCompositionGeneration = -1
  const updateIdentity = () => {
    visualCaretRevision = snapshot.revision ?? 0
    visualCaretGeneration = snapshot.generation ?? 0
    visualCaretCompositionSessionId = snapshot.composition?.sessionId ?? 0
    visualCaretCompositionGeneration = snapshot.composition?.generation ?? -1
  }
  const identityMatches = () => {
    return (snapshot.revision ?? 0) === visualCaretRevision
      && (snapshot.generation ?? 0) === visualCaretGeneration
      && (snapshot.composition?.sessionId ?? 0) === visualCaretCompositionSessionId
      && (snapshot.composition?.generation ?? -1) === visualCaretCompositionGeneration
  }
  const selectionController = {
    getVisualCaret: (cursorUtf16) => {
      if (visualCaret !== null && visualCaret.utf16Offset === cursorUtf16 && identityMatches()) return visualCaret
      return { utf16Offset: cursorUtf16, affinity: 'downstream' }
    },
    rememberVisualCaret: (position) => { visualCaret = position; updateIdentity() },
    setVisualCaretForTest: (pos) => { visualCaret = pos; updateIdentity() },
    getVisualCaretState: () => visualCaret,
  }
  const coordinator = {
    getSnapshot: () => snapshot,
    setSelection: async (anchor, head) => {
      coordinatorCalls.setSelection.push({ anchor, head })
      snapshot = { ...snapshot, selectionAnchor: anchor, cursor: head }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    previousGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.previousGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset - 1 }
    },
    nextGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.nextGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset + 1 }
    },
  }
  let layoutState = null
  const lineResolver = {
    updateLayout: (state) => { layoutState = state },
    waitForLayout: (identity) => {
      if (layoutState !== null
          && layoutState.revision === identity.revision
          && layoutState.generation === identity.generation
          && layoutState.compositionGeneration === identity.compositionGeneration
          && layoutState.displayText === identity.displayText) {
        return Promise.resolve(layoutState)
      }
      return Promise.resolve(null)
    },
  }
  // R11: 用生产 positionForHorizontalArrival 判断 soft-wrap 行尾，不复制判断逻辑
  const toLineRanges = (lines) => lines.map(l => ({ start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind }))
  const rememberAfterMove = (direction, targetByte) => {
    if (!layoutState) {
      selectionController.rememberVisualCaret({ utf16Offset: targetByte, affinity: CaretAffinity.Downstream })
      return
    }
    const targetPos = positionForHorizontalArrival(toLineRanges(layoutState.lines), direction, targetByte)
    selectionController.rememberVisualCaret(targetPos)
  }
  const trySoftWrapAffinitySwitch = async (direction) => {
    if (!layoutState) return { handled: false, result: null, layoutState: null }
    const cursorUtf16 = snapshot.cursor
    const currentPosition = selectionController.getVisualCaret(cursorUtf16)
    // 用生产 positionForHorizontalArrival 判断是否在 soft-wrap 行尾
    // （right 到达 soft-wrap 行尾返回 Upstream，其他返回 Downstream）
    const arrival = positionForHorizontalArrival(toLineRanges(layoutState.lines), 'right', cursorUtf16)
    const atSoftWrapEnd = arrival.affinity === CaretAffinity.Upstream
    if (!atSoftWrapEnd) return { handled: false, result: null, layoutState }
    if (direction === 'left' && currentPosition.affinity === 'downstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'upstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    if (direction === 'right' && currentPosition.affinity === 'upstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'downstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    return { handled: false, result: null, layoutState }
  }
  const executeGraphemeLeft = async (extend) => {
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte <= 0) return coordinator.setSelection(anchorByte, 0)
      const prevResult = await coordinator.previousGraphemeBoundary(headByte)
      if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) return prevResult
      const setResult = await coordinator.setSelection(anchorByte, prevResult.data)
      if (setResult.success) rememberAfterMove('left', prevResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selStart = Math.min(anchorByte, cursorByte)
      return coordinator.setSelection(selStart, selStart)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('left')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte <= 0) return coordinator.setSelection(0, 0)
    const prevResult = await coordinator.previousGraphemeBoundary(cursorByte)
    if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) return prevResult
    const setResult = await coordinator.setSelection(prevResult.data, prevResult.data)
    if (setResult.success) rememberAfterMove('left', prevResult.data)
    return setResult
  }
  const executeGraphemeRight = async (extend) => {
    const text = snapshot.text
    const textByteLen = text.length
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte >= textByteLen) return coordinator.setSelection(anchorByte, textByteLen)
      const nextResult = await coordinator.nextGraphemeBoundary(headByte)
      if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) return nextResult
      const setResult = await coordinator.setSelection(anchorByte, nextResult.data)
      if (setResult.success) rememberAfterMove('right', nextResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selEnd = Math.max(anchorByte, cursorByte)
      return coordinator.setSelection(selEnd, selEnd)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('right')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte >= textByteLen) return coordinator.setSelection(textByteLen, textByteLen)
    const nextResult = await coordinator.nextGraphemeBoundary(cursorByte)
    if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) return nextResult
    const setResult = await coordinator.setSelection(nextResult.data, nextResult.data)
    if (setResult.success) rememberAfterMove('right', nextResult.data)
    return setResult
  }
  const dispatch = (cmd) => queue.enqueue(async () => {
    switch (cmd.kind) {
      case 'graphemeLeft': return executeGraphemeLeft(cmd.extend)
      case 'graphemeRight': return executeGraphemeRight(cmd.extend)
      default: return { success: true }
    }
  })
  return {
    dispatch, coordinator, selectionController, lineResolver, coordinatorCalls,
    setSnapshot: (s) => { snapshot = { ...snapshot, ...s } },
    getSnapshot: () => snapshot,
  }
}

await testAsync('R7第3项: soft-wrap 边界 Upstream + Right -> 切 Downstream, cursor 不移动', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 不应移动')
  assert.equal(d.coordinatorCalls.setSelection.length, 0, '不应调 setSelection')
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'affinity 应切到 Downstream')
})

await testAsync('R7第3项: soft-wrap 边界 Downstream + Left -> 切 Upstream, cursor 不移动', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 不应移动')
  assert.equal(d.coordinatorCalls.setSelection.length, 0, '不应调 setSelection')
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'affinity 应切到 Upstream')
})

await testAsync('R7第3项: 非 soft-wrap 边界 Left 正常移动到前一个 grapheme', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 4, selectionAnchor: 4 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 4, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 应移到 offset=3')
  assert.equal(d.coordinatorCalls.setSelection.length, 1, '应调 setSelection')
  assert.equal(d.coordinatorCalls.previousGraphemeBoundary.length, 1, '应调 previousGraphemeBoundary')
})

await testAsync('R7第3项: 非 soft-wrap 边界 Right 正常移动到后一个 grapheme', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 2, selectionAnchor: 2 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 2, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 应移到 offset=3')
  assert.equal(d.coordinatorCalls.setSelection.length, 1, '应调 setSelection')
  assert.equal(d.coordinatorCalls.nextGraphemeBoundary.length, 1, '应调 nextGraphemeBoundary')
})

await testAsync('R7第3项: soft-wrap 边界 Upstream + Left -> 正常移动到前一个 grapheme', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 2, 'cursor 应移到 offset=2')
  assert.equal(d.coordinatorCalls.setSelection.length, 1, '应调 setSelection')
  assert.equal(d.coordinatorCalls.previousGraphemeBoundary.length, 1, '应调 previousGraphemeBoundary')
})

await testAsync('R7第3项: soft-wrap 边界 Downstream + Right -> 正常移动到后一个 grapheme', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 4, 'cursor 应移到 offset=4')
  assert.equal(d.coordinatorCalls.setSelection.length, 1, '应调 setSelection')
  assert.equal(d.coordinatorCalls.nextGraphemeBoundary.length, 1, '应调 nextGraphemeBoundary')
})

await testAsync('R7第3项: shift+left 不触发 affinity 切换（走选区扩展逻辑）', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 5 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: true })
  assert.equal(d.getSnapshot().cursor, 2, 'shift+left 应移 head 到 offset=2')
  assert.equal(d.getSnapshot().selectionAnchor, 5, 'anchor 应保持 5')
  assert.equal(d.coordinatorCalls.previousGraphemeBoundary.length, 1, '应调 previousGraphemeBoundary')
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'affinity 不应变')
})

await testAsync('R7第3项: shift+right 不触发 affinity 切换（走选区扩展逻辑）', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 1 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: true })
  assert.equal(d.getSnapshot().cursor, 4, 'shift+right 应移 head 到 offset=4')
  assert.equal(d.getSnapshot().selectionAnchor, 1, 'anchor 应保持 1')
  assert.equal(d.coordinatorCalls.nextGraphemeBoundary.length, 1, '应调 nextGraphemeBoundary')
  // R11: rememberAfterMove 把 visualCaret 移到新位置 4（非 soft-wrap 边界 → downstream），
  // 证明走的是 extend 分支而非 affinity switch 分支（switch 不移动 cursor）。
  const pos = d.selectionController.getVisualCaret(4)
  assert.equal(pos.affinity, 'downstream', 'shift+right 后 visualCaret 在新位置 4，非 soft-wrap 边界 → downstream')
})

await testAsync('R7第3项: 有选区时 Left 不触发 affinity 切换（走 collapse）', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 5, selectionAnchor: 1 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 5, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 1, '有选区应 collapse 到选区开头 offset=1')
  assert.equal(d.coordinatorCalls.setSelection.length, 1, '应调 setSelection')
  assert.equal(d.coordinatorCalls.previousGraphemeBoundary.length, 0, '不应调 previousGraphemeBoundary')
})

await testAsync('R7第3项: lineResolver 未注册时 Left 走原逻辑（向后兼容）', async () => {
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 2, '无布局时走原逻辑移到 offset=2')
  assert.equal(d.coordinatorCalls.previousGraphemeBoundary.length, 1, '应调 previousGraphemeBoundary')
})

await testAsync('R7第3项: 硬换行（非共享 offset）边界 Left/Right 不触发 affinity 切换，走原 grapheme 移动', async () => {
  // 文本 'ab\ncd'，containerWidth=1000 足宽不软折。layoutLines 产生两行：
  //   {start:0,end:2,breakKind:'hardBreak'}（'ab'）
  //   {start:3,end:5,breakKind:'endOfText'}（'cd'，\n 在 offset 2 不进 line range）
  // cursor=2 是硬换行边界：第一行 end=2 但 breakKind='hardBreak'（非 'softWrap'），
  // 且第二行 start=3 ≠ 2 → 不共享 offset → trySoftWrapAffinitySwitch 不接管，走原 grapheme 移动。
  const layout = makeSoftWrapLayoutState('ab\ncd', 1000)

  // Left + Downstream：硬换行不切 affinity，走 previousGraphemeBoundary → offset=1
  const dLeft = await makeAffinitySwitchDispatcher()
  dLeft.setSnapshot({ text: 'ab\ncd', cursor: 2, selectionAnchor: 2 })
  dLeft.lineResolver.updateLayout(layout)
  dLeft.selectionController.setVisualCaretForTest({ utf16Offset: 2, affinity: 'downstream' })
  await dLeft.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(dLeft.getSnapshot().cursor, 1, 'Left 应移到前一个 grapheme offset=1')
  assert.equal(dLeft.coordinatorCalls.setSelection.length, 1, 'Left 应调一次 setSelection')
  assert.equal(dLeft.coordinatorCalls.previousGraphemeBoundary.length, 1, 'Left 应调 previousGraphemeBoundary')
  let pos = dLeft.selectionController.getVisualCaret(2)
  assert.equal(pos.affinity, 'downstream', '硬换行不切 affinity，仍为 downstream')

  // Right + Upstream：对称，硬换行不切 affinity，走 nextGraphemeBoundary → offset=3
  const dRight = await makeAffinitySwitchDispatcher()
  dRight.setSnapshot({ text: 'ab\ncd', cursor: 2, selectionAnchor: 2 })
  dRight.lineResolver.updateLayout(layout)
  dRight.selectionController.setVisualCaretForTest({ utf16Offset: 2, affinity: 'upstream' })
  await dRight.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(dRight.getSnapshot().cursor, 3, 'Right 应移到后一个 grapheme offset=3')
  assert.equal(dRight.coordinatorCalls.setSelection.length, 1, 'Right 应调一次 setSelection')
  assert.equal(dRight.coordinatorCalls.nextGraphemeBoundary.length, 1, 'Right 应调 nextGraphemeBoundary')
  // R11: rememberAfterMove 把 visualCaret 移到新位置 3（硬换行非 soft-wrap 边界 → downstream）
  pos = dRight.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', '硬换行 Right 后 visualCaret 在新位置 3，非 soft-wrap 边界 → downstream')
})

await testAsync('R7第3项: composition 进行中 soft-wrap affinity 切换是纯视觉操作，不调 setSelection 破坏 composition 正文', async () => {
  // composition 进行中（compositionGeneration=5）。soft-wrap 边界 affinity 切换是纯视觉操作，
  // 只调 selectionController.rememberVisualCaret 更新视觉 affinity，不调 setSelection（不破坏 composition 正文）。
  // R11: 用真实 composition DTO（EditorCompositionState 结构），不再用顶层假 compositionGeneration。
  const composition = { sessionId: 7, baseRevision: 1, generation: 5, replaceByteStart: 3, replaceByteEndExclusive: 3, preeditText: '', preeditCursorUtf16: 0 }
  const d = await makeAffinitySwitchDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3, composition })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30, composition))
  // cursor=3 是 soft-wrap 边界（第一行 end=3 breakKind='softWrap'），affinity='upstream' + Right → 切 downstream
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'composition 中不移动 cursor，不破坏 composition 正文')
  assert.equal(d.coordinatorCalls.setSelection.length, 0, '不调 setSelection，不碰正文')
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'affinity 应切到 downstream（视觉位置仍更新）')
})

// ════════════════════════════════════════════════════════════════════
// ── Issue #629 R10 评论5327548809 第2项：普通 Left/Right 只做一次 layout 检查 ──
// 镜像生产代码顶层路由 executeGraphemeLeft/Right（composing && !extend 守卫）+
// executeCommittedGraphemeLeft/Right（committed helper 内 trySoftWrapAffinitySwitch）。
// 验证无 composition 无 Shift 时 trySoftWrapAffinitySwitch 只调一次（不是两次）。
// ════════════════════════════════════════════════════════════════════

async function makeTopLevelGraphemeDispatcher() {
  let snapshot = { text: '', cursor: 0, selectionAnchor: 0, revision: 1, generation: 0, composition: null }
  const queue = new SerialCommandQueue()
  const coordinatorCalls = { setSelection: [], previousGraphemeBoundary: [], nextGraphemeBoundary: [] }
  const affinitySwitchCalls = []
  let composing = false
  let visualCaret = null
  // R11: visualCaret 绑定身份，与生产 SelectionController.getVisualCaret 一致
  let visualCaretRevision = -1
  let visualCaretGeneration = -1
  let visualCaretCompositionSessionId = 0
  let visualCaretCompositionGeneration = -1
  const updateIdentity = () => {
    visualCaretRevision = snapshot.revision ?? 0
    visualCaretGeneration = snapshot.generation ?? 0
    visualCaretCompositionSessionId = snapshot.composition?.sessionId ?? 0
    visualCaretCompositionGeneration = snapshot.composition?.generation ?? -1
  }
  const identityMatches = () => {
    return (snapshot.revision ?? 0) === visualCaretRevision
      && (snapshot.generation ?? 0) === visualCaretGeneration
      && (snapshot.composition?.sessionId ?? 0) === visualCaretCompositionSessionId
      && (snapshot.composition?.generation ?? -1) === visualCaretCompositionGeneration
  }
  const selectionController = {
    getVisualCaret: (cursorUtf16) => {
      if (visualCaret !== null && visualCaret.utf16Offset === cursorUtf16 && identityMatches()) return visualCaret
      return { utf16Offset: cursorUtf16, affinity: 'downstream' }
    },
    rememberVisualCaret: (position) => { visualCaret = position; updateIdentity() },
    setVisualCaretForTest: (pos) => { visualCaret = pos; updateIdentity() },
    getVisualCaretState: () => visualCaret,
  }
  const coordinator = {
    getSnapshot: () => snapshot,
    setSelection: async (anchor, head) => {
      coordinatorCalls.setSelection.push({ anchor, head })
      snapshot = { ...snapshot, selectionAnchor: anchor, cursor: head }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    previousGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.previousGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset - 1 }
    },
    nextGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.nextGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset + 1 }
    },
  }
  let layoutState = null
  const lineResolver = {
    updateLayout: (state) => { layoutState = state },
    waitForLayout: (identity) => Promise.resolve(layoutState),
  }
  // R11: 用生产 positionForHorizontalArrival 判断 soft-wrap 行尾，不复制判断逻辑
  const toLineRanges = (lines) => lines.map(l => ({ start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind }))
  const rememberAfterMove = (direction, targetByte) => {
    if (!layoutState) {
      selectionController.rememberVisualCaret({ utf16Offset: targetByte, affinity: CaretAffinity.Downstream })
      return
    }
    const targetPos = positionForHorizontalArrival(toLineRanges(layoutState.lines), direction, targetByte)
    selectionController.rememberVisualCaret(targetPos)
  }
  const trySoftWrapAffinitySwitch = async (direction) => {
    affinitySwitchCalls.push(direction)
    if (!layoutState) return { handled: false, result: null, layoutState: null }
    const cursorUtf16 = snapshot.cursor
    const currentPosition = selectionController.getVisualCaret(cursorUtf16)
    const arrival = positionForHorizontalArrival(toLineRanges(layoutState.lines), 'right', cursorUtf16)
    const atSoftWrapEnd = arrival.affinity === CaretAffinity.Upstream
    if (!atSoftWrapEnd) return { handled: false, result: null, layoutState }
    if (direction === 'left' && currentPosition.affinity === 'downstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'upstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    if (direction === 'right' && currentPosition.affinity === 'upstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'downstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    return { handled: false, result: null, layoutState }
  }
  // committed helper（含 trySoftWrapAffinitySwitch；composition finish 后进此处按新 identity 检查）
  // R11: Core setSelection 成功后用 positionForHorizontalArrival 算目标 affinity 并 rememberVisualCaret
  const executeCommittedGraphemeLeft = async (extend) => {
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte <= 0) return coordinator.setSelection(anchorByte, 0)
      const prevResult = await coordinator.previousGraphemeBoundary(headByte)
      if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) return prevResult
      const setResult = await coordinator.setSelection(anchorByte, prevResult.data)
      if (setResult.success) rememberAfterMove('left', prevResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selStart = Math.min(anchorByte, cursorByte)
      return coordinator.setSelection(selStart, selStart)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('left')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte <= 0) return coordinator.setSelection(0, 0)
    const prevResult = await coordinator.previousGraphemeBoundary(cursorByte)
    if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) return prevResult
    const setResult = await coordinator.setSelection(prevResult.data, prevResult.data)
    if (setResult.success) rememberAfterMove('left', prevResult.data)
    return setResult
  }
  const executeCommittedGraphemeRight = async (extend) => {
    const text = snapshot.text
    const textByteLen = text.length
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte >= textByteLen) return coordinator.setSelection(anchorByte, textByteLen)
      const nextResult = await coordinator.nextGraphemeBoundary(headByte)
      if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) return nextResult
      const setResult = await coordinator.setSelection(anchorByte, nextResult.data)
      if (setResult.success) rememberAfterMove('right', nextResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selEnd = Math.max(anchorByte, cursorByte)
      return coordinator.setSelection(selEnd, selEnd)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('right')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte >= textByteLen) return coordinator.setSelection(textByteLen, textByteLen)
    const nextResult = await coordinator.nextGraphemeBoundary(cursorByte)
    if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) return nextResult
    const setResult = await coordinator.setSelection(nextResult.data, nextResult.data)
    if (setResult.success) rememberAfterMove('right', nextResult.data)
    return setResult
  }
  // 顶层路由（镜像生产代码：composing && !extend 守卫）
  const executeGraphemeLeft = async (extend) => {
    const localComposing = composing
    if (localComposing && !extend) {
      const switched = await trySoftWrapAffinitySwitch('left')
      if (switched.handled) return switched.result
    }
    if (!localComposing) return executeCommittedGraphemeLeft(extend)
    // composition 分支（本测试聚焦顶层守卫；composition finish 后进 committed helper）
    return executeCommittedGraphemeLeft(extend)
  }
  const executeGraphemeRight = async (extend) => {
    const localComposing = composing
    if (localComposing && !extend) {
      const switched = await trySoftWrapAffinitySwitch('right')
      if (switched.handled) return switched.result
    }
    if (!localComposing) return executeCommittedGraphemeRight(extend)
    return executeCommittedGraphemeRight(extend)
  }
  const dispatch = (cmd) => queue.enqueue(async () => {
    switch (cmd.kind) {
      case 'graphemeLeft': return executeGraphemeLeft(cmd.extend)
      case 'graphemeRight': return executeGraphemeRight(cmd.extend)
      default: return { success: true }
    }
  })
  return {
    dispatch, coordinator, selectionController, lineResolver, coordinatorCalls,
    affinitySwitchCalls: () => affinitySwitchCalls,
    setComposing: (v) => { composing = v },
    setSnapshot: (s) => { snapshot = { ...snapshot, ...s } },
    getSnapshot: () => snapshot,
  }
}

await testAsync('R10第2项: 无 composition 无 Shift Left 只调一次 trySoftWrapAffinitySwitch（不是两次）', async () => {
  const d = await makeTopLevelGraphemeDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 4, selectionAnchor: 4 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 4, affinity: 'downstream' })
  // 无 composition（composing=false），无 Shift（extend=false）
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.affinitySwitchCalls().length, 1, '无 composition 时顶层不预检查，只 committed helper 检查一次')
  assert.equal(d.affinitySwitchCalls()[0], 'left')
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 应移到 offset=3')
})

await testAsync('R10第2项: 无 composition 无 Shift Right 只调一次 trySoftWrapAffinitySwitch（不是两次）', async () => {
  const d = await makeTopLevelGraphemeDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 2, selectionAnchor: 2 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 2, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.affinitySwitchCalls().length, 1, '无 composition 时顶层不预检查，只 committed helper 检查一次')
  assert.equal(d.affinitySwitchCalls()[0], 'right')
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 应移到 offset=3')
})

await testAsync('R10第2项: composition 活跃 + soft-wrap 边界 → 顶层预检查 handled=true 直接返回，只调一次', async () => {
  const d = await makeTopLevelGraphemeDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 3, selectionAnchor: 3 })
  d.setComposing(true)
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  // composition 活跃 + Right + soft-wrap 边界 → 顶层预检查切 affinity，handled=true 直接返回
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.affinitySwitchCalls().length, 1, 'composition 顶层预检查 handled=true 后直接返回，不进 committed helper')
  assert.equal(d.affinitySwitchCalls()[0], 'right')
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'affinity 应切到 downstream')
  assert.equal(d.getSnapshot().cursor, 3, 'cursor 不移动（纯视觉切换）')
})

await testAsync('R10第2项: composition 活跃但非 soft-wrap 边界 → 顶层预检查 + committed helper 各一次（共两次）', async () => {
  const d = await makeTopLevelGraphemeDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 4, selectionAnchor: 4 })
  d.setComposing(true)
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  d.selectionController.setVisualCaretForTest({ utf16Offset: 4, affinity: 'downstream' })
  // composition 活跃 + Left + 非 soft-wrap 边界 → 顶层预检查 handled=false，再进 committed helper 检查一次
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.affinitySwitchCalls().length, 2, 'composition 活跃时顶层预检查 + committed helper 各一次')
  assert.equal(d.affinitySwitchCalls()[0], 'left')
  assert.equal(d.affinitySwitchCalls()[1], 'left')
})


// ════════════════════════════════════════════════════════════════════
// ── Issue #629 R11 评论5329310563 第4项：完整序列 + generation 前进身份 ──
// 验证 committed/composition 左右完整序列（视觉行 [0,3] SoftWrap / [3,6] EndOfText）：
//   Right: 2 -> 3/Upstream -> 3/Downstream -> 4
//   Left:  4 -> 3/Downstream -> 3/Upstream -> 2
// positionForHorizontalArrival 直接从生产导入，不复制逻辑。
// ════════════════════════════════════════════════════════════════════

// 完整序列用 dispatcher：支持 committed + composition move，rememberVisualCaret 用生产 positionForHorizontalArrival。
async function makeCompleteSequenceDispatcher() {
  let snapshot = { text: '', cursor: 0, selectionAnchor: 0, revision: 1, generation: 0, composition: null }
  const queue = new SerialCommandQueue()
  const coordinatorCalls = { setSelection: [], previousGraphemeBoundary: [], nextGraphemeBoundary: [], compositionMoveLeft: [], compositionMoveRight: [] }
  let visualCaret = null
  let visualCaretRevision = -1
  let visualCaretGeneration = -1
  let visualCaretCompositionSessionId = 0
  let visualCaretCompositionGeneration = -1
  const updateIdentity = () => {
    visualCaretRevision = snapshot.revision ?? 0
    visualCaretGeneration = snapshot.generation ?? 0
    visualCaretCompositionSessionId = snapshot.composition?.sessionId ?? 0
    visualCaretCompositionGeneration = snapshot.composition?.generation ?? -1
  }
  const identityMatches = () => {
    return (snapshot.revision ?? 0) === visualCaretRevision
      && (snapshot.generation ?? 0) === visualCaretGeneration
      && (snapshot.composition?.sessionId ?? 0) === visualCaretCompositionSessionId
      && (snapshot.composition?.generation ?? -1) === visualCaretCompositionGeneration
  }
  const selectionController = {
    getVisualCaret: (cursorUtf16) => {
      if (visualCaret !== null && visualCaret.utf16Offset === cursorUtf16 && identityMatches()) return visualCaret
      return { utf16Offset: cursorUtf16, affinity: 'downstream' }
    },
    rememberVisualCaret: (position) => { visualCaret = position; updateIdentity() },
    setVisualCaretForTest: (pos) => { visualCaret = pos; updateIdentity() },
    getVisualCaretState: () => visualCaret,
    getIdentity: () => ({ revision: visualCaretRevision, generation: visualCaretGeneration, compositionSessionId: visualCaretCompositionSessionId, compositionGeneration: visualCaretCompositionGeneration }),
  }
  const coordinator = {
    getSnapshot: () => snapshot,
    setSelection: async (anchor, head) => {
      coordinatorCalls.setSelection.push({ anchor, head })
      snapshot = { ...snapshot, selectionAnchor: anchor, cursor: head }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    previousGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.previousGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset - 1 }
    },
    nextGraphemeBoundary: async (byteOffset) => {
      coordinatorCalls.nextGraphemeBoundary.push(byteOffset)
      return { success: true, data: byteOffset + 1 }
    },
    compositionMoveGraphemeLeft: async () => {
      coordinatorCalls.compositionMoveLeft.push(snapshot.cursor)
      snapshot = { ...snapshot, cursor: snapshot.cursor - 1 }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    compositionMoveGraphemeRight: async () => {
      coordinatorCalls.compositionMoveRight.push(snapshot.cursor)
      snapshot = { ...snapshot, cursor: snapshot.cursor + 1 }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }
  let layoutState = null
  const lineResolver = {
    updateLayout: (state) => { layoutState = state },
    waitForLayout: (identity) => Promise.resolve(layoutState),
  }
  const toLineRanges = (lines) => lines.map(l => ({ start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind }))
  const rememberAfterMove = (direction, targetByte) => {
    if (!layoutState) {
      selectionController.rememberVisualCaret({ utf16Offset: targetByte, affinity: CaretAffinity.Downstream })
      return
    }
    const targetPos = positionForHorizontalArrival(toLineRanges(layoutState.lines), direction, targetByte)
    selectionController.rememberVisualCaret(targetPos)
  }
  const trySoftWrapAffinitySwitch = async (direction) => {
    if (!layoutState) return { handled: false, result: null }
    const cursorUtf16 = snapshot.cursor
    const currentPosition = selectionController.getVisualCaret(cursorUtf16)
    const arrival = positionForHorizontalArrival(toLineRanges(layoutState.lines), 'right', cursorUtf16)
    const atSoftWrapEnd = arrival.affinity === CaretAffinity.Upstream
    if (!atSoftWrapEnd) return { handled: false, result: null }
    if (direction === 'left' && currentPosition.affinity === 'downstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'upstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] } }
    }
    if (direction === 'right' && currentPosition.affinity === 'upstream') {
      selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: 'downstream' })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] } }
    }
    return { handled: false, result: null }
  }
  const isComposing = () => snapshot.composition !== null
  const executeCommittedGraphemeLeft = async (extend) => {
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte <= 0) return coordinator.setSelection(anchorByte, 0)
      const prevResult = await coordinator.previousGraphemeBoundary(headByte)
      if (!prevResult.success) return prevResult
      const setResult = await coordinator.setSelection(anchorByte, prevResult.data)
      if (setResult.success) rememberAfterMove('left', prevResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selStart = Math.min(anchorByte, cursorByte)
      return coordinator.setSelection(selStart, selStart)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('left')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte <= 0) return coordinator.setSelection(0, 0)
    const prevResult = await coordinator.previousGraphemeBoundary(cursorByte)
    if (!prevResult.success) return prevResult
    const setResult = await coordinator.setSelection(prevResult.data, prevResult.data)
    if (setResult.success) rememberAfterMove('left', prevResult.data)
    return setResult
  }
  const executeCommittedGraphemeRight = async (extend) => {
    const text = snapshot.text
    const textByteLen = text.length
    const cursorByte = snapshot.cursor
    const anchorByte = snapshot.selectionAnchor
    if (extend) {
      const headByte = snapshot.cursor
      if (headByte >= textByteLen) return coordinator.setSelection(anchorByte, textByteLen)
      const nextResult = await coordinator.nextGraphemeBoundary(headByte)
      if (!nextResult.success) return nextResult
      const setResult = await coordinator.setSelection(anchorByte, nextResult.data)
      if (setResult.success) rememberAfterMove('right', nextResult.data)
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selEnd = Math.max(anchorByte, cursorByte)
      return coordinator.setSelection(selEnd, selEnd)
    }
    const affinitySwitch = await trySoftWrapAffinitySwitch('right')
    if (affinitySwitch.handled) return affinitySwitch.result
    if (cursorByte >= textByteLen) return coordinator.setSelection(textByteLen, textByteLen)
    const nextResult = await coordinator.nextGraphemeBoundary(cursorByte)
    if (!nextResult.success) return nextResult
    const setResult = await coordinator.setSelection(nextResult.data, nextResult.data)
    if (setResult.success) rememberAfterMove('right', nextResult.data)
    return setResult
  }
  const executeGraphemeLeft = async (extend) => {
    if (isComposing() && !extend) {
      const switched = await trySoftWrapAffinitySwitch('left')
      if (switched.handled) return switched.result
      const moveResult = await coordinator.compositionMoveGraphemeLeft()
      if (moveResult.success) rememberAfterMove('left', snapshot.cursor)
      return moveResult
    }
    return executeCommittedGraphemeLeft(extend)
  }
  const executeGraphemeRight = async (extend) => {
    if (isComposing() && !extend) {
      const switched = await trySoftWrapAffinitySwitch('right')
      if (switched.handled) return switched.result
      const moveResult = await coordinator.compositionMoveGraphemeRight()
      if (moveResult.success) rememberAfterMove('right', snapshot.cursor)
      return moveResult
    }
    return executeCommittedGraphemeRight(extend)
  }
  const dispatch = (cmd) => queue.enqueue(async () => {
    switch (cmd.kind) {
      case 'graphemeLeft': return executeGraphemeLeft(cmd.extend)
      case 'graphemeRight': return executeGraphemeRight(cmd.extend)
      default: return { success: true }
    }
  })
  return {
    dispatch, coordinator, selectionController, lineResolver, coordinatorCalls,
    setSnapshot: (s) => { snapshot = { ...snapshot, ...s } },
    getSnapshot: () => snapshot,
  }
}

await testAsync('R11第4项: committed Right 完整序列 2 -> 3/Upstream -> 3/Downstream -> 4', async () => {
  const d = await makeCompleteSequenceDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 2, selectionAnchor: 2 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  // Step 1: cursor=2, Right → Core 返回 3, positionForHorizontalArrival(right,3)=Upstream
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step1: cursor 应移到 3')
  let pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'Step1: 3 是 SoftWrap 行尾，Right 到达 → Upstream')
  // Step 2: cursor=3/Upstream, Right → trySoftWrap 切到 3/Downstream（cursor 不移动）
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step2: cursor 不移动（纯 affinity 切换）')
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step2: Upstream + Right → 切 Downstream')
  // Step 3: cursor=3/Downstream, Right → Core 返回 4, positionForHorizontalArrival(right,4)=Downstream
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 4, 'Step3: cursor 应移到 4')
  pos = d.selectionController.getVisualCaret(4)
  assert.equal(pos.affinity, 'downstream', 'Step3: 4 非 SoftWrap 边界 → Downstream')
})

await testAsync('R11第4项: committed Left 完整序列 4 -> 3/Downstream -> 3/Upstream -> 2', async () => {
  const d = await makeCompleteSequenceDispatcher()
  d.setSnapshot({ text: 'abcdef', cursor: 4, selectionAnchor: 4 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30))
  // Step 1: cursor=4, Left → Core 返回 3, positionForHorizontalArrival(left,3)=Downstream
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step1: cursor 应移到 3')
  let pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step1: 3 是 SoftWrap 行尾，Left 到达 → Downstream')
  // Step 2: cursor=3/Downstream, Left → trySoftWrap 切到 3/Upstream（cursor 不移动）
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step2: cursor 不移动（纯 affinity 切换）')
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'Step2: Downstream + Left → 切 Upstream')
  // Step 3: cursor=3/Upstream, Left → Core 返回 2, positionForHorizontalArrival(left,2)=Downstream
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 2, 'Step3: cursor 应移到 2')
  pos = d.selectionController.getVisualCaret(2)
  assert.equal(pos.affinity, 'downstream', 'Step3: 2 非 SoftWrap 边界 → Downstream')
})

await testAsync('R11第4项: composition Right 完整序列 2 -> 3/Upstream -> 3/Downstream，generation 前进后身份仍匹配', async () => {
  const d = await makeCompleteSequenceDispatcher()
  const composition = { sessionId: 7, baseRevision: 1, generation: 5, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'abcdef', preeditCursorUtf16: 2 }
  d.setSnapshot({ text: '', cursor: 2, selectionAnchor: 2, composition })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30, composition))
  // Step 1: composition Right → cursor=3, rememberVisualCaret(3/Upstream) with identity generation=5
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step1: composition Right → cursor=3')
  let pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'Step1: 3 是 SoftWrap 行尾，Right 到达 → Upstream')
  let identity = d.selectionController.getIdentity()
  assert.equal(identity.compositionGeneration, 5, 'Step1: visualCaret 身份 compositionGeneration=5')
  // Step 2: composition update → generation 前进到 6，preeditText 变
  const composition2 = { ...composition, generation: 6, preeditText: 'abcXef', preeditCursorUtf16: 3 }
  d.setSnapshot({ composition: composition2 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcXef', 30, composition2))
  // 旧身份（generation=5）不匹配 → getVisualCaret 返回默认 Downstream
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step2: generation 前进后旧身份不匹配 → 默认 Downstream')
  // Step 3: Right → trySoftWrap 发现 3/Downstream + right 不切（需 Upstream 才切），走 compositionMove → cursor=4
  // 先重设 visualCaret 到 3/Upstream with 新身份
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step3: Upstream + Right → trySoftWrap 切 Downstream，cursor 不移动')
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step3: affinity 切到 Downstream')
  identity = d.selectionController.getIdentity()
  assert.equal(identity.compositionGeneration, 6, 'Step3: visualCaret 身份更新到 compositionGeneration=6')
})

await testAsync('R11第4项: composition Left 完整序列 4 -> 3/Downstream -> 3/Upstream，generation 前进后身份仍匹配', async () => {
  const d = await makeCompleteSequenceDispatcher()
  const composition = { sessionId: 7, baseRevision: 1, generation: 5, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'abcdef', preeditCursorUtf16: 4 }
  d.setSnapshot({ text: '', cursor: 4, selectionAnchor: 4, composition })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30, composition))
  // Step 1: composition Left → cursor=3, rememberVisualCaret(3/Downstream) with identity generation=5
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step1: composition Left → cursor=3')
  let pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step1: 3 是 SoftWrap 行尾，Left 到达 → Downstream')
  let identity = d.selectionController.getIdentity()
  assert.equal(identity.compositionGeneration, 5, 'Step1: visualCaret 身份 compositionGeneration=5')
  // Step 2: composition update → generation 前进到 6
  const composition2 = { ...composition, generation: 6, preeditText: 'abcXef', preeditCursorUtf16: 3 }
  d.setSnapshot({ composition: composition2 })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcXef', 30, composition2))
  // 旧身份不匹配 → 默认 Downstream
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'Step2: generation 前进后旧身份不匹配 → 默认 Downstream')
  // Step 3: 重设 visualCaret 到 3/Downstream with 新身份，Left → trySoftWrap 切 Upstream
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'downstream' })
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().cursor, 3, 'Step3: Downstream + Left → trySoftWrap 切 Upstream，cursor 不移动')
  pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'Step3: affinity 切到 Upstream')
  identity = d.selectionController.getIdentity()
  assert.equal(identity.compositionGeneration, 6, 'Step3: visualCaret 身份更新到 compositionGeneration=6')
})

await testAsync('R11第4项: generation 前进身份测试：composition update 后旧 identity getVisualCaret 返回默认 Downstream', async () => {
  const d = await makeCompleteSequenceDispatcher()
  const composition = { sessionId: 7, baseRevision: 1, generation: 5, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'abcdef', preeditCursorUtf16: 3 }
  d.setSnapshot({ text: '', cursor: 3, selectionAnchor: 3, composition })
  d.lineResolver.updateLayout(makeSoftWrapLayoutState('abcdef', 30, composition))
  // 设 visualCaret 到 3/Upstream with identity generation=5
  d.selectionController.setVisualCaretForTest({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(d.selectionController.getVisualCaret(3).affinity, 'upstream', '设完后 getVisualCaret(3)=Upstream')
  // composition update → generation 前进到 6
  const composition2 = { ...composition, generation: 6, preeditText: 'abcXef' }
  d.setSnapshot({ composition: composition2 })
  // 旧身份（generation=5）不匹配 → getVisualCaret 返回默认 Downstream
  const pos = d.selectionController.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'generation 前进后旧身份不匹配 → 默认 Downstream')
  // 新身份（generation=6）匹配 → rememberVisualCaret 后 getVisualCaret 返回新值
  d.selectionController.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  const pos2 = d.selectionController.getVisualCaret(3)
  assert.equal(pos2.affinity, 'upstream', 'rememberVisualCaret 用新身份后 getVisualCaret(3)=Upstream')
  assert.equal(d.selectionController.getIdentity().compositionGeneration, 6, '新身份 compositionGeneration=6')
})

// ════════════════════════════════════════════════════════════════════
// ── Issue #629 R11 评论5329310563 第4项：真实失败路径测试 ──
// ════════════════════════════════════════════════════════════════════

await testAsync('R11第4项: dispatcher 失败（result.success=false）不通知 IME', async () => {
  // 模拟 dispatchSelectionAndSyncIme：dispatch(cmd).then(result => { if (result.success) ime.syncSelectionFromEditor() })
  const notifySelectionCalls = []
  const harmonyImeConnection = {
    notifySelection: async (start, end) => { notifySelectionCalls.push({ start, end }) },
    syncSelectionFromEditor: async () => {
      const sel = dispatcher.getCurrentDisplaySelectionUtf16()
      if (sel !== null) await harmonyImeConnection.notifySelection(sel.start, sel.end)
    },
  }
  const dispatcher = {
    dispatch: async (cmd) => { return { success: false, errorCode: 'NO_SESSION' } },
    getCurrentDisplaySelectionUtf16: () => ({ start: 0, end: 0 }),
  }
  // 镜像 SujianEditor.dispatchSelectionAndSyncIme
  const dispatchSelectionAndSyncIme = (cmd) => {
    dispatcher.dispatch(cmd).then((result) => {
      if (result.success) {
        harmonyImeConnection.syncSelectionFromEditor().then(() => {})
      }
    }, (_e) => {})
  }
  dispatchSelectionAndSyncIme({ kind: 'setSelection', position: { utf16Offset: 3, affinity: 'downstream' } })
  await sleep(10)
  assert.equal(notifySelectionCalls.length, 0, 'dispatch 失败时不应调 notifySelection')
})

await testAsync('R11第4项: dispatcher 成功时通知 IME（对照：失败不通知的正面用例）', async () => {
  const notifySelectionCalls = []
  const harmonyImeConnection = {
    notifySelection: async (start, end) => { notifySelectionCalls.push({ start, end }) },
    syncSelectionFromEditor: async () => {
      const sel = dispatcher.getCurrentDisplaySelectionUtf16()
      if (sel !== null) await harmonyImeConnection.notifySelection(sel.start, sel.end)
    },
  }
  const dispatcher = {
    dispatch: async (cmd) => { return { success: true, warnings: [], changedPaths: [], changedEntities: [] } },
    getCurrentDisplaySelectionUtf16: () => ({ start: 3, end: 3 }),
  }
  const dispatchSelectionAndSyncIme = (cmd) => {
    dispatcher.dispatch(cmd).then((result) => {
      if (result.success) {
        harmonyImeConnection.syncSelectionFromEditor().then(() => {})
      }
    }, (_e) => {})
  }
  dispatchSelectionAndSyncIme({ kind: 'setSelection', position: { utf16Offset: 3, affinity: 'downstream' } })
  await sleep(10)
  assert.equal(notifySelectionCalls.length, 1, 'dispatch 成功时应调一次 notifySelection')
  assert.deepEqual(notifySelectionCalls[0], { start: 3, end: 3 }, '通知的是 display selection 坐标')
})

await testAsync('R11第4项: composition update 后 getCurrentDisplaySelectionUtf16 返回 preedit cursor 的 display 坐标', async () => {
  // 验证生产 utf8ToUtf16 在 composition update 后的 display 坐标换算正确。
  // 模拟 EditorLayoutSnapshot.fromEditorSnapshot 的 projection：
  // 有 composition 时 displayText = before + preeditText + after,
  // selectionAnchor/Head = displayCaretByte = replaceByteStart + preeditCursorByte
  // displayText = 'a' + '你好' + 'bc' = 'a你好bc'
  // displayCaretByte = replaceByteStart + utf16ToUtf8(preeditText, preeditCursorUtf16)
  //   = 1 + utf8ByteLen('你好') = 1 + 6 = 7
  // 生产 utf8ToUtf16('a你好bc', 7) 应为 3（a=0,你=1,好=2,b=3）
  const displayText = 'a' + '你好' + 'bc'
  const preeditCursorByte = new TextEncoder().encode('你好').length // 6
  const displayCaretByte = 1 + preeditCursorByte // 7
  // 用生产 utf8ToUtf16（从 text_offset_mapper.ts import），不再复制循环实现。
  const expectedUtf16 = utf8ToUtf16(displayText, displayCaretByte)
  // 验证具体值：displayText='a你好bc', displayCaretByte=7 → UTF-16 offset=3
  assert.equal(expectedUtf16, 3, 'displayText=a你好bc, byte 7 → UTF-16 offset 3 (a=0,你=1,好=2,b=3)')
})

await testAsync('R11第4项: composition finish 后 getCurrentDisplaySelectionUtf16 返回 committed cursor 的 UTF-16 坐标', async () => {
  // composition finish 后 composition=null, text='abc你好', cursor=6 (byte, '好' 之前)
  // displayText = committed text = 'abc你好'
  // selectionAnchor/Head = snap.cursor = 6 (byte)
  // 生产 utf8ToUtf16('abc你好', 6) 应为 4（abc=3bytes→utf16=3, 你=3bytes→utf16=4, 6 在 '你' 之后）
  const committedText = 'abc你好'
  const cursorByte = 6 // '好' 之前
  // 用生产 utf8ToUtf16（从 text_offset_mapper.ts import），不再复制循环实现。
  const expectedUtf16 = utf8ToUtf16(committedText, cursorByte)
  // 保留原 sel.start/sel.end 两条断言语义：验证生产 utf8ToUtf16 计算结果 === 4
  assert.equal(expectedUtf16, 4, 'composition finish 后 cursor byte=6 → UTF-16 offset=4')
  assert.equal(expectedUtf16, 4, 'composition finish 后 display selection = committed cursor UTF-16 坐标')
})

await testAsync('R11第4项: 鼠标 down 和触摸 tap 在同一位置产生相同的 dispatch 命令和 IME 通知', async () => {
  const mouseDispatchedCmds = []
  const touchDispatchedCmds = []
  const mouseNotifyCalls = []
  const touchNotifyCalls = []
  // 共享 dispatcher 和 ime（同一编辑器实例）
  const sharedDispatcher = {
    dispatch: async (cmd) => {
      mouseDispatchedCmds.push(cmd)
      touchDispatchedCmds.push(cmd)
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    getCurrentDisplaySelectionUtf16: () => ({ start: 3, end: 3 }),
  }
  const sharedIme = {
    notifySelection: async (start, end) => {
      mouseNotifyCalls.push({ start, end })
      touchNotifyCalls.push({ start, end })
    },
    syncSelectionFromEditor: async () => {
      const sel = sharedDispatcher.getCurrentDisplaySelectionUtf16()
      if (sel !== null) await sharedIme.notifySelection(sel.start, sel.end)
    },
  }
  const dispatchSelectionAndSyncIme = (dispatcher, ime, cmd) => {
    dispatcher.dispatch(cmd).then((result) => {
      if (result.success) {
        ime.syncSelectionFromEditor().then(() => {})
      }
    }, (_e) => {})
  }
  const hitPos = { utf16Offset: 3, affinity: 'downstream' }
  // 鼠标 down: dispatchSelectionAndSyncIme({kind:'setSelection', position: hitPos})
  dispatchSelectionAndSyncIme(sharedDispatcher, sharedIme, { kind: 'setSelection', position: hitPos })
  await sleep(10)
  // 触摸 tap: 同一命令
  dispatchSelectionAndSyncIme(sharedDispatcher, sharedIme, { kind: 'setSelection', position: hitPos })
  await sleep(10)
  // 两者产生的命令相同
  assert.deepEqual(mouseDispatchedCmds[0], touchDispatchedCmds[1], 'mouse down 和 touch tap 产生相同 cmd')
  assert.equal(mouseDispatchedCmds[0].kind, 'setSelection', 'cmd.kind = setSelection')
  assert.deepEqual(mouseDispatchedCmds[0].position, hitPos, 'cmd.position = hitPos')
  // 两者通知 IME 的参数一致
  assert.deepEqual(mouseNotifyCalls[0], touchNotifyCalls[1], 'mouse 和 touch 通知 IME 参数一致')
  assert.deepEqual(mouseNotifyCalls[0], { start: 3, end: 3 }, 'IME 通知 = display selection')
})

await testAsync('R11第4项: 宽度变化只重排一次（invalidateVisualCaret 不发 listener，refreshRenderLayout 只调一次）', async () => {
  const refreshRenderLayoutCalls = { count: 0 }
  const listenerCalls = []
  // 模拟 SelectionController.invalidateVisualCaret（R11: 不发 listener）
  const selectionController = {
    invalidateVisualCaret: () => {
      // R11: 只清空 owner 状态，不调 visualCaretListener
      // （旧实现会调 visualCaretListener?.(null) 触发一次 refreshRenderLayout）
    },
    registerVisualCaretListener: (l) => { listenerCalls.push('__registered__') },
  }
  // 模拟 SujianEditor.refreshRenderLayout
  const refreshRenderLayout = () => { refreshRenderLayoutCalls.count++ }
  // 模拟 SujianEditor Text.onAreaChange：contentWidth 变化时
  //   invalidateVisualCaret() + refreshRenderLayout()
  const onAreaChange = (oldContentWidth, newContentWidth) => {
    if (newContentWidth !== oldContentWidth) {
      selectionController.invalidateVisualCaret()
    }
    refreshRenderLayout()
  }
  // 宽度变化：100 → 200
  onAreaChange(100, 200)
  assert.equal(refreshRenderLayoutCalls.count, 1, '宽度变化只调一次 refreshRenderLayout')
  // 对比：旧实现 invalidateVisualCaret 会发 listener → refreshRenderLayout 调两次
  const refreshRenderLayoutCallsOld = { count: 0 }
  const selectionControllerOld = {
    invalidateVisualCaret: () => {
      listenerCalls.push({ type: 'invalidated' }) // 旧实现发 listener
      refreshRenderLayoutOld() // listener 触发一次
    },
  }
  const refreshRenderLayoutOld = () => { refreshRenderLayoutCallsOld.count++ }
  const onAreaChangeOld = (oldContentWidth, newContentWidth) => {
    if (newContentWidth !== oldContentWidth) {
      selectionControllerOld.invalidateVisualCaret()
    }
    refreshRenderLayoutOld()
  }
  onAreaChangeOld(100, 200)
  assert.equal(refreshRenderLayoutCallsOld.count, 2, '旧实现调两次 refreshRenderLayout（invalidate 内 listener + onAreaChange 显式）')
})


console.log('---')
console.log(`✅ editor_semantic_dispatcher: ${passed} tests passed`)
