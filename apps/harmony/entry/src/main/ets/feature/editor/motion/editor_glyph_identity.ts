// editor_glyph_identity.ts — 跨修订稳定的字形身份表。
//
// Issue #879 最新复核评论问题3：动画查找键必须跨修订稳定。
// 旧的 `utf16Start-utf16End-hash(textSlice)` 在字前插入/删除后 offset 平移即失配，
// 相同内容出现在相同位置也可能被误认成同一个字。
//
// 本模块维护一张随每次编辑“搬运”的「字符簇 → stableGlyphId」表：
// - 建立时按字符簇分配 id（`orig-<revision>-<utf16Start>`）。
// - 每笔事务按 patch 逐笔搬运：被替换范围覆盖的条目丢弃；幸存条目保留原 id、
//   位置随搬运平移；新插入的文本按字符簇分配新 id（`ins-<revision>-<utf16Start>`）。
// - 因此同一个字在整条编辑历史里始终是同一个 id，与它的当前位置、old/new 角色无关。
//
// 纯逻辑：不依赖 ArkUI、不 import .ets，生产由 motion 层 .ets 调用，Node 单测直接 import。

import type { GlyphCarryPatch } from './editor_patch_carry.ts'
import { toUtf16CarryPatches } from './editor_patch_carry.ts'

/** 一个字形身份条目——一段连续字符簇及其稳定身份。 */
export interface GlyphIdentityEntry {
  /** 在当前文本中的 UTF-16 起始 offset */
  utf16Start: number
  /** 在当前文本中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 稳定字形身份——跨修订不变，直到这段字符被删除 */
  glyphId: string
}

/**
 * 判断一个码点是否「附着」在前一个字符上（组合字符 / 变体选择符 / ZWJ / 肤色修饰符）。
 *
 * 这里不做完整 Unicode grapheme 分段，只覆盖影响排版的常见情况：
 * 组合附加符号、变体选择符、ZWJ 连接序列、emoji 肤色修饰符。
 * 过度拆分不影响正确性——每个片段仍然拿到自己的稳定身份，
 * 而动画窗口只按「起始字符簇的身份」匹配。
 */
function isClusterContinuation(codePoint: number): boolean {
  if (codePoint === 0x200D) {
    return true // ZWJ
  }
  if (codePoint >= 0xFE00 && codePoint <= 0xFE0F) {
    return true // 变体选择符
  }
  if (codePoint >= 0xE0100 && codePoint <= 0xE01EF) {
    return true // 补充变体选择符
  }
  if (codePoint >= 0x1F3FB && codePoint <= 0x1F3FF) {
    return true // emoji 肤色修饰符
  }
  return (codePoint >= 0x0300 && codePoint <= 0x036F) ||
    (codePoint >= 0x0483 && codePoint <= 0x0489) ||
    (codePoint >= 0x0591 && codePoint <= 0x05BD) ||
    (codePoint >= 0x0610 && codePoint <= 0x061A) ||
    (codePoint >= 0x064B && codePoint <= 0x065F) ||
    (codePoint >= 0x06D6 && codePoint <= 0x06DC) ||
    (codePoint >= 0x0730 && codePoint <= 0x074A) ||
    (codePoint >= 0x07A6 && codePoint <= 0x07B0) ||
    (codePoint >= 0x0900 && codePoint <= 0x0903) ||
    (codePoint >= 0x093A && codePoint <= 0x094F) ||
    (codePoint >= 0x0951 && codePoint <= 0x0957) ||
    (codePoint >= 0x0E31 && codePoint <= 0x0E3A) ||
    (codePoint >= 0x0E47 && codePoint <= 0x0E4E) ||
    (codePoint >= 0x1AB0 && codePoint <= 0x1AFF) ||
    (codePoint >= 0x1DC0 && codePoint <= 0x1DFF) ||
    (codePoint >= 0x20D0 && codePoint <= 0x20FF) ||
    (codePoint >= 0xFE20 && codePoint <= 0xFE2F)
}

