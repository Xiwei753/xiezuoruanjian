// editor_layout_math.ts — 编辑器折行与命中测试的纯数学模块。
//
// 不依赖 ArkUI / EditorLayoutSnapshot，只依赖 string/number/Array。
// 生产由 EditorRenderBackend.hitTestByPoint 调用：
//   - measureTextFn 注入 @ohos.measure 的真实文本测量（返回 px 单行宽度）；
//   - containerWidth 注入 onAreaChange 的实际容器宽度（px）；
//   - lineSpacingPx 注入 onAreaChange 实际渲染高度 / lines.length（px）。
// 测试由 Node 单测（editor_layout_math.test.mjs）注入确定性 mock measureFn，
// 验证折行/命中的数学性质（多行、自动折行、Unicode 坐标命中、surrogate pair 不切断）。
//
// 所有 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。
// 折行按 UTF-16 code point 推进，不切断 surrogate pair（emoji 等 U+10000+ 字符）。
// 与 SujianEditor Text 组件的 WordBreak.BREAK_ALL（任意 code point 可断行）一致。

/** 一行的 UTF-16 code unit 半开区间 [start, end)。 */
export interface LineRange {
  readonly start: number
  readonly end: number
}

/**
 * 返回 code unit index i 后的下一个 code point 边界。
 * 若 i 处是 high surrogate (0xD800-0xDBFF) 且 i+1 是 low surrogate (0xDC00-0xDFFF)，
 * 返回 i+2（跳过代理对）；否则返回 i+1。i >= text.length 时返回 text.length。
 */
export function nextCodePointBoundary(text: string, i: number): number {
  const n = text.length
  if (i >= n) {
    return n
  }
  if (i < 0) {
    i = 0
  }
  const code = text.charCodeAt(i)
  if (code >= 0xD800 && code <= 0xDBFF && i + 1 < n) {
    const low = text.charCodeAt(i + 1)
    if (low >= 0xDC00 && low <= 0xDFFF) {
      return i + 2
    }
  }
  return i + 1
}

/**
 * 折行：按 UTF-16 code point 推进，对每行二分找最大 end 使
 * measureTextFn(text.substring(start, end)) <= containerWidth。
 *
 * 性质：
 * - 不切断 surrogate pair：每行 end 总落在 code point 边界。
 * - 单个 code point 宽度超过 containerWidth 时独占一行（至少推进一个 code point）。
 * - containerWidth <= 0 时每行一个 code point（退化但确定）。
 * - 空文本返回 []。
 * - 行区间首尾相接：lines[k].end === lines[k+1].start，lines[0].start === 0，
 *   lines[last].end === text.length。
 *
 * measureTextFn 必须满足：measureTextFn('') === 0，且子串宽度随长度单调不减
 * （用于二分正确性）。生产用 @ohos.measure.measureText 满足此性质。
 */
export function layoutLines(
  text: string,
  containerWidth: number,
  measureTextFn: (s: string) => number,
): LineRange[] {
  const n = text.length
  if (n === 0) {
    return []
  }
  // 预计算所有 code point 边界（code unit index），二分在这些边界上做。
  const bounds: number[] = [0]
  let i = 0
  while (i < n) {
    i = nextCodePointBoundary(text, i)
    bounds.push(i)
  }
  // bounds = [0, b1, b2, ..., n]，共 (codePointCount + 1) 个元素。
  const lines: LineRange[] = []
  let startIdx = 0
  const lastIdx = bounds.length - 1
  while (startIdx < lastIdx) {
    const lineStart = bounds[startIdx]
    // 二分找最大 endIdx (startIdx < endIdx <= lastIdx) 使
    // measureTextFn(text.substring(lineStart, bounds[endIdx])) <= containerWidth。
    // 至少放一个 code point：bestIdx = startIdx + 1（即使宽度超 containerWidth 也强制放）。
    let lo = startIdx + 1
    let hi = lastIdx
    let bestIdx = startIdx + 1
    while (lo <= hi) {
      const mid = Math.floor((lo + hi) / 2)
      const w = measureTextFn(text.substring(lineStart, bounds[mid]))
      if (w <= containerWidth) {
        bestIdx = mid
        lo = mid + 1
      } else {
        hi = mid - 1
      }
    }
    lines.push({ start: lineStart, end: bounds[bestIdx] })
    startIdx = bestIdx
  }
  return lines
}

