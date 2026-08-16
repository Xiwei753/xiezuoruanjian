// editor_patch_logic.ts — 编辑会话纯逻辑模块。
// 不依赖 ArkUI，只依赖 string/number/Array/Promise。
// 生产由 EditorSessionState.ets / EditorSessionCoordinator.ets 调用；
// 测试由 Node 单测直接 import。
//
// 包含三块纯逻辑：
//   1. UTF-8 byte offset ↔ UTF-16 code unit offset 严格映射（utf8ByteOffsetToUtf16）
//   2. DisplayPatch 严格应用（applyPatchStrict）+ EditorEditResult → snapshot 增量更新（applyEditResultToSnapshot）
//   3. 串行命令队列（SerialCommandQueue）— 保证编辑命令按 enqueue 顺序执行，
//      前一条完成才下一条；每条出队执行时才读当前 state，避免并发命令拿到同一个 expectedRevision。
//
// 与 Core api/types/editor.rs 真实契约对齐：字段名严格 camelCase
// （Core serde rename_all = "camelCase"）。
//
// Issue #629 评论 5 第 3 部分：
//   - applyEditResultToSnapshot 不再用"返回原 snapshot"表达失败，改成明确的 { ok, snapshot, reason } 结果。
//   - 删除旧 deprecated applyPatch（UTF-8 byte offset 直接当 JS UTF-16 下标的错误实现）。

/** DisplayPatch 形状（与 Core DTO 对齐，camelCase）。 */
export interface DisplayPatch {
  readonly baseRevision: number
  readonly newRevision: number
  readonly replaceByteStart: number
  readonly replaceByteEndExclusive: number
  readonly insertedText: string
  readonly resultingSelectionStart: number
  readonly resultingSelectionEnd: number
}

/** CompositionSession 形状。 */
export interface CompositionSession {
  readonly sessionId: number
  readonly baseRevision: number
  readonly generation: number
}

/** EditorContentDelta 形状。 */
export interface EditorContentDelta {
  readonly insertedChars: number
  readonly deletedChars: number
  readonly insertedNonWhitespaceChars: number
  readonly deletedNonWhitespaceChars: number
}

/** EditorEditResult 形状（与 Core DTO 对齐，camelCase）。 */
export interface EditorEditResult {
  readonly outcome: string
  readonly transactionId: number
  readonly baseRevision: number
  readonly newRevision: number
  readonly displayPatches: DisplayPatch[]
  readonly oldSelectionStart: number
  readonly oldSelectionEnd: number
  readonly newSelectionStart: number
  readonly newSelectionEnd: number
  readonly visualIntent: Record<string, unknown>
  readonly compositionSession: CompositionSession | null
  readonly contentDelta: EditorContentDelta
}

/** EditorSessionSnapshot 形状。 */
export interface EditorSessionSnapshot {
  readonly text: string
  readonly revision: number
  readonly cursor: number
  readonly selectionAnchor: number
  readonly generation: number
  readonly chapterId: string
}

/** 编辑命令结果枚举值。与 Rust EditorEditOutcome 变体字符串对齐。 */
export const APPLIED = 'applied'
export const APPLIED_WITH_ADJUSTED_SELECTION = 'appliedWithAdjustedSelection'
export const NO_CHANGE = 'noChange'
export const STALE_REVISION = 'staleRevision'
export const INVALID_OFFSET = 'invalidOffset'
export const INVALID_RANGE = 'invalidRange'

/**
 * applyEditResultToSnapshot 的结果：明确区分成功与失败，不吞失败。
 * - ok=true：snapshot 是应用 patch 后的新状态
 * - ok=false：reason 说明失败原因（staleRevision/invalidOffset/invalidRange/patchFailed:.../unknownOutcome:...）
 *   调用方（EditorSessionState/Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state，
 *   不能吞失败让 UI 停在旧文本。
 */
export type ApplyEditResultOutcome =
  | { readonly ok: true; readonly snapshot: EditorSessionSnapshot }
  | { readonly ok: false; readonly reason: string }

