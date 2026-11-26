// editor_patch_logic.ts — 编辑会话纯逻辑模块。
// 不依赖 ArkUI，只依赖 string/number/Array/Promise。
// 生产由 EditorSessionState.ets / EditorSessionCoordinator.ets 调用；
// 测试由 Node 单测直接 import。
//
// 包含三块纯逻辑：
//   1. UTF-8 byte offset ↔ UTF-16 code unit offset 严格映射（委托 input/text_offset_mapper.ts 的唯一实现）
//   2. patch 批次校验（validatePatchBatch）+ 批量原子应用（applyEditResultToSnapshot）
//      + 单 patch 严格应用（applyPatchStrict，保留给其它调用方）
//   3. 串行命令队列（SerialCommandQueue）— 保证编辑命令按 enqueue 顺序执行，
//      前一条完成才下一条；每条出队执行时才读当前 state，避免并发命令拿到同一个 expectedRevision。
//
// 与 Core api/types/editor.rs 真实契约对齐：字段名严格 camelCase
// （Core serde rename_all = "camelCase"）。
//
// Issue #629 评论 5 第 3 部分：
//   - applyEditResultToSnapshot 不再用"返回原 snapshot"表达失败，改成明确的 { ok, snapshot, reason } 结果。
//   - 删除旧 deprecated applyPatch（UTF-8 byte offset 直接当 JS UTF-16 下标的错误实现）。
//
// Issue #1063 评论第 2 条（displayPatches 是 base 坐标的原子批次）：
//   - 一个 EditorEditResult 的 displayPatches 全部以同一 baseRevision 的 **base 正文
//     UTF-8 byte 坐标** 表示，数组按 replaceByteStart 严格降序且互不重叠。
//   - 平台必须先把整批映射到 base 快照的 UTF-16 局部坐标并整体校验（revision 一致、
//     越界、UTF-8 字符边界、严格降序、不重叠），全部通过后才从右往左重建正文，
//     最后一次性替换 snapshot；任何一项失败都不产生部分修改。
//   - 严禁把 byte offset 直接当 UTF-16 code unit，也严禁在逐项执行时重新按“当前文本”
//     计算 byte→UTF-16 偏移（patch 都是 base 坐标，重算会静默移位）。

import {
  utf8ByteLength,
  utf8ByteOffsetToUtf16 as utf8ByteOffsetToUtf16Strict
} from '../input/text_offset_mapper.ts'

/**
 * Issue #826 评论 10 阻塞 4.4：这里曾声明第二份手写 `DisplayPatch`，
 * 注释还写「与 Core DTO 对齐」，但 Core wire schema 演进后它已经不对齐，
 * 会误导下一轮维护者以为两份表同步。
 *
 * 本文件是纯 TS（Node 单测环境不能直接 import `.ets` DTO），所以这里只声明
 * **patch 应用器真正需要的最小字段视图**，不再假装复制整份 wire DTO：
 * 应用只需要 old 坐标的替换区间 + 插入文本 + 结果选区，
 * 不需要 `insertedByteStart` / `insertedByteEndExclusive`
 * （那是给平台端定位 patch 在 final new 坐标里的位置用的）。
 */