/** 一个字符簇的 UTF-16 区间。 */
export interface ClusterRange {
  start: number
  end: number
}

/**
 * 把文本切成字符簇的 UTF-16 区间列表（不含身份）。
 */
function splitClusterRanges(text: string): ClusterRange[] {
  const ranges: ClusterRange[] = []
  let index = 0
  while (index < text.length) {
    const codePoint = text.codePointAt(index) ?? 0
    let end = index + (codePoint > 0xFFFF ? 2 : 1)
    // 后续附着字符并入当前簇：ZWJ 后面的字符也并进来，保证 emoji 序列整体一个身份。
    while (end < text.length) {
      const nextCodePoint = text.codePointAt(end) ?? 0
      const prevWasJoiner = text.charCodeAt(end - 1) === 0x200D
      if (!prevWasJoiner && !isClusterContinuation(nextCodePoint)) {
        break
      }
      end += nextCodePoint > 0xFFFF ? 2 : 1
    }
    ranges.push({ start: index, end: end })
    index = end
  }
  return ranges
}

/**
 * 稳定字形身份表。
 *
 * 一张表描述一个 revision 的完整文本：条目按位置升序、首尾相接覆盖 [0, text.length)。
 * 每次编辑产生一张新表（不可变），幸存字符沿用旧 id。
 */
export class GlyphIdentityTable {
  /** 本表描述的 revision */
  readonly revision: number
  /** 本表描述的正文 */
  readonly text: string
  /** 字符簇身份条目（按 utf16Start 升序，首尾相接） */
  readonly entries: GlyphIdentityEntry[]

  private constructor(revision: number, text: string, entries: GlyphIdentityEntry[]) {
    this.revision = revision
    this.text = text
    this.entries = entries
  }

  /**
   * 从文本建立初始身份表——每个字符簇分配一个新 id。
   */
  static create(text: string, revision: number): GlyphIdentityTable {
    const ranges = splitClusterRanges(text)
    const entries: GlyphIdentityEntry[] = []
    for (const range of ranges) {
      entries.push({
        utf16Start: range.start,
        utf16End: range.end,
        glyphId: `orig-${revision}-${range.start}`,
      })
    }
    return new GlyphIdentityTable(revision, text, entries)
  }

  /**
   * 按一笔事务的 patch 搬运身份，得到该笔事务结束后的新表。
   *
   * @param patches 该笔事务的 patch（坐标对应当前表文本）
   * @param nextText 该笔事务结束后的正文（必须与重放结果一致，否则返回 null）
   * @param nextRevision 该笔事务结束后的 revision
   * @returns 新表；patch 非法或与 nextText 不一致时返回 null
   */
  applyPatches(
    patches: GlyphCarryPatch[],
    nextText: string,
    nextRevision: number
  ): GlyphIdentityTable | null {
    const ops = toUtf16CarryPatches(this.text, patches)
    if (ops === null) {
      return null
    }

    const nextEntries: GlyphIdentityEntry[] = []
    let cursor = 0
    let outPos = 0

    // 把 [from, to) 之间完整包含的旧条目搬到 outPos 起点。
    // 位移为 0 时直接复用原条目对象（条目按约定不可变）——
    // 打字通常在文末，前缀条目无需重建，避免每次按键重建全表。
    const carry = (from: number, to: number): void => {
      const shift = outPos - from
      for (const entry of this.entries) {
        if (entry.utf16Start < from || entry.utf16End > to) {
          continue
        }
        if (shift === 0) {
          nextEntries.push(entry)
          continue
        }
        nextEntries.push({
          utf16Start: entry.utf16Start + shift,
          utf16End: entry.utf16End + shift,
          glyphId: entry.glyphId,
        })
      }
    }

    for (const op of ops) {
      carry(cursor, op.replaceStartUtf16)
      outPos += op.replaceStartUtf16 - cursor
      // 插入的新文本——按字符簇分配新身份
      const insertedRanges = splitClusterRanges(op.insertedText)
      for (const range of insertedRanges) {
        nextEntries.push({
          utf16Start: outPos + range.start,
          utf16End: outPos + range.end,
          glyphId: `ins-${nextRevision}-${outPos + range.start}`,
        })
      }
      outPos += op.insertedText.length
      cursor = op.replaceEndUtf16
    }
    carry(cursor, this.text.length)
    outPos += this.text.length - cursor

    // 校验：搬运结果必须与事务给出的文本完全一致，且条目首尾相接覆盖全文。
    if (outPos !== nextText.length) {
      return null
    }
    if (!GlyphIdentityTable.entriesTileText(nextEntries, nextText.length)) {
      return null
    }
    return new GlyphIdentityTable(nextRevision, nextText, nextEntries)
  }