/**
 * UTF-8 byte offset → UTF-16 code unit offset（严格版）。
 * 逐 Unicode code point 累加 UTF-8 byte 长度和 UTF-16 code unit 长度。
 * byteOffset 必须恰好落在字符边界（某个 code point 的开始处）或等于文本总 byte 长度（末尾）。
 * 非字符边界（落在某个 code point 中间）或越界时返回 -1。
 *
 * 与 TextOffsetMapper.utf8ToUtf16 的区别：后者在非边界时静默截断到前一个边界，
 * 本函数严格返回 -1，让调用方明确处理失败（从 Core snapshot() 恢复）。
 */
export function utf8ByteOffsetToUtf16(text: string, byteOffset: number): number {
  if (byteOffset < 0) return -1
  if (byteOffset === 0) return 0
  let byteLen = 0
  let utf16Index = 0
  let i = 0
  while (i < text.length) {
    const code = text.charCodeAt(i)
    let charByteLen: number
    let utf16Step: number
    if (code < 0x80) {
      charByteLen = 1
      utf16Step = 1
    } else if (code < 0x800) {
      charByteLen = 2
      utf16Step = 1
    } else if (code >= 0xD800 && code <= 0xDBFF) {
      // 高代理项：UTF-16 surrogate pair 对应 1 个 code point，UTF-8 4 字节，UTF-16 2 code unit。
      charByteLen = 4
      utf16Step = 2
      i += 1  // 跳过低代理项
    } else {
      charByteLen = 3
      utf16Step = 1
    }
    byteLen += charByteLen
    utf16Index += utf16Step
    i += 1
    if (byteLen === byteOffset) {
      return utf16Index
    }
    if (byteLen > byteOffset) {
      // 落在当前 code point 中间，非字符边界
      return -1
    }
  }
  // 遍历完所有 code point 仍未达到 byteOffset → 越界
  return -1
}

/**
 * 严格应用单个 DisplayPatch：把 text[replaceByteStart, replaceByteEndExclusive)（UTF-8 byte offset）
 * 替换为 insertedText。byte offset 先转成 UTF-16 code unit offset 再 substring。
 *
 * 成功返回 { ok: true, text }；失败（非字符边界或越界）返回 { ok: false, reason }，
 * 让上层从 Core snapshot() 恢复，不静默截断。
 */
export function applyPatchStrict(
  text: string,
  patch: DisplayPatch
): { ok: true, text: string } | { ok: false, reason: string } {
  const startUtf16 = utf8ByteOffsetToUtf16(text, patch.replaceByteStart)
  if (startUtf16 < 0) {
    return {
      ok: false,
      reason: `replaceByteStart=${patch.replaceByteStart} 不是字符边界或越界`,
    }
  }
  const endUtf16 = utf8ByteOffsetToUtf16(text, patch.replaceByteEndExclusive)
  if (endUtf16 < 0) {
    return {
      ok: false,
      reason: `replaceByteEndExclusive=${patch.replaceByteEndExclusive} 不是字符边界或越界`,
    }
  }
  if (endUtf16 < startUtf16) {
    return {
      ok: false,
      reason: `转换后 endUtf16=${endUtf16} < startUtf16=${startUtf16}`,
    }
  }
  const before = text.substring(0, startUtf16)
  const after = text.substring(endUtf16)
  return { ok: true, text: before + patch.insertedText + after }
}

/**
 * 从 EditorEditResult 增量更新 snapshot。
 *
 * 成功返回 { ok: true, snapshot }；失败返回 { ok: false, reason }，reason 明确说明失败原因。
 * 失败情况：
 *   - outcome 为 staleRevision → reason = 'staleRevision'
 *   - outcome 为 invalidOffset → reason = 'invalidOffset'
 *   - outcome 为 invalidRange → reason = 'invalidRange'
 *   - outcome 为未知值 → reason = 'unknownOutcome:<value>'
 *   - patch 应用失败（非字符边界或越界）→ reason = 'patchFailed:<details>'
 *
 * 调用方（EditorSessionState/Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state，
 * 不能吞失败让 UI 停在旧文本。
 *
 * 多个 DisplayPatch 按顺序应用，每个 patch 都针对当时的当前文本转换 offset。
 * compositionSession 非空时取其 generation；为空时保留原 snapshot.generation
 * （finishComposition 后 compositionSession=null，generation 不重置）。
 */
