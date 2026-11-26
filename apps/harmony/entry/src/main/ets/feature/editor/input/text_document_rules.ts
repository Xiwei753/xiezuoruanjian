// text_document_rules.ts — 磁盘/平台原文的段落解释（UTF-16 绝对索引）。
// 纯 TS、不依赖 ArkUI，可被 Node 单测直接 import。
//
// 本文件只做**磁盘/平台原文的段落解释**，是 Core
// `core/writer_core/src/editor/text_document_rules.rs` 中 `paragraph_byte_ranges`
// 的 UTF-16 索引镜像：
//   - 分隔符：`\n`（1 个 UTF-16 单元）、`\r\n`（2 个单元，算**一个**段落边界）、
//     `\u2029`（1 个单元）；孤立 `\r` **不是**分隔符。
//   - 段落 range 的 end 不含分隔符（CRLF 两个字符都不属于前一段内容）；
//     连续分隔符产生空段落；末尾分隔符产生末尾空段落；空正文返回空数组。
//
// 导入/粘贴写入正文前的换行与控制字符规范化由 Core `text_document_rules.rs`
// （normalize_inserted_text）与 Qt 入口负责，本文件不复制 normalize 逻辑，
// 也不改写正文；这里只读原文、只返回段落坐标。

// 单个段落在正文中的范围（UTF-16，end 不含分隔符）。
export interface ParagraphRange {
  start: number
  end: number
  hasTrailingNewline: boolean
}

// index 处段落分隔符的 UTF-16 长度；不是分隔符时返回 0。
// 先识别 `\r\n` 再识别 `\r`：孤立 `\r` 长度为 0（正文内容，不是段落边界）。
function separatorLengthAt(text: string, index: number): number {
  const code: number = text.charCodeAt(index)
  if (code === 10) {
    // '\n'
    return 1
  }
  if (code === 13) {
    // '\r'：只有紧跟 '\n' 才构成 CRLF 段落边界。
    if (index + 1 < text.length && text.charCodeAt(index + 1) === 10) {
      return 2
    }
    return 0
  }
  if (code === 0x2029) {
    // U+2029 PARAGRAPH SEPARATOR
    return 1
  }
  return 0
}

// 按统一段落规则切分正文，返回每个段落的 UTF-16 半开 range（不含分隔符）。
// 单次线性扫描 O(n)；空正文返回空数组。
export function listParagraphRanges(text: string): ParagraphRange[] {
  const ranges: ParagraphRange[] = []
  if (text.length === 0) {
    return ranges
  }
  let start: number = 0
  let index: number = 0
  while (index < text.length) {
    const separatorLength: number = separatorLengthAt(text, index)
    if (separatorLength > 0) {
      const range: ParagraphRange = {
        start: start,
        end: index,
        hasTrailingNewline: true
      }
      ranges.push(range)
      index += separatorLength
      start = index
    } else {
      index += 1
    }
  }
  const last: ParagraphRange = {
    start: start,
    end: text.length,
    hasTrailingNewline: false
  }
  ranges.push(last)
  return ranges
}

// insertedText 内每个分隔符之后的绝对 UTF-16 起点（CRLF 只压一次，取分隔符结束处）。
// 语义与旧实现“每个 '\n' 后的绝对位置”一致，但按统一段落规则识别 CRLF 与 U+2029。
// 只按插入文本判断，无法表达“跨编辑边界形成/拆开 CRLF”；通知路径改用
// describeLocalParagraphChange（本文件下方），这里保留给只需要插入段起点的只读场景。
export function insertedParagraphStarts(insertedText: string, insertionStartUtf16: number): number[] {
  const starts: number[] = []
  let index: number = 0
  while (index < insertedText.length) {
    const separatorLength: number = separatorLengthAt(insertedText, index)
    if (separatorLength > 0) {
      starts.push(insertionStartUtf16 + index + separatorLength)
      index += separatorLength
    } else {
      index += 1
    }
  }
  return starts
}

// ── Issue #1063 复审评论第 3 条：局部段落变化描述 ─────────────────────
//
// 原生编辑器只把「本次替换范围」交给桥接，因此段落结构变化必须能在**局部**判定：
//   - 只取编辑点前后各约 2 个 UTF-16 单元的窗口，按 code point / CRLF 边界钳位，
//     不整章重建，也不把代理对或 CRLF 拆到窗口外；
//   - 用 before 局部文本 + insertedText 构造 after 局部文本，两侧都按统一段落规则扫描；
//   - 跨编辑边界形成/拆开 CRLF 也算段落结构变化（例：`a\nb` 在 1 插入 `\r`、
//     `a\rb` 在 2 插入 `\n`、删掉 `a\r\nb` 的 `\r`），不能只扫描 insertedText。

export interface LocalParagraphChange {
  // 局部段落分隔符结构变化（含 CRLF 形成/拆开、U+2029、LF）。
  paragraphsChanged: boolean
  // 编辑后新增段落的绝对 UTF-16 起点（after 有而映射后的 before 没有）。
  newParagraphStarts: number[]
  // '\n' 字符数变化（供 nativeNewlineCount 增量维护）。
  newlineDelta: number
  // 段落边界数变化（分隔符个数差；CRLF 算一个边界）。
  paragraphDelta: number
}

// 编辑点前后各取多少个 UTF-16 单元做局部窗口。
const LOCAL_CONTEXT_UTF16: number = 2

interface LocalSeparatorFact {
  // 分隔符起点的绝对 UTF-16 位置（before 或 after 坐标）。
  at: number
  // 分隔符长度：'\n' = 1、CRLF = 2、U+2029 = 1。
  length: number
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xD800 && code <= 0xDBFF
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xDC00 && code <= 0xDFFF
}

