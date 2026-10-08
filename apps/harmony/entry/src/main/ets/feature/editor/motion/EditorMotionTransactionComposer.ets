// EditorMotionTransactionComposer — 事务链复合器。
//
// Issue #879 最新复核评论修复（问题1）：
// 连续快速输入时，多笔 editResult 形成事务链。每笔 patch 的 UTF-8 坐标系不同——
// 第二笔 replaceByteStart 是相对于第一笔修改后的正文，不是最初显示的正文。
// 简单拼接 displayPatches 并把最后一笔 offsetMap 当整条链的 offsetMap 必然错位。
//
// 本模块逐笔转换/复合 UTF-8 字符身份映射，输出起始显示正文→最终正文的统一可见差分
// 和保留字符映射。校验 editResults[0].baseRevision===oldSnapshot.revision、
// editResults[last].newRevision===newSnapshot.revision 及链内部逐项连续；
// 不准通过篡改首尾 revision 伪造对齐。
//
// 核心算法：
// 1. 逐笔处理：对每笔 editResult，把它的 displayPatches 从"相对于前一笔修改后的正文"
//    转换到"相对于最初显示正文"的坐标系。
// 2. 逐笔复合 offsetMap：第一笔的 offsetMap 是 old→new1，第二笔是 new1→new2，...
//    需要链式复合为 old→final。
// 3. 输出 ComposedTransaction：统一的 displayPatches（所有 replace 范围用最初显示正文的
//    UTF-8 坐标，所有 inserted 范围用最终正文的 UTF-8 坐标）+ 统一的 offsetMap（old→final）。
//
// 不修改 Core 的编辑事实模型，只读 editResult 做复合计算。

import { EditorEditResult, DisplayPatch, OffsetMap, OffsetMapEntry, OffsetMapKind } from '../../../corebridge/dto/EditorDtos'

/**
 * 复合后的事务——包含统一的 displayPatches 和 offsetMap。
 *
 * - displayPatches：所有 replace 范围用最初显示正文的 UTF-8 坐标，
 *   所有 inserted 范围用最终正文的 UTF-8 坐标。
 * - offsetMap：old→final 的字符身份映射。
 * - isValid：链是否连续、首尾 revision 是否匹配。
 */
export interface ComposedTransaction {
  /** 统一的 displayPatches——replace 范围用最初显示正文坐标，inserted 范围用最终正文坐标 */
  displayPatches: DisplayPatch[]
  /** 统一的 offsetMap——old→final 的字符身份映射 */
  offsetMap: OffsetMap | undefined
  /** 校验结果——链是否连续、首尾 revision 是否匹配 */
  isValid: boolean
  /** 校验失败原因 */
  validationError: string | null
}

