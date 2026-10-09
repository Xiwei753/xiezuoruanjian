// editor_glyph_identity.ts — 跨修订稳定的字形身份表。
//
// Issue #879 最新复核评论问题3：动画查找键必须跨修订稳定。
// 旧的 `utf16Start-utf16End-hash(textSlice)` 在字前插入/删除后 offset 平移即失配，
// 相同内容出现在相同位置也可能被误认成同一个字。
//
// 本模块维护一张随每次编辑“搬运”的「字符簇 → glyphId」表：
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
 * Issue #879 复核评论问题4：由一段字符簇身份序列派生「窗口身份」。
 *
 * 字形单元 id（glyphId）是单个字符簇的身份，而动画里的一个裁切窗口往往覆盖
 * 多个字符簇；两者不是同一种对象，不能拿首字符簇身份代表整窗口。
 *
 * 窗口身份 = 完整簇身份序列的**单射编码**：`win-<簇数>-` 后面跟着每个簇的
 * `<长度>:<glyphId>`。任何一个序列都能被唯一地解析回来，因此：
 * - 位置平移不改变窗口身份（id 里不含 utf16 区间，也不含窗口坐标）；
 * - 折行 / 运动区间重分段导致簇序列变化时窗口身份随之变化；
 * - 不同簇序列（包括 `[A,B,C]` 与 `[A,D,E]` 这种首簇相同、数量相同的序列）
 *   一定得到不同 windowId——不用 hash，不做「首簇 + 数量」近似，无碰撞。
 *
 * 长度前缀编码对分隔符没有假设：即使 glyphId 里出现 `:` 或 `-`，
 * 也能按长度无歧义地解析（旧实现 `win-${glyphIds[0]}x${glyphIds.length}`
 * 会碰撞，`join('-')` + hash 也只是把碰撞概率缩小而不是消除）。
 *
 * 只用字母数字、`-` 和 `:`，保证可直接用作 ArkUI 组件 id 与 ForEach key。
 * 调用方必须保证 glyphIds 非空（身份表兜底会让空序列拿到内容 hash id）。
 */
export function windowIdForGlyphIds(glyphIds: string[]): string {
  if (glyphIds.length === 0) {
    // 空序列在生产路径不可达：身份表不可用时 Planner 会退回内容 hash 的单元素列表。
    // 这里只给一个固定哨兵，不会被当成真实窗口节点身份使用。
    return 'win-empty'
  }
  let encoded = `win-${glyphIds.length}-`
  for (let i = 0; i < glyphIds.length; i++) {
    const id = glyphIds[i]
    encoded += `${id.length}:${id}`
  }
  return encoded
}

/**
 * Issue #879 复核评论6075662695问题3：把 [utf16Start, utf16End) 切成字符簇边界。
 *
 * 返回簇边界 offset 列表，长度 = 簇数 + 1：第 i 个簇占 [result[i], result[i+1])。
 * 与身份表分簇规则完全一致（splitClusterRanges），
 * 因此 run 携带的 glyphIds[i] 与这里的第 i 个簇一一对应——
 * 提交瞬间按字形算局部几何（可见宽度/锚点）时不会错位。
 */
export function clusterBoundaries(text: string, utf16Start: number, utf16End: number): number[] {
  if (utf16Start >= utf16End || utf16Start < 0 || utf16End > text.length) {
    return []
  }
  const ranges = splitClusterRanges(text.substring(utf16Start, utf16End))
  const boundaries: number[] = []
  for (const range of ranges) {
    boundaries.push(utf16Start + range.start)
  }
  boundaries.push(utf16End)
  return boundaries
}

