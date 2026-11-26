// editor_merge_plan.ts — 把已冻结的 pendingEdits 逐笔重基到「已前进的 Core 正文」
// （Issue #1063 复审评论第 2、3 条）。
//
// 为什么要单开这一层：旧 reconcileSession 用 computeTextDelta(base, native) 一次算整段差量。
// 「文本相等」看起来永远成立，但它有两处硬伤：
//   1. 从不核对 pendingEdits 的 beforeText / oldText / 队列顺序 —— 文本相等被当成「编辑事务
//      已经确认」，用户输入/删除的事实与原因因此可能没有进入原有统计与证明路径；
//   2. 多笔离散编辑被公共前后缀压成一段 —— 两侧实际互不重叠也会被误判冲突；合并成功时也
//      只剩一整段事务，逐笔映射、逐笔回执都无从谈起；UTF-16 diff 还允许切到代理对内部。
// 本模块只做纯逻辑：先把整条原生链按队列顺序重放到 nativeText（逐笔校验 beforeText /
// oldText / 代理对边界），再在「当前 Core 正文」上按同样的队列顺序逐笔映射，并同时推进
// 两份模拟正文。任何一步无法无歧义映射（区间重叠、oldText 对不上、越界、切代理对）都返回
// ok=false，不猜测、不用文本相等兜底 —— 调用方据此保留全部编辑事实与原生正文，走明确的
// 失败/冲突路径（复用已有 Core 冲突材料机制，本文件不新增持久化流程）。
//
// 生产接线在 NativeRichEditorBridge.ets 的 reconcileSession（由桥接侧负责）；本文件不依赖
// ArkUI / NAPI，Node 单测可直接 import。
//
// 运行测试：node --experimental-strip-types feature/editor/session/__tests__/editor_merge_plan.test.mjs
//
// 注：这里显式写 `.ts` 后缀，是因为 Node 的 ESM 解析不做后缀补全；同目录既有
// stats_drain_loop.ts → editor_stats_barrier.ts 也是同样写法，ArkTS 侧可正常编译。

import { applyTextDelta, computeTextDelta, TextDelta } from './editor_reconcile_logic.ts'

// 一笔已经冻结的用户原生编辑事实（输入已按会话过滤，顺序即队列顺序）。
//
// beforeText 是该笔编辑发生前编辑器里的完整正文，oldText 是被替换区间原本的内容。只有三者
// （beforeText、坐标、oldText）同时对上，才能证明「这笔编辑确实发生在这条链的这个位置」，
// 而不是「前后文本恰好相等」。冻结队列之后原生正文仍可能变化，所以这份事实是映射的唯一依据。
export interface PendingEditFact {
  queueSequence: number
  beforeText: string
  startUtf16: number
  endUtf16: number
  insertedText: string
  oldText: string
}

// 映射到当前 Core 正文坐标后的一笔编辑。
//
// 坐标是「按队列顺序逐笔提交到 Core」时的坐标：每提交一笔，Core 正文就变长/变短，所以这些
// 位置不能一次性并行套用到同一个正文快照上，调用方必须按数组顺序逐笔提交。
export interface MappedPendingEdit {
  queueSequence: number
  startUtf16: number
  endUtf16: number
  insertedText: string
  oldText: string
}

export interface PendingEditRebasePlan {
  ok: boolean
  reason: string
  // 按队列顺序、在「当前 Core 正文」上可直接提交的事务位置与内容。
  edits: MappedPendingEdit[]
  // 所有 edits 顺序提交完成后 Core 应等于的正文。
  resultText: string
  // Core 相对共同基线的外部改动（base→core）；无外部改动时 start===end 且 replacement===''。
  coreDelta: TextDelta
}

// ── UTF-16 边界 ──

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff
}

/**
 * 位置是否落在 UTF-16 代理对边界上。
 *
 * 落点「是低代理且前一个 code unit 是高代理」时说明它切在 😀 这类字符中间：后续 slice /
 * substring 会得到半个码点，写进 Core 就是非法文本。空串两端与字符串首尾天然是边界。
 */
function isUtf16Boundary(text: string, index: number): boolean {
  if (index <= 0 || index >= text.length) {
    return true
  }
  return !(isLowSurrogate(text.charCodeAt(index)) && isHighSurrogate(text.charCodeAt(index - 1)))
}

/**
 * 在一份模拟正文上校验一笔待重放的编辑事实。
 *
 * 返回 '' 表示通过，否则是具体失败原因。校验对象是「当前模拟正文」而不是调用方传入的
 * baseText：前一笔编辑的插入/删除会改变后一笔的 beforeText 与坐标，只有逐笔推进的模拟正文
 * 才能证明整条链连续。
 */