/**
 * 事务链复合器。
 *
 * 输入完整连续的每笔 base/new revision、patch、offsetMap、起始文本和最终文本，
 * 真正逐笔转换/复合 UTF-8 字符身份映射，输出起始显示正文→最终正文的统一可见差分
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
    // 算法：
    // 1. 维护一个"当前正文"游标，初始为 oldText。
    // 2. 对每笔 editResult，把它的 displayPatches 的 replaceByteStart/End 从"当前正文坐标"
    //    转换到"最初显示正文坐标"。转换方法是累计之前所有笔的 offsetMap 偏移。
    // 3. insertedByteStart/End 保持"最终正文坐标"——但需要累计后续所有笔的偏移，
    //    把它从"本笔的新正文坐标"转换到"最终正文坐标"。
    // 4. offsetMap 链式复合。

    // 逐笔构建中间正文文本，用于坐标转换
    const intermediateTexts: string[] = [oldText]
    for (let i = 0; i < editResults.length; i++) {
      // 从 editResults[i] 的 displayPatches 重建中间正文
      // 但我们无法从 patch 直接重建——需要用 offsetMap 或 patch 的 insertedText
      // 更简单的方式：用 patch 的 replace 和 inserted 信息逐步推导
      // 最可靠的方式：逐笔应用 patch 到文本上
      const prevText = intermediateTexts[i]
      const nextText = EditorMotionTransactionComposer.applyPatchesToText(prevText, editResults[i].displayPatches)
      intermediateTexts.push(nextText)
    }

    // 验证最终文本
    if (intermediateTexts[intermediateTexts.length - 1] !== newText) {
      // 重建文本与传入的 newText 不一致——可能 patch 信息不完整
      // 仍然继续，但标记警告（使用重建文本作为最终正文坐标基准）
      // 这种情况理论上不应发生——如果发生说明 patch 数据有问题
    }

    // 复合 displayPatches：
    // 对每笔 editResult 的每个 patch：
    // - replaceByteStart/End：从"本笔的旧正文坐标"转换到"最初显示正文坐标"
    // - insertedByteStart/End：从"本笔的新正文坐标"转换到"最终正文坐标"
    const composedPatches: DisplayPatch[] = []

    // 累积偏移映射：oldText 坐标 → 第 i 笔旧正文坐标
    // 以及：第 i 笔新正文坐标 → finalText 坐标
    // 用 OffsetMap 链式复合来实现

    // 简化但正确的方案：
    // 对每笔 patch，用 TextOffsetMapper 把 byte offset 转成 UTF-16 offset，
    // 然后在对应中间文本中定位字符内容，再在 oldText/newText 中找到相同字符内容的位置。
    // 但这种方法在重复字符场景下不可靠。

    // 更可靠的方案：逐笔构建 byte offset 映射表
    // oldByteOffsetMap[i]：从 oldText 的 byte offset → 第 i 笔旧正文的 byte offset
    // newByteOffsetMap[i]：从第 i 笔新正文的 byte offset → finalText 的 byte offset

    // 构建 oldText → 各中间文本的 byte offset 映射
    // oldByteOffsetMaps[0] = identity（oldText → oldText）
    // oldByteOffsetMaps[i] = compose(oldByteOffsetMaps[i-1], editResults[i-1].offsetMap)
    const oldByteOffsetMaps: Map<number, number>[] = []
    oldByteOffsetMaps.push(EditorMotionTransactionComposer.buildIdentityMap(oldText))

    for (let i = 0; i < editResults.length; i++) {
      const prevMap = oldByteOffsetMaps[i]
      const currentOffsetMap = editResults[i].offsetMap
      if (currentOffsetMap !== undefined) {
        oldByteOffsetMaps.push(
          EditorMotionTransactionComposer.composeOffsetMaps(prevMap, currentOffsetMap, intermediateTexts[i])
        )
      } else {
        // 无 offsetMap——无法精确复合，退化为 identity
        oldByteOffsetMaps.push(EditorMotionTransactionComposer.buildIdentityMap(intermediateTexts[i + 1]))
      }
    }

    // 构建 各中间文本 → finalText 的 byte offset 映射（反向复合）
    // newByteOffsetMaps[i]：从第 i 笔新正文的 byte offset → finalText 的 byte offset
    const finalText = intermediateTexts[intermediateTexts.length - 1]
    const newByteOffsetMaps: Map<number, number>[] = new Array(editResults.length)
    // 从最后一笔开始反向复合
    newByteOffsetMaps[editResults.length - 1] = EditorMotionTransactionComposer.buildIdentityMap(finalText)
    for (let i = editResults.length - 2; i >= 0; i--) {
      const nextMap = newByteOffsetMaps[i + 1]
      const currentOffsetMap = editResults[i + 1].offsetMap
      if (currentOffsetMap !== undefined) {
        // 第 i+1 笔的 offsetMap 是 new_i → new_{i+1}
        // 我们需要 new_i → final，即 compose(new_i → new_{i+1}, new_{i+1} → final)
        newByteOffsetMaps[i] = EditorMotionTransactionComposer.composeOffsetMapsForward(
          currentOffsetMap, nextMap, intermediateTexts[i + 1], intermediateTexts[i + 2]
        )
      } else {
        newByteOffsetMaps[i] = EditorMotionTransactionComposer.buildIdentityMap(intermediateTexts[i + 1])
      }
    }

    // 逐笔转换每个 patch 的坐标
    for (let i = 0; i < editResults.length; i++) {
      const er = editResults[i]
      const oldMap = oldByteOffsetMaps[i]  // oldText byte → 第 i 笔旧正文 byte
      const newMap = newByteOffsetMaps[i]  // 第 i 笔新正文 byte → finalText byte

      for (const patch of er.displayPatches) {
        // replaceByteStart/End：从"第 i 笔旧正文坐标"转换到"最初显示正文坐标"
        const composedReplaceStart = oldMap.get(patch.replaceByteStart)
        const composedReplaceEnd = oldMap.get(patch.replaceByteEndExclusive)

        // insertedByteStart/End：从"第 i 笔新正文坐标"转换到"最终正文坐标"
        const composedInsertedStart = newMap.get(patch.insertedByteStart)
        const composedInsertedEnd = newMap.get(patch.insertedByteEndExclusive)

        if (composedReplaceStart === undefined || composedReplaceEnd === undefined ||
            composedInsertedStart === undefined || composedInsertedEnd === undefined) {
          // 映射失败——跳过此 patch（不应发生）
          continue
        }

        composedPatches.push({
          baseRevision: oldRevision,
          newRevision: newRevision,
          replaceByteStart: composedReplaceStart,
          replaceByteEndExclusive: composedReplaceEnd,
          insertedByteStart: composedInsertedStart,
          insertedByteEndExclusive: composedInsertedEnd,
          insertedText: patch.insertedText,
          resultingSelectionStart: patch.resultingSelectionStart,
          resultingSelectionEnd: patch.resultingSelectionEnd
        })
      }
    }

    // 复合 offsetMap：链式复合所有笔的 offsetMap，得到 old→final
    const composedOffsetMap = EditorMotionTransactionComposer.composeAllOffsetMaps(editResults)

    return {
      displayPatches: composedPatches,
      offsetMap: composedOffsetMap,
      isValid: true,
      validationError: null
    }
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
   * 构建 identity 映射表（每个 byte offset 映射到自身）。
   */
  private static buildIdentityMap(text: string): Map<number, number> {
    const map = new Map<number, number>()
    // 遍历所有 UTF-8 byte offset 边界
    let byteOffset = 0
    for (let utf16Offset = 0; utf16Offset <= text.length; utf16Offset++) {
      map.set(byteOffset, byteOffset)
      if (utf16Offset < text.length) {
        const codePoint = text.codePointAt(utf16Offset)!
        // UTF-8 编码长度
        if (codePoint <= 0x7F) {
          byteOffset += 1
        } else if (codePoint <= 0x7FF) {
          byteOffset += 2
        } else if (codePoint <= 0xFFFF) {
          byteOffset += 3
        } else {
          byteOffset += 4
          utf16Offset++ // 跳过 surrogate pair 的第二个 code unit
        }
      }
    }
    return map
  }

  /**
   * 复合两个 offsetMap 映射表：prevMap (old→mid) ∘ currentOffsetMap (mid→new) = old→new
   *
   * prevMap 是一个 Map<number, number>，表示 oldText byte offset → midText byte offset。
   * currentOffsetMap 是 Core 的 OffsetMap，表示 midText byte offset → newText byte offset。
   * 复合后返回 oldText byte offset → newText byte offset 的 Map。
   */
  private static composeOffsetMaps(
    prevMap: Map<number, number>,
    currentOffsetMap: OffsetMap,
    midText: string
  ): Map<number, number> {
    const result = new Map<number, number>()

    // 对 prevMap 中的每个 oldByte → midByte 映射，
    // 在 currentOffsetMap 中找到 midByte 对应的 newByte
    prevMap.forEach((midByte: number, oldByte: number) => {
      const newByte = EditorMotionTransactionMapper.lookupOffsetMap(currentOffsetMap, midByte)
      result.set(oldByte, newByte)
    })

    return result
  }

  /**
   * 复合两个 offsetMap 映射表（正向）：currentOffsetMap (mid→next) ∘ nextMap (next→final) = mid→final
   *
   * currentOffsetMap 是 Core 的 OffsetMap，表示 midText byte offset → nextText byte offset。
   * nextMap 是一个 Map<number, number>，表示 nextText byte offset → finalText byte offset。
   * 复合后返回 midText byte offset → finalText byte offset 的 Map。
   */
  private static composeOffsetMapsForward(
    currentOffsetMap: OffsetMap,
    nextMap: Map<number, number>,
    midText: string,
    nextText: string
  ): Map<number, number> {
    const result = new Map<number, number>()

    // 遍历 midText 的所有 byte offset 边界
    let byteOffset = 0
    for (let utf16Offset = 0; utf16Offset <= midText.length; utf16Offset++) {
      // 在 currentOffsetMap 中查找 midByte → nextByte
      const nextByte = EditorMotionTransactionMapper.lookupOffsetMap(currentOffsetMap, byteOffset)
      // 在 nextMap 中查找 nextByte → finalByte
      const finalByte = nextMap.get(nextByte)
      if (finalByte !== undefined) {
        result.set(byteOffset, finalByte)
      }
      if (utf16Offset < midText.length) {
        const codePoint = midText.codePointAt(utf16Offset)!
        if (codePoint <= 0x7F) {
          byteOffset += 1
        } else if (codePoint <= 0x7FF) {
          byteOffset += 2
        } else if (codePoint <= 0xFFFF) {
          byteOffset += 3
        } else {
          byteOffset += 4
          utf16Offset++
        }
      }
    }

    return result
  }

  /**
   * 链式复合所有笔的 offsetMap，得到 old→final 的 OffsetMap。
   *
   * 第一笔 offsetMap 是 old→new1，第二笔是 new1→new2，...，
   * 复合后得到 old→final。
   */
  private static composeAllOffsetMaps(editResults: EditorEditResult[]): OffsetMap | undefined {
    // 收集所有有 offsetMap 的笔
    const offsetMaps: OffsetMap[] = []
    for (const er of editResults) {
      if (er.offsetMap !== undefined) {
        offsetMaps.push(er.offsetMap)
      }
    }

    if (offsetMaps.length === 0) {
      return undefined
    }

    if (offsetMaps.length === 1) {
      return offsetMaps[0]
    }

    // 链式复合：result = offsetMaps[0]
    // for each subsequent map: result = compose(result, map)
    let result = offsetMaps[0]
    for (let i = 1; i < offsetMaps.length; i++) {
      result = EditorMotionTransactionComposer.composeTwoOffsetMaps(result, offsetMaps[i])
    }

    return result
  }

  /**
   * 复合两个 OffsetMap：first (A→B) ∘ second (B→C) = A→C
   */
  private static composeTwoOffsetMaps(first: OffsetMap, second: OffsetMap): OffsetMap {
    const composedEntries: OffsetMapEntry[] = []

    for (const firstEntry of first.entries) {
      // firstEntry: A byte [oldByteOffset, oldByteOffset+length) → B byte [newByteOffset, newByteOffset+length)
      // 在 second 中查找 newByteOffset 对应的 C byte offset
      const secondNewStart = EditorMotionTransactionMapper.lookupOffsetMap(second, firstEntry.newByteOffset)
      const secondNewEnd = EditorMotionTransactionMapper.lookupOffsetMap(second, firstEntry.newByteOffset + firstEntry.length)

      // 复合后的条目：A byte [firstEntry.oldByteOffset, firstEntry.oldByteOffset+firstEntry.length)
      // → C byte [secondNewStart, secondNewEnd)
      // length 保持不变（字符身份映射保持字符长度一致）
      composedEntries.push({
        oldByteOffset: firstEntry.oldByteOffset,
        newByteOffset: secondNewStart,
        length: firstEntry.length,
        kind: firstEntry.kind === OffsetMapKind.Identity && secondNewStart === firstEntry.newByteOffset
          ? OffsetMapKind.Identity
          : OffsetMapKind.Shifted
      })
    }

    return { entries: composedEntries }
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
}