/**
 * Issue #879 复核评论6077187962 问题4：窗口实例 ID 分配器。
 *
 * `windowIdForGlyphIds()` 的输出长度随 glyphIds 数量线性增长，不适合直接用作
 * ArkUI ForEach key / 组件 .id() / ComponentObserver 索引——长 key 影响渲染性能，
 * 也让调试日志难以阅读。
 *
 * 本分配器把完整身份序列留在算法内部（`windowIdForGlyphIds()` 不变），
 * 为 ArkUI 节点分配独立的有限长度 `win-<number>` 格式 ID。
 *
 * - `allocate(glyphIds)` —— 给定 glyphIds，返回对应的 `win-<number>`（已存在则复用）
 * - `resolve(windowInstanceId)` —— 给定 instanceId，返回对应的 glyphIds（调试/日志用）
 * - `release(windowInstanceId)` —— 释放一个 instanceId（窗口不再需要时）
 * - `releaseByWindowId(windowId)` —— 通过算法身份释放
 *
 * 释放后的 instanceId 可以被重新分配（但同一时间不能有两个相同 instanceId 的活跃窗口）。
 *
 * 纯逻辑：不依赖 ArkUI，可被 Node 单测直接 import。
 */
export class WindowInstanceIdAllocator {
  /** 下一个待分配的编号 */
  private nextId: number = 0
  /** windowIdForGlyphIds() 结果 → windowInstanceId 的映射 */
  private windowIdToInstanceId: Map<string, string> = new Map()
  /** windowInstanceId → windowIdForGlyphIds() 结果 的反向映射 */
  private instanceIdToWindowId: Map<string, string> = new Map()
  /** windowInstanceId → glyphIds 的反向映射（调试/日志用） */
  private instanceIdToGlyphIds: Map<string, string[]> = new Map()
  /** 已释放的 instanceId 列表——可复用以避免编号无限增长 */
  private releasedIds: string[] = []
  /**
   * Issue #879 复核评论6078682695 问题3：按 renderNodeKey 管理的引用计数。
   *
   * 同一 glyphIds 序列可以被上一帧 `new-r11` 与候选 `old-r11` 两个不同 Text 节点
   * 同时使用，共享同一 instanceId。任一节点 onDisAppear 不应直接释放整段 glyph 序列——
   * 只有最后一个 lease 被归还才真正释放 instanceId。
   *
   * Map<instanceId, Set<renderNodeKey>>：记录每个 instanceId 被哪些 renderNodeKey 引用。
   */
  private instanceIdToLeases: Map<string, Set<string>> = new Map()

  /**
   * 给定 glyphIds，返回对应的 `win-<number>` 格式 instanceId。
   *
   * 已存在（同一 glyphIds 序列）则复用，不存在则新建。
   * 释放后的 instanceId 可被重新分配给不同的 glyphIds。
   *
   * @param glyphIds 字形身份列表（非空）
   * @returns `win-<number>` 格式的有限长度 ID
   */
  allocate(glyphIds: string[]): string {
    const windowId = windowIdForGlyphIds(glyphIds)
    const existing = this.windowIdToInstanceId.get(windowId)
    if (existing !== undefined) {
      return existing
    }
    // 优先复用已释放的 ID，避免编号无限增长
    let instanceId: string
    if (this.releasedIds.length > 0) {
      instanceId = this.releasedIds.pop()!
    } else {
      instanceId = `win-${this.nextId++}`
    }
    this.windowIdToInstanceId.set(windowId, instanceId)
    this.instanceIdToWindowId.set(instanceId, windowId)
    this.instanceIdToGlyphIds.set(instanceId, glyphIds)
    return instanceId
  }

  /**
   * 给定 instanceId，返回对应的 glyphIds（调试/日志用）。
   *
   * @param windowInstanceId `win-<number>` 格式 ID
   * @returns 对应的 glyphIds，或 null（不存在或已释放）
   */
  resolve(windowInstanceId: string): string[] | null {
    const glyphIds = this.instanceIdToGlyphIds.get(windowInstanceId)
    if (glyphIds !== undefined) {
      return glyphIds
    }
    return null
  }