  /**
   * Issue #879 复核评论问题4：返回 [utf16Start, utf16End) 范围内所有字符簇的身份条目列表。
   *
   * 一个 Planner run（InsertRun/DeletedRun/RetainedMove）可能覆盖多个字符簇，
   * 仅取起始字符簇的身份无法在折行变化/运动区间拆分合并后正确接续。
   * 本方法返回该范围内所有字符簇的 GlyphIdentityEntry，供 Planner 为每个 run
   * 构建 glyphIds 列表和独立 windowId。
   *
   * @param utf16Start 起始 UTF-16 offset（inclusive）
   * @param utf16End 结束 UTF-16 offset（exclusive）
   * @returns 范围内所有字符簇的身份条目列表（按 utf16Start 升序）；范围无效时返回空数组
   */
  idsForRange(utf16Start: number, utf16End: number): GlyphIdentityEntry[] {
    if (utf16Start >= utf16End) {
      return []
    }
    const result: GlyphIdentityEntry[] = []
    for (const entry of this.entries) {
      // 条目完全在范围内
      if (entry.utf16Start >= utf16Start && entry.utf16End <= utf16End) {
        result.push(entry)
      }
      // 条目部分重叠——裁切后纳入
      if (entry.utf16Start < utf16Start && entry.utf16End > utf16Start) {
        result.push({
          utf16Start: utf16Start,
          utf16End: Math.min(entry.utf16End, utf16End),
          glyphId: entry.glyphId,
        })
      }
      // 已越过范围
      if (entry.utf16Start >= utf16End) {
        break
      }
    }
    return result
  }

  /**
   * 查询 utf16Start 所在字符簇的稳定身份。
   *
   * 窗口身份取「起始字符簇」——一个窗口被折行拆成多段时，每段的起始字符簇不同，
   * 因此拆分段各自拥有独立身份，不会互相误配。
   */
  glyphIdAt(utf16Start: number): string | null {
    let low = 0
    let high = this.entries.length - 1
    while (low <= high) {
      const mid = (low + high) >> 1
      const entry = this.entries[mid]
      if (utf16Start < entry.utf16Start) {
        high = mid - 1
      } else if (utf16Start >= entry.utf16End) {
        low = mid + 1
      } else {
        return entry.glyphId
      }
    }
    return null
  }

  /** 条目数（诊断/测试用）。 */
  entryCount(): number {
    return this.entries.length
  }

  /**
   * 同一份身份、换一个 revision 标签。
   *
   * 用于「正文没变但 Core 修订号前进」的更新：身份必须原样延续，
   * 否则下一次编辑会因为 revision 对不上而重建整张表，丢掉全部跨修订身份。
   */
  withRevision(revision: number): GlyphIdentityTable {
    if (revision === this.revision) {
      return this
    }
    return new GlyphIdentityTable(revision, this.text, this.entries)
  }

  /** 条目是否首尾相接覆盖 [0, textLength)。 */
  private static entriesTileText(entries: GlyphIdentityEntry[], textLength: number): boolean {
    let expected = 0
    for (const entry of entries) {
      if (entry.utf16Start !== expected || entry.utf16End <= entry.utf16Start) {
        return false
      }
      expected = entry.utf16End
    }
    return expected === textLength
  }
}