// 窗口起点：不落在代理对中间，也不把 CRLF 的 '\r' 留在窗口外。
function clampWindowStart(text: string, index: number): number {
  let start: number = Math.max(0, Math.min(index, text.length))
  if (start > 0 && start < text.length &&
      isLowSurrogate(text.charCodeAt(start)) && isHighSurrogate(text.charCodeAt(start - 1))) {
    start -= 1
  }
  if (start > 0 && start < text.length &&
      text.charCodeAt(start) === 10 && text.charCodeAt(start - 1) === 13) {
    start -= 1
  }
  return start
}

// 窗口终点（不含）：不落在代理对中间，也不把 CRLF 的 '\n' 留在窗口外。
function clampWindowEnd(text: string, index: number): number {
  let end: number = Math.max(0, Math.min(index, text.length))
  if (end > 0 && end < text.length &&
      isLowSurrogate(text.charCodeAt(end)) && isHighSurrogate(text.charCodeAt(end - 1))) {
    end += 1
  }
  if (end > 0 && end < text.length &&
      text.charCodeAt(end - 1) === 13 && text.charCodeAt(end) === 10) {
    end += 1
  }
  return end
}

// 扫描局部文本里的全部段落分隔符，返回绝对坐标。
function scanLocalSeparators(text: string, absoluteBase: number): LocalSeparatorFact[] {
  const separators: LocalSeparatorFact[] = []
  let index: number = 0
  while (index < text.length) {
    const separatorLength: number = separatorLengthAt(text, index)
    if (separatorLength > 0) {
      separators.push({ at: absoluteBase + index, length: separatorLength })
      index += separatorLength
    } else {
      index += 1
    }
  }
  return separators
}

function countLineFeeds(text: string): number {
  let count: number = 0
  for (let index = 0; index < text.length; index++) {
    if (text.charCodeAt(index) === 10) {
      count += 1
    }
  }
  return count
}

/**
 * 描述一次原生替换（before[startUtf16, endUtf16) → insertedText）造成的局部段落变化。
 *
 * 纯 UTF-16 坐标、只读局部窗口，不整章重建正文；供 NativeRichEditorBridge 的通知路径
 * 维护 nativeNewlineCount 并核对新增段落样式继承。
 */
export function describeLocalParagraphChange(beforeText: string, insertedText: string,
  startUtf16: number, endUtf16: number): LocalParagraphChange {
  const textLength: number = beforeText.length
  const start: number = Math.max(0, Math.min(startUtf16, textLength))
  // endUtf16 < startUtf16 视为纯插入：不在非法范围上抛错，也不改写原文。
  const end: number = Math.max(start, Math.min(endUtf16, textLength))
  const delta: number = insertedText.length - (end - start)

  const windowStart: number = clampWindowStart(beforeText, start - LOCAL_CONTEXT_UTF16)
  const windowEnd: number = clampWindowEnd(beforeText, end + LOCAL_CONTEXT_UTF16)
  const prefix: string = beforeText.substring(windowStart, start)
  const suffix: string = beforeText.substring(end, windowEnd)
  const beforeLocal: string = beforeText.substring(windowStart, windowEnd)
  const afterLocal: string = prefix + insertedText + suffix

  // after 局部文本的绝对起点与 before 相同：prefix 在编辑点之前，长度不变。
  const beforeSeparators: LocalSeparatorFact[] = scanLocalSeparators(beforeLocal, windowStart)
  const afterSeparators: LocalSeparatorFact[] = scanLocalSeparators(afterLocal, windowStart)

  // 把 before 分隔符映射到 after 坐标：完全在编辑点之前不变，完全在删除区间之后整体平移；
  // 其余（与编辑点/删除区间相交）说明这一处段落结构被改写（含 CRLF 形成/拆开）。
  const mappedBeforeKeys: string[] = []
  let separatorTouchedByEdit: boolean = false
  for (const separator of beforeSeparators) {
    const separatorEnd: number = separator.at + separator.length
    if (separatorEnd <= start) {
      mappedBeforeKeys.push(`${separator.at}:${separator.length}`)
    } else if (separator.at >= end) {
      mappedBeforeKeys.push(`${separator.at + delta}:${separator.length}`)
    } else {
      separatorTouchedByEdit = true
    }
  }

  const afterKeys: string[] = []
  const afterStarts: number[] = []
  for (const separator of afterSeparators) {
    afterKeys.push(`${separator.at}:${separator.length}`)
    afterStarts.push(separator.at + separator.length)
  }

  let structureChanged: boolean = separatorTouchedByEdit
  if (!structureChanged) {
    if (mappedBeforeKeys.length !== afterKeys.length) {
      structureChanged = true
    } else {
      for (const key of mappedBeforeKeys) {
        if (afterKeys.indexOf(key) < 0) {
          structureChanged = true
          break
        }
      }
    }
  }

  // before 段落起点映射到 after 坐标：删除区间内的起点丢弃。
  const mappedStarts: number[] = []
  for (const separator of beforeSeparators) {
    const beforeStart: number = separator.at + separator.length
    if (beforeStart <= start) {
      mappedStarts.push(beforeStart)
    } else if (beforeStart >= end) {
      mappedStarts.push(beforeStart + delta)
    }
  }

  const newParagraphStarts: number[] = []
  for (const afterStart of afterStarts) {
    if (mappedStarts.indexOf(afterStart) < 0) {
      newParagraphStarts.push(afterStart)
    }
  }

  const oldText: string = beforeText.substring(start, end)
  return {
    paragraphsChanged: structureChanged,
    newParagraphStarts: newParagraphStarts,
    newlineDelta: countLineFeeds(insertedText) - countLineFeeds(oldText),
    paragraphDelta: afterSeparators.length - beforeSeparators.length
  }
}