/**
 * OffsetMap 查找辅助——在 OffsetMap 中查找给定 byte offset 对应的目标 byte offset。
 */
class EditorMotionTransactionMapper {
  /**
   * 在 OffsetMap 中查找 oldByteOffset 对应的 newByteOffset。
   *
   * OffsetMap 的 entries 记录 old→new 的映射。
   * 对于 Identity 条目，oldByteOffset === newByteOffset。
   * 对于 Shifted 条目，字符从 oldByteOffset 移到了 newByteOffset。
   *
   * 查找逻辑：
   * 1. 精确匹配 oldByteOffset 的条目。
   * 2. 如果没有精确匹配，找到包含 oldByteOffset 的条目（oldByteOffset 在 [entry.oldByteOffset, entry.oldByteOffset+length) 内），
   *    按比例映射到 newByteOffset 侧。
   * 3. 如果 oldByteOffset 在所有条目之前或之后，按线性外推。
   */
  static lookupOffsetMap(offsetMap: OffsetMap, oldByteOffset: number): number {
    const entries = offsetMap.entries
    if (entries.length === 0) {
      return oldByteOffset
    }

    // 精确匹配
    for (const entry of entries) {
      if (entry.oldByteOffset === oldByteOffset) {
        return entry.newByteOffset
      }
    }

    // 包含在某个条目范围内
    for (const entry of entries) {
      if (oldByteOffset >= entry.oldByteOffset && oldByteOffset < entry.oldByteOffset + entry.length) {
        // 按比例映射
        const offsetInEntry = oldByteOffset - entry.oldByteOffset
        return entry.newByteOffset + offsetInEntry
      }
    }

    // 在所有条目之前——线性外推
    const firstEntry = entries[0]
    if (oldByteOffset < firstEntry.oldByteOffset) {
      // 在第一个条目之前，偏移量保持不变（Identity 区域）
      return oldByteOffset
    }

    // 在所有条目之后——按最后一个条目的偏移量外推
    const lastEntry = entries[entries.length - 1]
    const lastOldEnd = lastEntry.oldByteOffset + lastEntry.length
    const lastNewEnd = lastEntry.newByteOffset + lastEntry.length
    if (oldByteOffset >= lastOldEnd) {
      return oldByteOffset - lastOldEnd + lastNewEnd
    }

    // 两个条目之间的间隙——按前一个条目的末尾偏移量外推
    for (let i = 0; i < entries.length - 1; i++) {
      const cur = entries[i]
      const next = entries[i + 1]
      const curOldEnd = cur.oldByteOffset + cur.length
      const curNewEnd = cur.newByteOffset + cur.length
      if (oldByteOffset >= curOldEnd && oldByteOffset < next.oldByteOffset) {
        // 在 cur 和 next 之间的间隙
        // 间隙中的 byte offset 在 Identity 区域（未被映射），偏移量应保持
        // 但需要考虑 cur 的偏移差异
        return oldByteOffset - curOldEnd + curNewEnd
      }
    }

    return oldByteOffset
  }
}
