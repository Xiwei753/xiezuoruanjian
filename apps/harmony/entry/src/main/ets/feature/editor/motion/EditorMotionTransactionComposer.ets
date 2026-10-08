// EditorMotionTransactionComposer — 事务链复合器。
//
// Issue #879 复核评论修复（问题1+2）：
// 连续快速输入时，多笔 editResult 形成事务链。每笔 patch 的 UTF-8 坐标系不同——
// 第二笔 replaceByteStart 是相对于第一笔修改后的正文，不是最初显示的正文。
//
// 本模块逐笔维护从初版到当前版的字符身份/片段表（Segment 表），逐笔在当前版的
// UTF-8 边界执行 replace，再为幸存片段记录来自初版的位置或新增身份。最终从
// 初始版→最终版提取统一 diff、保留簇映射。
//
// 核心算法：
// 1. 逐笔处理：对每笔 editResult，把它的 displayPatches 在当前 Segment 表上执行
//    replace 操作——切割/移除被替换的片段，插入新增片段（记录 insertedText）。
// 2. 幸存片段（origin=original）保留初版 byte 范围信息；新增片段（origin=inserted）
//    记录插入的文本内容。
// 3. 最终从 Segment 表生成统一的 displayPatches（replace 范围用最初显示正文的
//    UTF-8 坐标，inserted 范围用最终正文的 UTF-8 坐标）和 offsetMap（old→final）。
//
// 不修改 Core 的编辑事实模型，只读 editResult 做复合计算。

import { EditorEditResult, DisplayPatch, OffsetMap, OffsetMapEntry, OffsetMapKind } from '../../../corebridge/dto/EditorDtos'

/**
 * 复合后的事务——包含统一的 displayPatches 和 offsetMap。
 *
 * - displayPatches：所有 replace 范围用最初显示正文的 UTF-8 坐标，
 *   所有 inserted 范围用最终正文的 UTF-8 坐标。
 * - offsetMap：old→final 的字符身份映射。
 * - isValid：链是否连续、首尾 revision 是否匹配、文本是否一致。
 */
export interface ComposedTransaction {
  /** 统一的 displayPatches——replace 范围用最初显示正文坐标，inserted 范围用最终正文坐标 */
  displayPatches: DisplayPatch[]
  /** 统一的 offsetMap——old→final 的字符身份映射 */
  offsetMap: OffsetMap | undefined
  /** 校验结果——链是否连续、首尾 revision 是否匹配、文本是否一致 */
  isValid: boolean
  /** 校验失败原因 */
  validationError: string | null
}

/**
 * 字符片段——记录当前版文本中一段连续字符的来源信息。
 *
 * - origin=0 (original)：来自初版的幸存字符，originalByteStart/End 记录在初版中的位置
 * - origin=1 (inserted)：某笔事务插入的新字符，text 记录插入的文本内容
 */
interface Segment {
  /** 在当前版文本中的 UTF-8 byte 起始位置 */
  currentByteStart: number
  /** 在当前版文本中的 UTF-8 byte 结束位置（exclusive） */
  currentByteEnd: number
  /** 来源类型：0 = original（初版幸存），1 = inserted（新增） */
  origin: number
  /** origin=0 时有效：在初版中的 UTF-8 byte 起始位置 */
  originalByteStart: number
  /** origin=0 时有效：在初版中的 UTF-8 byte 结束位置（exclusive） */
  originalByteEnd: number
  /** origin=1 时有效：插入的文本内容 */
  text: string
}

/**
 * applyPatchesToSegments 的返回类型。
 */
interface ApplyPatchesResult {
  success: boolean
  segments: Segment[]
  error: string
}

/**
 * 事务链复合器。
 *
 * 输入完整连续的每笔 base/new revision、patch、offsetMap、起始文本和最终文本，
 * 逐笔维护字符身份/片段表，输出起始显示正文→最终正文的统一可见差分
 * 和保留字符映射。
 */