function pendingEditRejectReason(text: string, edit: PendingEditFact): string {
  if (edit.beforeText !== text) {
    return `pending edit #${edit.queueSequence} beforeText mismatch`
      + ` (simLen=${text.length}, beforeLen=${edit.beforeText.length})`
  }
  if (edit.startUtf16 < 0 || edit.endUtf16 < edit.startUtf16 || edit.endUtf16 > text.length) {
    return `pending edit #${edit.queueSequence} range [${edit.startUtf16}, ${edit.endUtf16})`
      + ` out of bounds (simLen=${text.length})`
  }
  if (!isUtf16Boundary(text, edit.startUtf16) || !isUtf16Boundary(text, edit.endUtf16)) {
    return `pending edit #${edit.queueSequence} range [${edit.startUtf16}, ${edit.endUtf16})`
      + ' splits a UTF-16 surrogate pair'
  }
  if (text.substring(edit.startUtf16, edit.endUtf16) !== edit.oldText) {
    return `pending edit #${edit.queueSequence} oldText mismatch at`
      + ` [${edit.startUtf16}, ${edit.endUtf16})`
  }
  return ''
}

function failedPlan(reason: string, coreText: string, coreDelta: TextDelta): PendingEditRebasePlan {
  return { ok: false, reason: reason, edits: [], resultText: coreText, coreDelta: coreDelta }
}

/**
 * 把 pendingEdits 逐笔重基到当前 Core 正文。
 *
 * 只在能证明「每笔编辑都来自这条原生链，且能被无歧义地放到当前 Core 坐标」时返回 ok=true：
 *   - 重放链：baseText + edits 按队列顺序必须逐笔接得上，最后恰好等于 nativeText；
 *   - Core 外部改动：base→core 的差量必须能复现 coreText；
 *   - 逐笔映射：edit 区间与 Core 外部改动区间不得重叠，映射后 oldText 必须仍对得上。
 * 失败时 resultText 保持 coreText，调用方不得据此提交；edits 为空表示没有任何可安全提交的事务。
 */