export function applyEditResultToSnapshot(
  snapshot: EditorSessionSnapshot,
  result: EditorEditResult
): ApplyEditResultOutcome {
  if (result.outcome === STALE_REVISION) {
    return { ok: false, reason: STALE_REVISION }
  }
  if (result.outcome === INVALID_OFFSET) {
    return { ok: false, reason: INVALID_OFFSET }
  }
  if (result.outcome === INVALID_RANGE) {
    return { ok: false, reason: INVALID_RANGE }
  }
  if (result.outcome !== APPLIED
    && result.outcome !== APPLIED_WITH_ADJUSTED_SELECTION
    && result.outcome !== NO_CHANGE) {
    return { ok: false, reason: `unknownOutcome:${result.outcome}` }
  }
  let newText: string = snapshot.text
  const patches: DisplayPatch[] = result.displayPatches ?? []
  for (let i = 0; i < patches.length; i++) {
    const applied = applyPatchStrict(newText, patches[i])
    if (!applied.ok) {
      return { ok: false, reason: `patchFailed:${applied.reason}` }
    }
    newText = applied.text
  }
  return {
    ok: true,
    snapshot: {
      text: newText,
      revision: result.newRevision,
      cursor: result.newSelectionEnd,
      selectionAnchor: result.newSelectionStart,
      generation: result.compositionSession ? result.compositionSession.generation : snapshot.generation,
      chapterId: snapshot.chapterId
    }
  }
}

// ─── 串行命令队列 ─────────────────────────────────────────────────────────────
// 编辑命令必须严格按 enqueue 顺序执行：前一条完成（state 更新）后，下一条才能开始。
// 每条命令在出队执行时才读取当前 state（revision），避免多条并发命令拿到同一个 expectedRevision。
// 这把"平台输入事件有序 → Core 命令有序"固定成编辑器边界。
//
// 实现用链式 Promise：tail 是上一条命令完成（无论成功失败）后才 resolve 的 Promise。
// 新 enqueue 的命令 .then 在 tail 上，保证顺序；thunk 自身的 reject 传给调用方，
// 但 tail 永远不 reject（加了两个 handler），所以一条失败不阻塞后续。
//
// 纯逻辑（不依赖 ArkUI），可由 Node --experimental-strip-types 直接 import 单测。

export interface QueueStats {
  readonly pending: number
  readonly running: boolean
}

export class SerialCommandQueue {
  private tail: Promise<void> = Promise.resolve()
  private pendingCount: number = 0
  private activeCount: number = 0

  /**
   * 把一个异步命令 thunk 排入队列。
   * thunk 必须在出队执行时才读取当前 state（调用方在 thunk 闭包内读 this.state.snapshot.revision）。
   * 返回的 Promise 在 thunk 完成时 resolve/reject，与 thunk 的结果一致。
   */
  enqueue<T>(thunk: () => Promise<T>): Promise<T> {
    this.pendingCount++
    // 链式：等前一条完成（无论成功失败）后才执行当前 thunk
    const result: Promise<T> = this.tail.then(() => {
      this.pendingCount--
      this.activeCount++
      return thunk()
    })
    // 更新 tail：当前 thunk 完成后（无论成功失败）才让下一条开始。
    // tail 永远不 reject（两个 handler 都返回 void），避免一条失败阻塞后续。
    this.tail = result.then(
      () => { this.activeCount-- },
      () => { this.activeCount-- }
    )
    return result
  }

  /** 当前排队中（未开始）的命令数。 */
  size(): number {
    return this.pendingCount
  }

  /** 是否有命令正在执行或排队。 */
  isIdle(): boolean {
    return this.pendingCount === 0 && this.activeCount === 0
  }

  /** 队列状态快照（用于测试断言）。 */
  stats(): QueueStats {
    return { pending: this.pendingCount, running: this.activeCount > 0 }
  }
}