export class EditorMotionTransactionComposer {
  /**
   * 复合事务链。
   *
   * @param editResults 事务链（按顺序排列的 editResult 数组）
   * @param oldText 起始显示正文
   * @param newText 最终正文
   * @param oldRevision 起始显示正文的 revision（应为 editResults[0].baseRevision）
   * @param newRevision 最终正文的 revision（应为 editResults[last].newRevision）
   * @returns 复合后的事务
   */
  static compose(
    editResults: EditorEditResult[],
    oldText: string,
    newText: string,
    oldRevision: number,
    newRevision: number
  ): ComposedTransaction {
    // 校验：空链
    if (editResults.length === 0) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: '事务链为空'
      }
    }

    // 校验：首尾 revision 匹配
    if (editResults[0].baseRevision !== oldRevision) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: `首笔 baseRevision(${editResults[0].baseRevision}) !== oldRevision(${oldRevision})`
      }
    }
    const lastEr = editResults[editResults.length - 1]
    if (lastEr.newRevision !== newRevision) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: `末笔 newRevision(${lastEr.newRevision}) !== newRevision(${newRevision})`
      }
    }

    // 校验：链内部逐项连续（每个 editResult 的 baseRevision 是前一个的 newRevision）
    for (let i = 1; i < editResults.length; i++) {
      if (editResults[i].baseRevision !== editResults[i - 1].newRevision) {
        return {
          displayPatches: [],
          offsetMap: undefined,
          isValid: false,
          validationError: `链断裂：editResults[${i}].baseRevision(${editResults[i].baseRevision}) !== editResults[${i - 1}].newRevision(${editResults[i - 1].newRevision})`
        }
      }
    }

    // 单笔事务——直接返回（坐标系已正确）
    if (editResults.length === 1) {
      return {
        displayPatches: editResults[0].displayPatches,
        offsetMap: editResults[0].offsetMap,
        isValid: true,
        validationError: null
      }
    }

    // 多笔事务——逐笔复合
    // 1. 初始化 Segment 表
    let segments = EditorMotionTransactionComposer.buildInitialSegments(oldText)
    let currentText = oldText

    // 2. 逐笔应用 patches
    for (let i = 0; i < editResults.length; i++) {
      const er = editResults[i]
      const patches = er.displayPatches

      // Issue #879 本轮复核评论修复（问题4）：逐笔验证 patch 的 UTF-8 边界合法性。
      // replaceByteStart/End 必须是当前版文本的合法 UTF-8 byte offset，
      // 不能超过文本总字节长度，不能落在多字节字符的中间。
      const currentByteLen = EditorMotionTransactionComposer.utf8ByteLengthLocal(currentText)
      for (let p = 0; p < patches.length; p++) {
        const patch = patches[p]
        if (patch.replaceByteStart > currentByteLen) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `editResults[${i}].displayPatches[${p}].replaceByteStart(${patch.replaceByteStart}) > 当前文本字节长度(${currentByteLen})`
          }
        }
        if (patch.replaceByteEndExclusive > currentByteLen) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `editResults[${i}].displayPatches[${p}].replaceByteEndExclusive(${patch.replaceByteEndExclusive}) > 当前文本字节长度(${currentByteLen})`
          }
        }
        // 验证 replaceByteStart/End 是合法 UTF-8 字符边界
        if (!EditorMotionTransactionComposer.isValidUtf8Boundary(currentText, patch.replaceByteStart)) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `editResults[${i}].displayPatches[${p}].replaceByteStart(${patch.replaceByteStart}) 不是合法 UTF-8 字符边界`
          }
        }
        if (!EditorMotionTransactionComposer.isValidUtf8Boundary(currentText, patch.replaceByteEndExclusive)) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `editResults[${i}].displayPatches[${p}].replaceByteEndExclusive(${patch.replaceByteEndExclusive}) 不是合法 UTF-8 字符边界`
          }
        }
      }

      // 用 applyPatchesToText 构建中间文本（用于最终验证）
      const nextText = EditorMotionTransactionComposer.applyPatchesToText(currentText, patches)

      // 用 Segment 表做坐标映射
      const result = EditorMotionTransactionComposer.applyPatchesToSegments(segments, patches)
      if (!result.success) {
        return {
          displayPatches: [],
          offsetMap: undefined,
          isValid: false,
          validationError: result.error
        }
      }
      segments = result.segments
      currentText = nextText
    }

    // 3. 验证最终文本（applyPatchesToText 的结果）
    if (currentText !== newText) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: '复合后文本与 newText 不一致'
      }
    }

    // Issue #879 本轮复核评论修复（问题4）：验证 Segment 表重建的文本与 newText 一致。
    // currentText 通过 applyPatchesToText 独立计算，但文字动画依赖的是 Segment 输出。
    // 必须确保 segments 重建出的正文也相同——防止 Segment 表与 currentText 静默分歧。
    const segmentRebuiltText = EditorMotionTransactionComposer.rebuildTextFromSegments(segments, oldText)
    if (segmentRebuiltText !== newText) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: 'Segment 表重建文本与 newText 不一致'
      }
    }

    // 4. 生成 displayPatches
    const oldTextByteLength = EditorMotionTransactionComposer.utf8ByteLengthLocal(oldText)
    const displayPatches = EditorMotionTransactionComposer.generateDisplayPatches(
      segments, oldTextByteLength, oldRevision, newRevision
    )

    // Issue #879 本轮复核评论修复（问题4）：验证 displayPatches 从 oldText 重放后 === newText。
    // displayPatches 的 replace 范围用最初显示正文坐标，inserted 范围用最终正文坐标。
    // 重放验证确保生成的 patches 可以从 oldText 正确重放到 finalText。
    const replayedText = EditorMotionTransactionComposer.replayDisplayPatches(displayPatches, oldText)
    if (replayedText !== newText) {
      return {
        displayPatches: [],
        offsetMap: undefined,
        isValid: false,
        validationError: 'displayPatches 重放结果与 newText 不一致'
      }
    }

    // 5. 生成 offsetMap
    const offsetMap = EditorMotionTransactionComposer.generateOffsetMap(segments)

    // Issue #879 本轮复核评论修复（问题4）：验证 offsetMap 条目的 UTF-8 边界和字符内容。
    // 每个 OffsetMapEntry 的 oldByteOffset/newByteOffset 必须是合法 UTF-8 字符边界，
    // 且 oldText[oldByteOffset, oldByteOffset+length) 的字符内容应与
    // newText[newByteOffset, newByteOffset+length) 的字符内容一致（幸存字符不变）。
    if (offsetMap !== undefined) {
      for (let e = 0; e < offsetMap.entries.length; e++) {
        const entry = offsetMap.entries[e]
        if (!EditorMotionTransactionComposer.isValidUtf8Boundary(oldText, entry.oldByteOffset)) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `offsetMap.entries[${e}].oldByteOffset(${entry.oldByteOffset}) 不是合法 UTF-8 字符边界`
          }
        }
        if (!EditorMotionTransactionComposer.isValidUtf8Boundary(newText, entry.newByteOffset)) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `offsetMap.entries[${e}].newByteOffset(${entry.newByteOffset}) 不是合法 UTF-8 字符边界`
          }
        }
        // 验证幸存字符内容一致：oldText 和 newText 在对应范围内的字符应相同
        const oldSlice = EditorMotionTransactionComposer.utf8Slice(oldText, entry.oldByteOffset, entry.length)
        const newSlice = EditorMotionTransactionComposer.utf8Slice(newText, entry.newByteOffset, entry.length)
        if (oldSlice !== newSlice) {
          return {
            displayPatches: [],
            offsetMap: undefined,
            isValid: false,
            validationError: `offsetMap.entries[${e}] 幸存字符内容不一致：old="${oldSlice}" new="${newSlice}"`
          }
        }
      }
    }

    return {
      displayPatches: displayPatches,
      offsetMap: offsetMap,
      isValid: true,
      validationError: null
    }
  }

  /**
   * 构建初始 Segment 表——整个 oldText 作为一个 original 片段。
   */
  private static buildInitialSegments(oldText: string): Segment[] {
    const byteLength = EditorMotionTransactionComposer.utf8ByteLengthLocal(oldText)
    const segments: Segment[] = [{
      currentByteStart: 0,
      currentByteEnd: byteLength,
      origin: 0,
      originalByteStart: 0,
      originalByteEnd: byteLength,
      text: ''
    }]
    return segments
  }

  /**
   * 在 Segment 表上应用 patches——切割/移除被替换的片段，插入新增片段。
   *
   * 每个 patch 的 replaceByteStart/EndExclusive 是相对于当前版文本的 UTF-8 byte offset。
   * 处理步骤：
   * 1. 复制 replaceStart 之前的完整 segments
   * 2. 切割跨在 replaceStart 上的 segment（保留左半）
   * 3. 跳过 [replaceStart, replaceEnd) 范围内的完整 segments（被替换/删除）
   * 4. 切割跨在 replaceEnd 上的 segment（保留右半）
   * 5. 插入新 segment（如果有 insertedText）
   * 6. 复制剩余 segments
   * 7. 重新计算所有 segment 的 currentByteStart/End
   */
  private static applyPatchesToSegments(
    segments: Segment[],
    patches: DisplayPatch[]
  ): ApplyPatchesResult {
    // 按 replaceByteStart 排序 patches
    const sortedPatches = patches.slice().sort((a: DisplayPatch, b: DisplayPatch): number =>
      a.replaceByteStart - b.replaceByteStart
    )

    // 验证 patches 不重叠
    for (let i = 1; i < sortedPatches.length; i++) {
      if (sortedPatches[i].replaceByteStart < sortedPatches[i - 1].replaceByteEndExclusive) {
        return {
          success: false,
          segments: [] as Segment[],
          error: `patches 重叠：patch[${i}].replaceByteStart(${sortedPatches[i].replaceByteStart}) < patch[${i - 1}].replaceByteEndExclusive(${sortedPatches[i - 1].replaceByteEndExclusive})`
        } as ApplyPatchesResult
      }
    }

    const resultSegments: Segment[] = []
    let segIdx = 0

    for (const patch of sortedPatches) {
      const replaceStart = patch.replaceByteStart
      const replaceEnd = patch.replaceByteEndExclusive

      // 验证范围合法性
      if (replaceStart > replaceEnd) {
        return {
          success: false,
          segments: [] as Segment[],
          error: `replaceByteStart(${replaceStart}) > replaceByteEndExclusive(${replaceEnd})`
        } as ApplyPatchesResult
      }

      // 1. 复制 replaceStart 之前的完整 segments
      while (segIdx < segments.length && segments[segIdx].currentByteEnd <= replaceStart) {
        resultSegments.push(segments[segIdx])
        segIdx++
      }

      // 2. 切割跨在 replaceStart 上的 segment
      if (segIdx < segments.length && segments[segIdx].currentByteStart < replaceStart) {
        const seg = segments[segIdx]
        const cutOffset = replaceStart - seg.currentByteStart

        if (seg.origin === 0) {
          // 切割 original segment——左半保留，右半留在 segments 中待处理
          resultSegments.push({
            currentByteStart: seg.currentByteStart,
            currentByteEnd: replaceStart,
            origin: 0,
            originalByteStart: seg.originalByteStart,
            originalByteEnd: seg.originalByteStart + cutOffset,
            text: ''
          })
          seg.currentByteStart = replaceStart
          seg.originalByteStart = seg.originalByteStart + cutOffset
        } else {
          // 切割 inserted segment——左半保留，右半留在 segments 中待处理
          const utf16Cut = EditorMotionTransactionComposer.utf8ToUtf16Local(seg.text, cutOffset)
          resultSegments.push({
            currentByteStart: seg.currentByteStart,
            currentByteEnd: replaceStart,
            origin: 1,
            originalByteStart: 0,
            originalByteEnd: 0,
            text: seg.text.substring(0, utf16Cut)
          })
          seg.currentByteStart = replaceStart
          seg.text = seg.text.substring(utf16Cut)
        }
      }

      // 3. 跳过 [replaceStart, replaceEnd) 范围内的完整 segments（被替换/删除）
      while (segIdx < segments.length && segments[segIdx].currentByteEnd <= replaceEnd) {
        segIdx++
      }

      // 4. 切割跨在 replaceEnd 上的 segment——左半被替换/删除，右半保留
      if (segIdx < segments.length && segments[segIdx].currentByteStart < replaceEnd) {
        const seg = segments[segIdx]
        const cutOffset = replaceEnd - seg.currentByteStart

        if (seg.origin === 0) {
          seg.currentByteStart = replaceEnd
          seg.originalByteStart = seg.originalByteStart + cutOffset
        } else {
          const utf16Cut = EditorMotionTransactionComposer.utf8ToUtf16Local(seg.text, cutOffset)
          seg.currentByteStart = replaceEnd
          seg.text = seg.text.substring(utf16Cut)
        }
      }

      // 5. 插入新 segment（如果有 insertedText）
      if (patch.insertedText.length > 0) {
        resultSegments.push({
          currentByteStart: 0,
          currentByteEnd: 0,
          origin: 1,
          originalByteStart: 0,
          originalByteEnd: 0,
          text: patch.insertedText
        })
      }
    }

    // 6. 复制剩余 segments
    while (segIdx < segments.length) {
      resultSegments.push(segments[segIdx])
      segIdx++
    }

    // 7. 重新计算 currentByteStart/End
    let currentBytePos = 0
    for (const seg of resultSegments) {
      seg.currentByteStart = currentBytePos
      if (seg.origin === 0) {
        seg.currentByteEnd = currentBytePos + (seg.originalByteEnd - seg.originalByteStart)
      } else {
        seg.currentByteEnd = currentBytePos + EditorMotionTransactionComposer.utf8ByteLengthLocal(seg.text)
      }
      currentBytePos = seg.currentByteEnd
    }

    return { success: true, segments: resultSegments, error: '' } as ApplyPatchesResult
  }

  /**
   * 从最终 Segment 表生成 displayPatches。
   *
   * 遍历 segments，跟踪 oldCursor（初版 byte 游标）和 newCursor（最终版 byte 游标），
   * 生成原始 patch 序列（INSERT/DELETE），再合并相邻的 INSERT+DELETE 为 REPLACE。
   *
   * - origin=0 的 segment：如果 oldCursor < originalByteStart，说明初版的
   *   [oldCursor, originalByteStart) 被删除了，生成 DELETE patch。
   * - origin=1 的 segment：生成 INSERT patch。
   * - 相邻的 INSERT+DELETE 或 DELETE+INSERT 合并为 REPLACE patch。
   */
  private static generateDisplayPatches(
    segments: Segment[],
    oldTextByteLength: number,
    oldRevision: number,
    newRevision: number
  ): DisplayPatch[] {
    // 1. 生成原始 patch 序列
    const rawPatches: DisplayPatch[] = []
    let oldCursor = 0
    let newCursor = 0

    for (const seg of segments) {
      if (seg.origin === 0) {
        // 检查是否有删除
        if (oldCursor < seg.originalByteStart) {
          // DELETE：初版 [oldCursor, originalByteStart) 被删除
          rawPatches.push({
            baseRevision: oldRevision,
            newRevision: newRevision,
            replaceByteStart: oldCursor,
            replaceByteEndExclusive: seg.originalByteStart,
            insertedByteStart: newCursor,
            insertedByteEndExclusive: newCursor,
            insertedText: '',
            resultingSelectionStart: 0,
            resultingSelectionEnd: 0
          })
        }
        oldCursor = seg.originalByteEnd
        newCursor = seg.currentByteEnd
      } else {
        // INSERT：在最终版 [newCursor, currentByteEnd) 插入了 text
        rawPatches.push({
          baseRevision: oldRevision,
          newRevision: newRevision,
          replaceByteStart: oldCursor,
          replaceByteEndExclusive: oldCursor,
          insertedByteStart: newCursor,
          insertedByteEndExclusive: seg.currentByteEnd,
          insertedText: seg.text,
          resultingSelectionStart: 0,
          resultingSelectionEnd: 0
        })
        newCursor = seg.currentByteEnd
      }
    }

    // 末尾删除
    if (oldCursor < oldTextByteLength) {
      rawPatches.push({
        baseRevision: oldRevision,
        newRevision: newRevision,
        replaceByteStart: oldCursor,
        replaceByteEndExclusive: oldTextByteLength,
        insertedByteStart: newCursor,
        insertedByteEndExclusive: newCursor,
        insertedText: '',
        resultingSelectionStart: 0,
        resultingSelectionEnd: 0
      })
    }

    // 2. 合并相邻的 INSERT+DELETE 或 DELETE+INSERT 为 REPLACE
    const mergedPatches: DisplayPatch[] = []
    let i = 0
    while (i < rawPatches.length) {
      const cur = rawPatches[i]
      if (i + 1 < rawPatches.length) {
        const next = rawPatches[i + 1]
        // DELETE + INSERT：cur 是删除（insertedText=''），next 是插入（replaceStart===replaceEnd）
        if (cur.insertedText === '' &&
            next.replaceByteStart === next.replaceByteEndExclusive &&
            cur.insertedByteEndExclusive === next.insertedByteStart) {
          mergedPatches.push({
            baseRevision: oldRevision,
            newRevision: newRevision,
            replaceByteStart: cur.replaceByteStart,
            replaceByteEndExclusive: cur.replaceByteEndExclusive,
            insertedByteStart: next.insertedByteStart,
            insertedByteEndExclusive: next.insertedByteEndExclusive,
            insertedText: next.insertedText,
            resultingSelectionStart: 0,
            resultingSelectionEnd: 0
          })
          i += 2
          continue
        }
        // INSERT + DELETE：cur 是插入，next 是删除
        if (next.insertedText === '' &&
            cur.replaceByteStart === cur.replaceByteEndExclusive &&
            cur.replaceByteEndExclusive === next.replaceByteStart) {
          mergedPatches.push({
            baseRevision: oldRevision,
            newRevision: newRevision,
            replaceByteStart: next.replaceByteStart,
            replaceByteEndExclusive: next.replaceByteEndExclusive,
            insertedByteStart: cur.insertedByteStart,
            insertedByteEndExclusive: cur.insertedByteEndExclusive,
            insertedText: cur.insertedText,
            resultingSelectionStart: 0,
            resultingSelectionEnd: 0
          })
          i += 2
          continue
        }
      }
      mergedPatches.push(cur)
      i++
    }

    return mergedPatches
  }

  /**
   * 从最终 Segment 表生成 offsetMap（old→final）。
   *
   * 仅 origin=0 的 segment 有对应关系——它们是初版的幸存字符，在最终版中有确定位置。
   * origin=1 的 segment 是新增字符，在初版中没有对应位置，不出现在 offsetMap 中。
   *
   * length 依据 segment 的 originalByteEnd - originalByteStart 计算（等于 currentByteEnd - currentByteStart），
   * 绝不保留原长或对删除空洞外推。
   */
  private static generateOffsetMap(segments: Segment[]): OffsetMap | undefined {
    const entries: OffsetMapEntry[] = []
    for (const seg of segments) {
      if (seg.origin === 0) {
        const length = seg.originalByteEnd - seg.originalByteStart
        if (length > 0) {
          const kind = seg.originalByteStart === seg.currentByteStart
            ? OffsetMapKind.Identity
            : OffsetMapKind.Shifted
          entries.push({
            oldByteOffset: seg.originalByteStart,
            newByteOffset: seg.currentByteStart,
            length: length,
            kind: kind
          })
        }
      }
    }

    if (entries.length === 0) {
      return undefined
    }

    return { entries: entries }
  }

  /**
   * 逐笔应用 patch 到文本上，重建中间正文。
   *
   * patch 的 replaceByteStart/End 是旧正文 UTF-8 byte offset，
   * insertedText 是插入的文本内容。
   * 应用方式：旧正文[0, replaceByteStart) + insertedText + 旧正文[replaceByteEndExclusive, end)
   */
  private static applyPatchesToText(text: string, patches: DisplayPatch[]): string {
    // 按 replaceByteStart 排序，确保从左到右应用
    const sorted = patches.slice().sort((a: DisplayPatch, b: DisplayPatch): number =>
      a.replaceByteStart - b.replaceByteStart
    )

    // 将 byte offset 转为 UTF-16 offset 并逐步拼接
    let result = ''
    let lastUtf16End = 0

    for (const patch of sorted) {
      // patch.replaceByteStart 是旧正文的 UTF-8 byte offset
      // 需要转换为 UTF-16 offset
      const replaceUtf16Start = EditorMotionTransactionComposer.utf8ToUtf16Local(text, patch.replaceByteStart)
      const replaceUtf16End = EditorMotionTransactionComposer.utf8ToUtf16Local(text, patch.replaceByteEndExclusive)

      // 拼接未被替换的前段
      result += text.substring(lastUtf16End, replaceUtf16Start)
      // 拼接插入的文本
      result += patch.insertedText

      lastUtf16End = replaceUtf16End
    }

    // 拼接末尾未被替换的部分
    result += text.substring(lastUtf16End)

    return result
  }

  /**
   * UTF-8 byte offset → UTF-16 code unit offset 的本地实现。
   * 不依赖 TextOffsetMapper（避免循环 import）。
   */
  private static utf8ToUtf16Local(text: string, utf8Offset: number): number {
    let bytePos = 0
    for (let i = 0; i < text.length; i++) {
      if (bytePos >= utf8Offset) {
        return i
      }
      const codePoint = text.codePointAt(i)!
      if (codePoint <= 0x7F) {
        bytePos += 1
      } else if (codePoint <= 0x7FF) {
        bytePos += 2
      } else if (codePoint <= 0xFFFF) {
        bytePos += 3
      } else {
        bytePos += 4
        i++ // 跳过 surrogate pair 的第二个 code unit
      }
    }
    return text.length
  }

  /**
   * 计算字符串的 UTF-8 byte length。
   */
  private static utf8ByteLengthLocal(text: string): number {
    let byteLength = 0
    for (let i = 0; i < text.length; i++) {
      const codePoint = text.codePointAt(i)!
      if (codePoint <= 0x7F) {
        byteLength += 1
      } else if (codePoint <= 0x7FF) {
        byteLength += 2
      } else if (codePoint <= 0xFFFF) {
        byteLength += 3
      } else {
        byteLength += 4
        i++ // 跳过 surrogate pair 的第二个 code unit
      }
    }
    return byteLength
  }

  // ── Issue #879 本轮复核评论修复（问题4）：验证辅助方法 ──

  /**
   * 验证给定的 UTF-8 byte offset 是否是 text 中的合法字符边界。
   *
   * 合法字符边界意味着 offset 要么等于 0（文本开头），
   * 要么等于文本的字节长度（文本末尾），
   * 要么恰好落在一个完整 UTF-8 字符的起始位置。
   * 不能落在多字节字符的中间字节上。
   */
  private static isValidUtf8Boundary(text: string, utf8Offset: number): boolean {
    if (utf8Offset < 0) {
      return false
    }
    if (utf8Offset === 0) {
      return true
    }
    let bytePos = 0
    for (let i = 0; i < text.length; i++) {
      if (bytePos === utf8Offset) {
        return true
      }
      const codePoint = text.codePointAt(i)!
      if (codePoint <= 0x7F) {
        bytePos += 1
      } else if (codePoint <= 0x7FF) {
        bytePos += 2
      } else if (codePoint <= 0xFFFF) {
        bytePos += 3
      } else {
        bytePos += 4
        i++ // 跳过 surrogate pair 的第二个 code unit
      }
    }
    return bytePos === utf8Offset
  }

  /**
   * 从 Segment 表重建文本。
   *
   * origin=0 的 segment 从 oldText 中取 originalByteStart~originalByteEnd 的字符，
   * origin=1 的 segment 直接取 text 字段。
   * 重建结果应与 newText 一致——如果不一致说明 Segment 表有错误。
   */
  private static rebuildTextFromSegments(segments: Segment[], oldText: string): string {
    let result = ''
    for (const seg of segments) {
      if (seg.origin === 0) {
        // 从 oldText 中取对应范围的字符
        const utf16Start = EditorMotionTransactionComposer.utf8ToUtf16Local(oldText, seg.originalByteStart)
        const utf16End = EditorMotionTransactionComposer.utf8ToUtf16Local(oldText, seg.originalByteEnd)
        result += oldText.substring(utf16Start, utf16End)
      } else {
        result += seg.text
      }
    }
    return result
  }

  /**
   * 从 oldText 重放 displayPatches，验证结果是否等于 expectedText。
   *
   * displayPatches 的 replaceByteStart/End 是 oldText 的 UTF-8 byte offset，
   * insertedText 是插入的文本内容。
   * 重放方式：oldText[0, replaceStart) + insertedText + oldText[replaceEnd, end)，
   * 多个 patch 按顺序从左到右应用。
   */
  private static replayDisplayPatches(patches: DisplayPatch[], oldText: string): string {
    // 按 replaceByteStart 排序
    const sorted = patches.slice().sort((a: DisplayPatch, b: DisplayPatch): number =>
      a.replaceByteStart - b.replaceByteStart
    )

    let result = ''
    let lastUtf16End = 0

    for (const patch of sorted) {
      const replaceUtf16Start = EditorMotionTransactionComposer.utf8ToUtf16Local(oldText, patch.replaceByteStart)
      const replaceUtf16End = EditorMotionTransactionComposer.utf8ToUtf16Local(oldText, patch.replaceByteEndExclusive)

      result += oldText.substring(lastUtf16End, replaceUtf16Start)
      result += patch.insertedText

      lastUtf16End = replaceUtf16End
    }

    result += oldText.substring(lastUtf16End)
    return result
  }

  /**
   * 从 text 的指定 UTF-8 byte offset 开始，取 length 个字节的字符内容。
   *
   * 用于验证 offsetMap 中幸存字符的 old/new 内容是否一致。
   * 返回对应的 UTF-16 子串。
   */
  private static utf8Slice(text: string, utf8ByteStart: number, utf8ByteLength: number): string {
    const utf16Start = EditorMotionTransactionComposer.utf8ToUtf16Local(text, utf8ByteStart)
    const utf16End = EditorMotionTransactionComposer.utf8ToUtf16Local(text, utf8ByteStart + utf8ByteLength)
    return text.substring(utf16Start, utf16End)
  }
}