/**
 * 命中测试：根据 touchY 算行号，在该行内根据 touchX 找最接近的 code point 边界。
 *
 * 性质：
 * - lines 为空返回 0。
 * - touchY < 0 clamp 到第 0 行；touchY 超过最后一行 clamp 到最后一行。
 * - touchX < 0 clamp 到行首；touchX 超过行宽 clamp 到行尾。
 * - 返回 UTF-16 code unit offset，总落在 code point 边界（不切断 surrogate pair）。
 * - lineSpacingPx <= 0 时按第 0 行处理（仅 x 方向命中）。
 *
 * lineSpacingPx 是每行实际高度（px），由调用方用实际渲染高度 / lines.length 算出，
 * 不依赖字体 density，保证与 Text 组件实际渲染行高一致。
 */
export function hitTestPoint(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  touchX: number,
  touchY: number,
  measureTextFn: (s: string) => number,
): number {
  if (lines.length === 0) {
    return 0
  }
  // 行号
  let lineIndex: number
  if (lineSpacingPx > 0) {
    lineIndex = Math.floor(touchY / lineSpacingPx)
  } else {
    lineIndex = 0
  }
  if (lineIndex < 0) {
    lineIndex = 0
  }
  if (lineIndex > lines.length - 1) {
    lineIndex = lines.length - 1
  }
  const line = lines[lineIndex]
  if (line.end <= line.start) {
    return line.start
  }
  // 预计算该行 code point 边界
  const bounds: number[] = [line.start]
  let i = line.start
  while (i < line.end) {
    const next = nextCodePointBoundary(text, i)
    const clamped = next > line.end ? line.end : next
    bounds.push(clamped)
    i = clamped
  }
  // bounds = [line.start, ..., line.end]，该行内 code point 边界。
  // 二分找最大 idx 使 measureTextFn(text.substring(line.start, bounds[idx])) <= touchX。
  let lo = 1
  let hi = bounds.length - 1
  let bestIdx = 0
  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2)
    const w = measureTextFn(text.substring(line.start, bounds[mid]))
    if (w <= touchX) {
      bestIdx = mid
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  // bestIdx 是使左边界宽度 <= touchX 的最大边界。
  // 候选：bounds[bestIdx]（左边界，宽度 <= touchX）和 bounds[bestIdx+1]（右边界，宽度 > touchX 或行尾）。
  // 选离 touchX 更近的边界。
  if (bestIdx + 1 < bounds.length) {
    const leftOffset = bounds[bestIdx]
    const rightOffset = bounds[bestIdx + 1]
    const leftW = bestIdx === 0 ? 0 : measureTextFn(text.substring(line.start, leftOffset))
    const rightW = measureTextFn(text.substring(line.start, rightOffset))
    if (touchX - leftW <= rightW - touchX) {
      return leftOffset
    }
    return rightOffset
  }
  return bounds[bestIdx]
}

// Issue #629 评论17 第4项：caret stop 生成下沉到 editor_layout_math.ts。
// 与 hitTestPoint 共享同一套 code point 边界算法，鼠标 hit-test 和键盘垂直导航
// 吃同一份算法，不两套折行/测宽。
// 每个 code point 边界对应一个 { utf16Offset, x } 停靠点，
// 供 LineNavigationResolver.getCaretX/getNearestOffsetAtX 消费。

/** 行内光标停靠点：code point 边界 + 对应 x 坐标（px）。 */
export interface CaretStop {
  readonly utf16Offset: number
  readonly x: number
}

/**
 * 给定一行文本和 measureTextFn，生成该行所有 code point 边界的 caret stops。
 * 每个 stop 包含 code point 边界的 UTF-16 offset 和从行首到该 offset 的测量宽度（px）。
 * 不切断 surrogate pair（emoji 等 U+10000+ 字符的两个 code unit 算一个 stop）。
 */
export function buildLineCaretStops(
  text: string,
  line: LineRange,
  measureTextFn: (s: string) => number,
): CaretStop[] {
  const stops: CaretStop[] = []
  let i = line.start
  while (i <= line.end) {
    const x = measureTextFn(text.substring(line.start, i))
    stops.push({ utf16Offset: i, x: x })
    if (i >= line.end) {
      break
    }
    // 下一个 code point 边界（不切断 surrogate pair）
    i = nextCodePointBoundary(text, i)
  }
  return stops
}