export interface PatchApplyView {
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

/** #629 评论6 Part B：EditorCompositionState 形状（与 Core DTO 对齐，camelCase）。 */
export interface EditorCompositionState {
  readonly sessionId: number
  readonly baseRevision: number
  readonly generation: number
  readonly replaceByteStart: number
  readonly replaceByteEndExclusive: number
  readonly preeditText: string
  readonly preeditCursorUtf16: number
}

/** EditorContentDelta 形状。 */
export interface EditorContentDelta {
  readonly insertedChars: number
  readonly deletedChars: number
  readonly insertedNonWhitespaceChars: number
  readonly deletedNonWhitespaceChars: number
}

/** EditorEditResult 形状（与 Core DTO 对齐，camelCase）。
 * Issue #735：visualIntent 已从 Core 删除，此处同步移除，与 EditorDtos.ets 对齐。 */
export interface EditorEditResult {
  readonly outcome: string
  readonly transactionId: number
  readonly baseRevision: number
  readonly newRevision: number
  readonly displayPatches: PatchApplyView[]
  readonly oldSelectionAnchor: number
  readonly oldSelectionHead: number
  readonly newSelectionAnchor: number
  readonly newSelectionHead: number
  readonly compositionSession: CompositionSession | null
  readonly contentDelta: EditorContentDelta
  // #629 评论6 Part B：当前 composition 完整状态。composition 活跃时非 null。
  readonly composition: EditorCompositionState | null
}

/** EditorSessionSnapshot 形状。 */
export interface EditorSessionSnapshot {
  readonly text: string
  readonly revision: number
  readonly cursor: number
  readonly selectionAnchor: number
  readonly generation: number
  readonly chapterId: string
  // #629 评论6 Part B：composition 活跃时非 null，平台端据此构造临时显示文本和下划线。
  readonly composition: EditorCompositionState | null
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
  | { readonly ok: true; readonly snapshot: Object }
  | { readonly ok: false; readonly reason: string }

/**
 * UTF-8 byte offset → UTF-16 code unit offset（严格版）。
 * byteOffset 必须恰好落在字符边界（某个编码单元的起始处）或等于文本总 byte 长度（末尾）；
 * 非字符边界（落在某个编码单元中间）、非整数或越界时返回 -1。
 *
 * Issue #1063：换算实现只有一份，位于 input/text_offset_mapper.ts（孤立代理项按 WTF-8
 * 3 byte 的统一协议），这里只保留同名导出给既有调用方，不再保留第二套算法。
 */
export function utf8ByteOffsetToUtf16(text: string, byteOffset: number): number {
  return utf8ByteOffsetToUtf16Strict(text, byteOffset)
}

/**
 * patch 批次校验结果：ok=true 表示整批可原子应用；ok=false 的 reason 说明首个不合法项。
 */
export type PatchBatchValidation =
  | { readonly ok: true }
  | { readonly ok: false; readonly reason: string }

/**
 * Issue #1063 评论第 2 条：把 displayPatches 当作同一 base 正文坐标的原子批次校验。
 *
 * 校验项（全部针对**原始 snapshot** 的 base 正文，不允许边执行边按新文本重算）：
 *   1. 每项 patch.baseRevision === result.baseRevision（同一批次，不混 revision）；
 *   2. replaceByteStart >= 0，replaceByteStart <= replaceByteEndExclusive；
 *   3. 两个 byte 坐标都 <= snapshotText 的 UTF-8 byte 总长度；
 *   4. 两个 byte 坐标都落在 UTF-8 字符边界上（严格版映射不为 -1）；
 *   5. 数组按 replaceByteStart 严格降序（前一项 start > 后一项 start）；
 *   6. 互不重叠（后一项 end <= 前一项 start）。
 *
 * 返回 { ok: true } 或 { ok: false, reason }；本函数不修改任何输入。
 */
export function validatePatchBatch(snapshotText: string, result: EditorEditResult): PatchBatchValidation {
  const patches: PatchApplyView[] = result.displayPatches ?? []
  if (patches.length === 0) {
    return { ok: true }
  }
  const textByteLength: number = utf8ByteLength(snapshotText)
  let previousStart: number = -1
  for (let i = 0; i < patches.length; i++) {
    const patch: PatchApplyView = patches[i]
    const start: number = patch.replaceByteStart
    const end: number = patch.replaceByteEndExclusive
    if (patch.baseRevision !== result.baseRevision) {
      return {
        ok: false,
        reason: `patch[${i}] baseRevision=${patch.baseRevision} != batch baseRevision=${result.baseRevision}`
      }
    }
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start) {
      return { ok: false, reason: `patch[${i}] invalid range start=${start} end=${end}` }
    }
    if (start > textByteLength || end > textByteLength) {
      return {
        ok: false,
        reason: `patch[${i}] range [${start}, ${end}) exceeds text byte length ${textByteLength}`
      }
    }
    if (utf8ByteOffsetToUtf16(snapshotText, start) < 0) {
      return { ok: false, reason: `patch[${i}] replaceByteStart=${start} is not a UTF-8 character boundary` }
    }
    if (utf8ByteOffsetToUtf16(snapshotText, end) < 0) {
      return {
        ok: false,
        reason: `patch[${i}] replaceByteEndExclusive=${end} is not a UTF-8 character boundary`
      }
    }
    if (i > 0) {
      // 严格降序：前一项 start 必须严格大于后一项；同起点的变化已由 Core 合并成一项。
      if (start >= previousStart) {
        return {
          ok: false,
          reason: `patch[${i}] start=${start} is not strictly below previous start=${previousStart}`
        }
      }
      // 不重叠：后一项 end 必须 <= 前一项 start。
      if (end > previousStart) {
        return {
          ok: false,
          reason: `patch[${i}] range [${start}, ${end}) overlaps previous patch starting at ${previousStart}`
        }
      }
    }
    previousStart = start
  }
  return { ok: true }
}

/**
 * 严格应用单个 patch：把 text[replaceByteStart, replaceByteEndExclusive)（UTF-8 byte offset）
 * 替换为 insertedText。byte offset 先转成 UTF-16 code unit offset 再 substring。
 *
 * 成功返回 { ok: true, text }；失败（非字符边界或越界）返回 { ok: false, reason }，
 * 让上层从 Core snapshot() 恢复，不静默截断。
 *
 * Issue #1063：批量应用走 applyEditResultToSnapshot 的批次原子路径；本函数保留导出
 * 供其它只需单 patch 的调用方使用，不参与批量 snapshot 更新。
 */
export function applyPatchStrict(
  text: string,
  patch: PatchApplyView
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
 * 最小文本差量：把 oldText 变成 newText 时，oldText[start, end) 替换为 replacement。
 *
 * Issue #1053 评论 7098652618：原生正文与 Core 已提交正文收敛时，只提交实际改变区间。
 * 公共前后缀按完整 Unicode code point 比较，避免把 surrogate pair 拆成半个字符；
 * start/end 是 UTF-16 code unit 下标，可直接用于 substring / TextOffsetMapper。
 */
export interface MinimalTextDiff {
  readonly start: number
  readonly end: number
  readonly replacement: string
}

/**
 * 计算最小文本差量（按完整 Unicode code point 求公共前后缀）。
 * oldText === newText 时返回 { start: end: oldText.length, replacement: '' }：
 * 是空区间替换（正文不变），调用方用 start === end && replacement.length === 0 判定无变化。
 */
export function computeMinimalTextDiff(oldText: string, newText: string): MinimalTextDiff {
  let prefixLen = 0
  while (prefixLen < oldText.length && prefixLen < newText.length) {
    const oldCp = oldText.codePointAt(prefixLen)
    const newCp = newText.codePointAt(prefixLen)
    if (oldCp === undefined || newCp === undefined || oldCp !== newCp) {
      break
    }
    prefixLen += (oldCp > 0xFFFF) ? 2 : 1
  }

  let oldSuffixStart = oldText.length
  let newSuffixStart = newText.length
  while (oldSuffixStart > prefixLen && newSuffixStart > prefixLen) {
    let oldCpLen = 1
    if (oldSuffixStart >= 2) {
      const lastUnit = oldText.charCodeAt(oldSuffixStart - 1)
      const prevUnit = oldText.charCodeAt(oldSuffixStart - 2)
      if (lastUnit >= 0xDC00 && lastUnit <= 0xDFFF && prevUnit >= 0xD800 && prevUnit <= 0xDBFF) {
        oldCpLen = 2
      }
    }
    let newCpLen = 1
    if (newSuffixStart >= 2) {
      const lastUnit = newText.charCodeAt(newSuffixStart - 1)
      const prevUnit = newText.charCodeAt(newSuffixStart - 2)
      if (lastUnit >= 0xDC00 && lastUnit <= 0xDFFF && prevUnit >= 0xD800 && prevUnit <= 0xDBFF) {
        newCpLen = 2
      }
    }

    const oldCp = oldText.codePointAt(oldSuffixStart - oldCpLen)
    const newCp = newText.codePointAt(newSuffixStart - newCpLen)
    if (oldCp !== newCp) {
      break
    }
    oldSuffixStart -= oldCpLen
    newSuffixStart -= newCpLen
  }

  return {
    start: prefixLen,
    end: oldSuffixStart,
    replacement: newText.substring(prefixLen, newSuffixStart),
  }
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
 *   - patch 批次校验失败（revision 不一致/越界/非字符边界/升序/重叠）→ reason = 'patchFailed:<details>'
 *   - 单个 patch 的 base 坐标映射失败 → reason = 'patchFailed:<details>'
 *
 * 调用方（EditorSessionState/Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state，
 * 不能吞失败让 UI 停在旧文本。
 *
 * Issue #1063 评论第 2 条：displayPatches 是同一 baseRevision 的原子批次，
 * 全部坐标都相对**原始 snapshot.text**（不是逐项执行后的中间文本）。因此：
 *   1. 先对 snapshot.text 一次校验完整批次（validatePatchBatch）；
 *   2. 再把每个 patch 的两个 base byte offset 用严格映射各转一次 UTF-16 局部偏移；
 *   3. 按 Core 已规范的严格降序，从右往左用 base UTF-16 区间 + insertedText 拼接新正文；
 *   4. 全部成功后才一次性替换 snapshot.text，失败不产生任何部分修改。
 *
 * compositionSession 非空时取其 generation；为空时保留原 snapshot.generation
 * （finishComposition 后 compositionSession=null，generation 不重置）。
 */
export function applyEditResultToSnapshot(
  snapshot: EditorSessionSnapshot,
  result: Object
): ApplyEditResultOutcome {
  const r: EditorEditResult = result as EditorEditResult
  if (r.outcome === STALE_REVISION) {
    return { ok: false, reason: STALE_REVISION }
  }
  if (r.outcome === INVALID_OFFSET) {
    return { ok: false, reason: INVALID_OFFSET }
  }
  if (r.outcome === INVALID_RANGE) {
    return { ok: false, reason: INVALID_RANGE }
  }
  if (r.outcome !== APPLIED
    && r.outcome !== APPLIED_WITH_ADJUSTED_SELECTION
    && r.outcome !== NO_CHANGE) {
    return { ok: false, reason: `unknownOutcome:${r.outcome}` }
  }

  const patches: PatchApplyView[] = r.displayPatches ?? []
  // 1. 整批校验：任何一项不合法都直接失败，绝不逐项“边试边改”。
  const batchValidation: PatchBatchValidation = validatePatchBatch(snapshot.text, r)
  if (batchValidation.ok === false) {
    return { ok: false, reason: `patchFailed:${batchValidation.reason}` }
  }

  // 2. base byte 坐标 → base UTF-16 局部偏移：每个 offset 只映射一次。
  const baseText: string = snapshot.text
  const patchStarts: number[] = []
  const patchEnds: number[] = []
  for (let i = 0; i < patches.length; i++) {
    const startUtf16: number = utf8ByteOffsetToUtf16(baseText, patches[i].replaceByteStart)
    if (startUtf16 < 0) {
      return {
        ok: false,
        reason: `patchFailed:patch[${i}] replaceByteStart=${patches[i].replaceByteStart}`
          + ` is not a UTF-8 character boundary`
      }
    }
    const endUtf16: number = utf8ByteOffsetToUtf16(baseText, patches[i].replaceByteEndExclusive)
    if (endUtf16 < 0) {
      return {
        ok: false,
        reason: `patchFailed:patch[${i}] replaceByteEndExclusive=${patches[i].replaceByteEndExclusive}`
          + ` is not a UTF-8 character boundary`
      }
    }
    if (endUtf16 < startUtf16) {
      return {
        ok: false,
        reason: `patchFailed:patch[${i}] endUtf16=${endUtf16} < startUtf16=${startUtf16}`
      }
    }
    patchStarts.push(startUtf16)
    patchEnds.push(endUtf16)
  }

  // 3. 严格降序 → 从右往左拼接：每段只取 base 文本的区间，坐标不随执行漂移。
  // newText 始终表示“已处理（更靠右）的区间 + 它们右侧未改动的 base 文本”，
  // 因此每一步是 insertedText + base[end, cursor) + 已处理结果。
  let newText: string = ''
  let cursorUtf16: number = baseText.length
  for (let i = 0; i < patches.length; i++) {
    const startUtf16: number = patchStarts[i]
    const endUtf16: number = patchEnds[i]
    if (startUtf16 > cursorUtf16 || endUtf16 > cursorUtf16) {
      return { ok: false, reason: `patchFailed:patch[${i}] start=${startUtf16} overlaps previous segment` }
    }
    newText = patches[i].insertedText + baseText.substring(endUtf16, cursorUtf16) + newText
    cursorUtf16 = startUtf16
  }
  newText = baseText.substring(0, cursorUtf16) + newText

  // 4. 全部通过后一次性替换 snapshot。
  return {
    ok: true,
    snapshot: {
      text: newText,
      revision: r.newRevision,
      cursor: r.newSelectionHead,
      selectionAnchor: r.newSelectionAnchor,
      generation: r.compositionSession ? r.compositionSession.generation : snapshot.generation,
      chapterId: snapshot.chapterId,
      // #629 评论6 Part B：把 Core 返回的 composition 完整状态透传到新 snapshot，
      // EditorLayoutSnapshot.fromEditorSnapshot 据此构造临时显示文本和下划线。
      // finish/cancel/普通编辑后 r.composition 为 null，snapshot.composition 也为 null。
      composition: r.composition
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

  /**
   * 等待队列空闲：所有已 enqueue 的命令都完成（无论成功失败）后才 resolve。
   *
   * Issue #629 评论6 Part A：统一串行边界需要"flush 语义"——
   * 页面退出/退后台/切章节前要等所有排队编辑命令完成，避免丢命令或读到半改 state。
   *
   * 实现：await 当前 tail（tail 是上一条命令完成后才 resolve 的 Promise，永远不 reject）；
   * await 期间若有新命令排入，递归再等。直到 pendingCount=0 且 activeCount=0 才返回。
   *
   * 不返回 thunk 的结果（调用方不关心每条命令成功失败，只关心"全部完成"）。
   * 永远不 reject（tail 已加两个 handler 吞掉 reject）。
   */
  async whenIdle(): Promise<void> {
    // 等当前 tail 完成（所有已 enqueue 命令执行完毕）。
    await this.tail
    // await 期间若有新命令排入，递归等待。
    if (!this.isIdle()) {
      await this.whenIdle()
    }
  }

  /**
   * drain：与 whenIdle 同义，语义更明确（"排空队列"）。
   * 供调用方按场景命名：页面退出 drain、状态查询 whenIdle。
   */
  async drain(): Promise<void> {
    return this.whenIdle()
  }
}