  /**
   * 释放一个 instanceId（窗口不再需要时）。
   *
   * 释放后该 instanceId 可被重新分配给不同的 glyphIds。
   * 同一时间不会有两个相同 instanceId 的活跃窗口。
   *
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  release(windowInstanceId: string): void {
    const windowId = this.instanceIdToWindowId.get(windowInstanceId)
    if (windowId !== undefined) {
      this.windowIdToInstanceId.delete(windowId)
      this.instanceIdToWindowId.delete(windowInstanceId)
      this.instanceIdToGlyphIds.delete(windowInstanceId)
      this.releasedIds.push(windowInstanceId)
    }
  }

  /**
   * Issue #879 复核评论6078682695 问题3：为 renderNodeKey 添加对 instanceId 的租约。
   *
   * 同一 glyphIds 序列可以被多个不同 Text 节点同时使用，共享同一 instanceId。
   * 每个节点持有一个 lease，只有所有 lease 都被归还才真正释放 instanceId。
   *
   * 使用 Set 确保幂等：同一 renderNodeKey 多次调用不会创建重复 lease。
   *
   * @param renderNodeKey 渲染节点身份标识
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  addLease(renderNodeKey: string, windowInstanceId: string): void {
    let leases = this.instanceIdToLeases.get(windowInstanceId)
    if (leases === undefined) {
      leases = new Set<string>()
      this.instanceIdToLeases.set(windowInstanceId, leases)
    }
    leases.add(renderNodeKey)
  }

  /**
   * Issue #879 复核评论6078682695 问题3：释放 renderNodeKey 对 instanceId 的租约。
   *
   * Text.onDisAppear 时调用——只释放该 renderNodeKey 持有的 lease，
   * 同一 glyphIds 序列同时供多个节点使用时，只有最后一个 lease 被归还才真正释放 instanceId。
   *
   * @param renderNodeKey 渲染节点身份标识
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  releaseLease(renderNodeKey: string, windowInstanceId: string): void {
    const leases = this.instanceIdToLeases.get(windowInstanceId)
    if (leases === undefined) {
      // 没有租约记录——直接释放（兼容旧路径）
      this.release(windowInstanceId)
      return
    }
    leases.delete(renderNodeKey)
    if (leases.size === 0) {
      // 所有租约已归还——真正释放 instanceId
      this.instanceIdToLeases.delete(windowInstanceId)
      this.release(windowInstanceId)
    }
  }

  /**
   * 通过算法身份（windowIdForGlyphIds() 的结果）释放。
   *
   * @param windowId `windowIdForGlyphIds()` 的输出
   */
  releaseByWindowId(windowId: string): void {
    const instanceId = this.windowIdToInstanceId.get(windowId)
    if (instanceId !== undefined) {
      this.release(instanceId)
    }
  }

  /**
   * 清空所有映射和已释放 ID——编辑器重置/会话切换时调用。
   */
  clear(): void {
    this.nextId = 0
    this.windowIdToInstanceId.clear()
    this.instanceIdToWindowId.clear()
    this.instanceIdToGlyphIds.clear()
    this.releasedIds = []
    this.instanceIdToLeases.clear()
  }
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
  /** glyphId → 条目 的惰性索引（表不可变，首次查询时建立） */
  private glyphIdIndex: Map<string, GlyphIdentityEntry> | null = null

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
   * Issue #879 复核评论6075662695问题3：按 glyphId 反查它在**本表正文**中的位置。
   *
   * 提交瞬间要判断「某个字形此刻在屏上处于哪一段可见区」，
   * 需要把 run 携带的 glyphId 映射回在屏正文的 UTF-16 区间——
   * 表里没有这个 id 说明这个字不在本表描述的正文里（属于真正的新字，尚未上屏）。
   *
   * 用 glypId → 条目的索引缓存（表不可变，缓存只在首次查询时建一次）。
   * 同一个 id 在表里只出现一次（条目首尾相接、身份唯一）。
   */
  entryByGlyphId(glyphId: string): GlyphIdentityEntry | null {
    if (this.glyphIdIndex === null) {
      const index: Map<string, GlyphIdentityEntry> = new Map()
      for (const entry of this.entries) {
        if (!index.has(entry.glyphId)) {
          index.set(entry.glyphId, entry)
        }
      }
      this.glyphIdIndex = index
    }
    return this.glyphIdIndex.get(glyphId) ?? null
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