export function planPendingEditRebase(
  baseText: string,
  coreText: string,
  nativeText: string,
  edits: Array<PendingEditFact>
): PendingEditRebasePlan {
  // coreDelta 是返回结构的一部分（失败也要带回），先算出来。它只取决于 base/core，
  // 与下面校验的先后顺序无关。
  const coreDelta: TextDelta = computeTextDelta(baseText, coreText)

  // 没有原始编辑事实却有原生差量：没有 beforeText / oldText 可核对，无法证明这些改动是什么，
  // 只能交给调用方按明确的恢复来源处理。这一步必须先于整链校验，否则会被「模拟结果不得等于
  // nativeText」的通用失败原因吞掉。
  if (edits.length === 0 && nativeText !== baseText) {
    return failedPlan('unmirrored native delta', coreText, coreDelta)
  }

  // 1) 顺序校验整条原生链。
  // 为什么必须按 pendingEdits 的队列顺序：beforeText 只对「上一笔编辑作用后的正文」成立，
  // 顺序一换，坐标与 oldText 就全部失效；跳过某一笔也会让后续 beforeText 接不上。整链
  // 通过并恰好等于 nativeText，才能说明这份事实完整覆盖了原生相对共同基线的全部改动。
  let sim: string = baseText
  for (let i = 0; i < edits.length; i += 1) {
    const edit: PendingEditFact = edits[i]
    const reject: string = pendingEditRejectReason(sim, edit)
    if (reject.length > 0) {
      return failedPlan(reject, coreText, coreDelta)
    }
    sim = sim.slice(0, edit.startUtf16) + edit.insertedText + sim.slice(edit.endUtf16)
  }
  if (sim !== nativeText) {
    // 链本身接得上，但只覆盖了 nativeText 的一部分：剩下的差量没有编辑事实，不能按存活部分提交。
    return failedPlan(
      `native text does not match replayed pending edits (simLen=${sim.length}, nativeLen=${nativeText.length})`,
      coreText,
      coreDelta
    )
  }

  // 2) Core 外部改动必须是可复现的：它是逐笔映射的唯一依据，复现失败说明输入或 diff 有问题。
  if (applyTextDelta(baseText, coreDelta) !== coreText) {
    return failedPlan('core delta does not reproduce current core text', coreText, coreDelta)
  }

  // 3) 逐笔重放并映射到当前 Core 坐标。
  //
  // 为什么同时维护 sim 与 coreSim：sim 固定从 baseText 出发，重放的是「用户实际做了什么」；
  // coreSim 从当前 coreText 出发，重放的是「把同样的编辑提交到 Core 之后应是什么」。每笔
  // 编辑都同时应用到两份正文，所以二者之间永远只差 Core 的外部改动 —— 下一笔就能重新算出
  // 一个小 delta，而不会像单段 base→core 差量那样把多笔编辑压成一大段。coreSim 的最终值
  // 就是 resultText，且每笔 mapped 位置都是「提交到当时的 Core」的坐标。
  let coreSim: string = coreText
  // sim 从第 1 步重放完的 nativeText 复位回共同基线：这一步要按队列顺序重新逐笔映射到 Core。
  sim = baseText
  const mappedEdits: MappedPendingEdit[] = []

  for (let i = 0; i < edits.length; i += 1) {
    const edit: PendingEditFact = edits[i]
    const reject: string = pendingEditRejectReason(sim, edit)
    if (reject.length > 0) {
      return failedPlan(reject, coreText, coreDelta)
    }
    const spanStart: number = edit.startUtf16
    const spanEnd: number = edit.endUtf16
    let mappedStart: number = spanStart
    let mappedEnd: number = spanEnd

    if (sim !== coreSim) {
      const externalDelta: TextDelta = computeTextDelta(sim, coreSim)
      if (applyTextDelta(sim, externalDelta) !== coreSim) {
        return failedPlan(
          `core delta does not reproduce core text at pending edit #${edit.queueSequence}`,
          coreText,
          coreDelta
        )
      }
      const externalStart: number = externalDelta.start
      const externalEnd: number = externalDelta.end
      const externalShift: number = externalDelta.replacement.length
        - (externalEnd - externalStart)
      // 防御性判定：只有真的存在外部改动区间或替换文本时才做重叠/平移。空区间且无替换时
      // 位置原样可用（这种 delta 在 sim !== coreSim 时按契约不可能出现，但保留判断以免把
      // 空 delta 误当成需要映射的外部改动）。
      const hasExternalChange: boolean =
        externalEnd > externalStart || externalDelta.replacement.length > 0
      if (hasExternalChange) {
        // 为什么重叠必须失败：两边都改了同一段文本，谁先谁后没有任何事实可以决定。
        // 继续往下猜要么丢掉用户编辑，要么把 Core 的改动覆盖掉；这里明确交给调用方。
        // 空区间边界（s===de 或 e===ds）不算重叠：插入点贴在改动区间的端点时可以确定先后。
        if (spanStart < externalEnd && spanEnd > externalStart) {
          return failedPlan(
            `pending edit #${edit.queueSequence} range [${spanStart}, ${spanEnd})`
              + ` overlaps core delta [${externalStart}, ${externalEnd})`,
            coreText,
            coreDelta
          )
        }
        if (spanEnd <= externalStart) {
          // 编辑整段在 Core 改动之前：坐标不变。
          mappedStart = spanStart
          mappedEnd = spanEnd
        } else {
          // 只在 Core 改动之后（spanStart >= externalEnd）：按外部改动的净长度平移。
          mappedStart = spanStart + externalShift
          mappedEnd = spanEnd + externalShift
        }
      }
    }

    if (mappedStart < 0 || mappedEnd < mappedStart || mappedEnd > coreSim.length) {
      return failedPlan(
        `mapped pending edit #${edit.queueSequence} range [${mappedStart}, ${mappedEnd})`
          + ` out of bounds (coreLen=${coreSim.length})`,
        coreText,
        coreDelta
      )
    }
    if (!isUtf16Boundary(coreSim, mappedStart) || !isUtf16Boundary(coreSim, mappedEnd)) {
      return failedPlan(
        `mapped pending edit #${edit.queueSequence} range [${mappedStart}, ${mappedEnd})`
          + ' splits a UTF-16 surrogate pair',
        coreText,
        coreDelta
      )
    }
    // 映射后 oldText 仍必须逐字对得上：对不上说明坐标平移把区间落到了别的内容上，
    // 此时提交就会用 insertedText 覆盖一段不是它该覆盖的正文 —— 只能失败。
    if (coreSim.substring(mappedStart, mappedEnd) !== edit.oldText) {
      return failedPlan(
        `mapped pending edit #${edit.queueSequence} oldText mismatch at`
          + ` [${mappedStart}, ${mappedEnd})`,
        coreText,
        coreDelta
      )
    }

    mappedEdits.push({
      queueSequence: edit.queueSequence,
      startUtf16: mappedStart,
      endUtf16: mappedEnd,
      insertedText: edit.insertedText,
      oldText: edit.oldText
    })

    // 同一笔编辑同时推进两份正文：sim 保持在原生链上，coreSim 保持在「提交后」的 Core 上。
    sim = sim.slice(0, spanStart) + edit.insertedText + sim.slice(spanEnd)
    coreSim = coreSim.slice(0, mappedStart) + edit.insertedText + coreSim.slice(mappedEnd)
  }

  return { ok: true, reason: '', edits: mappedEdits, resultText: coreSim, coreDelta: coreDelta }
}
