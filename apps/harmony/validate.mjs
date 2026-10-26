#!/usr/bin/env node
// Minimal .ets syntax validator — checks brace balance, import refs, removed types.
import { readFileSync, readdirSync, statSync } from 'fs'
import { join, extname } from 'path'

const ROOT = join(import.meta.dirname, 'entry/src/main/ets')
const errors = []
const REGEX_PREFIX_KEYWORDS = new Set([
  'await', 'case', 'delete', 'do', 'else', 'extends', 'in', 'instanceof', 'new', 'of',
  'return', 'throw', 'typeof', 'void', 'yield', 'default'
])

function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (extname(p) === '.ets') checkFile(p)
  }
}

// Strip comments, string literals, and regular-expression literals so checks only see code.
// 花括号/标识符出现在注释和字符串里时不是代码：例如注释里的裸 '{'、
// startsWith('{')、以及 HiLog 占位符 '%{public}s'。
// 模板字面量只保留 ${} 插值里的代码，插值照常参与花括号配平和类型检查。
function stripNonCode(src) {
  let out = ''
  // null | 'line-comment' | 'block-comment' | 'single' | 'double' | 'template' | 'regex'
  let state = null
  let regexInClass = false
  let regexBuffer = ''
  let canStartRegex = true
  const interpDepths = [] // ${} 插值栈：记录当前插值内的花括号深度
  let i = 0
  while (i < src.length) {
    const ch = src[i]
    const next = src[i + 1]
    if (state === null) {
      if (ch === '/' && next === '/') { state = 'line-comment'; i += 2; continue }
      if (ch === '/' && next === '*') { state = 'block-comment'; i += 2; continue }
      if (ch === '/' && next === '=') { out += '/='; canStartRegex = true; i += 2; continue }
      if (ch === '/') {
        if (canStartRegex) {
          state = 'regex'
          regexInClass = false
          regexBuffer = '/'
          i += 1
          continue
        }
        out += ch
        canStartRegex = true
        i += 1
        continue
      }
      if (ch === "'") { state = 'single'; canStartRegex = false; i += 1; continue }
      if (ch === '"') { state = 'double'; canStartRegex = false; i += 1; continue }
      if (ch === '`') { state = 'template'; canStartRegex = false; i += 1; continue }
      if (interpDepths.length > 0) {
        if (ch === '{') {
          interpDepths[interpDepths.length - 1] += 1
          out += ch
          canStartRegex = true
          i += 1
          continue
        }
        if (ch === '}') {
          if (interpDepths[interpDepths.length - 1] === 0) {
            interpDepths.pop()
            state = 'template' // 插值结束，回到模板字面量
            canStartRegex = false
            i += 1
            continue
          }
          interpDepths[interpDepths.length - 1] -= 1
          out += ch
          canStartRegex = false
          i += 1
          continue
        }
      }
      if (/[A-Za-z_$]/.test(ch)) {
        const start = i
        i += 1
        while (/[A-Za-z0-9_$]/.test(src[i] ?? '')) i += 1
        const word = src.slice(start, i)
        out += word
        canStartRegex = REGEX_PREFIX_KEYWORDS.has(word)
        continue
      }
      if (/[0-9]/.test(ch)) {
        const start = i
        i += 1
        while (/[A-Za-z0-9_$.]/.test(src[i] ?? '')) i += 1
        out += src.slice(start, i)
        canStartRegex = false
        continue
      }
      if (/\s/.test(ch)) { out += ch; i += 1; continue }

      const pair = src.slice(i, i + 2)
      if (pair === '=>' || pair === '&&' || pair === '||' || pair === '??' ||
        pair === '==' || pair === '!=' || pair === '<=' || pair === '>=' ||
        pair === '**' || pair === '<<' || pair === '>>') {
        out += pair
        canStartRegex = true
        i += 2
        continue
      }
      if (pair === '++' || pair === '--') {
        out += pair
        i += 2
        continue
      }

      out += ch
      if (ch === '(' || ch === '[' || ch === '{' || ch === ',' || ch === ';' ||
        ch === ':' || ch === '?' || ch === '=' || ch === '!' || ch === '~' ||
        ch === '+' || ch === '-' || ch === '*' || ch === '%' || ch === '^' ||
        ch === '&' || ch === '|' || ch === '<' || ch === '>') {
        canStartRegex = true
      } else if (ch === ')' || ch === ']' || ch === '}' || ch === '.') {
        canStartRegex = false
      }
      i += 1
      continue
    }
    if (state === 'line-comment') {
      if (ch === '\n') { state = null; out += ch }
      i += 1
      continue
    }
    if (state === 'block-comment') {
      if (ch === '*' && next === '/') { state = null; i += 2; continue }
      i += 1
      continue
    }
    if (state === 'template') {
      if (ch === '\\') { i += 2; continue }
      if (ch === '`') { state = null; canStartRegex = false; i += 1; continue }
      if (ch === '$' && next === '{') {
        interpDepths.push(0)
        state = null
        canStartRegex = true
        i += 2
        continue
      }
      i += 1
      continue
    }
    if (state === 'regex') {
      if (ch === '\n' || ch === '\r') {
        // A regexp literal cannot cross a line terminator. Preserve malformed text so
        // a missing brace inside it cannot disappear from the structural check.
        out += regexBuffer + ch
        regexBuffer = ''
        state = null
        canStartRegex = false
        i += 1
        continue
      }
      if (ch === '\\') {
        if (next === '\n' || next === '\r') {
          out += regexBuffer + ch + next
          regexBuffer = ''
          state = null
          canStartRegex = false
          i += 2
          continue
        }
        regexBuffer += ch + (next ?? '')
        i += next === undefined ? 1 : 2
        continue
      }
      if (regexInClass) {
        regexBuffer += ch
        if (ch === ']') regexInClass = false
        i += 1
        continue
      }
      if (ch === '[') {
        regexInClass = true
        regexBuffer += ch
        i += 1
        continue
      }
      if (ch === '/') {
        state = null
        regexBuffer = ''
        i += 1
        while (/[A-Za-z]/.test(src[i] ?? '')) i += 1 // consume regexp flags
        canStartRegex = false
        continue
      }
      regexBuffer += ch
      i += 1
      continue
    }
    // 单/双引号字符串内部：跳过转义和结束引号，内容一律不计入代码。
    if (ch === '\\') { i += 2; continue }
    if ((state === 'single' && ch === "'") || (state === 'double' && ch === '"')) {
      state = null
      canStartRegex = false
    }
    i += 1
  }
  if (state === 'regex') out += regexBuffer // Keep unterminated regex text visible to brace checks.
  return out
}

function inspectBraceBalance(codeOnly) {
  let braces = 0
  let unmatched = false
  for (const ch of codeOnly) {
    if (ch === '{') braces++
    if (ch === '}') braces--
    if (braces < 0) { unmatched = true; break }
  }
  return { braces, unmatched }
}

function assertBraceFixture(label, src, expectedBraces, expectedUnmatched = false) {
  const codeOnly = stripNonCode(src)
  const result = inspectBraceBalance(codeOnly)
  if (result.braces !== expectedBraces || result.unmatched !== expectedUnmatched) {
    throw new Error(`stripNonCode regression in ${label}: got ${JSON.stringify(result)}`)
  }
  return codeOnly
}

function validateStripNonCode() {
  const archive = readFileSync(join(ROOT, 'platform/diagnostics/HarmonyDiagnosticsArchive.ets'), 'utf8')
  assertBraceFixture('diagnostics archive regexp literals', archive, 0)

  const expressionCode = assertBraceFixture(
    'division followed by regexp with braces and quotes',
    String.raw`const quotient = top / bottom; const pattern = /[{}"']/;`,
    0
  )
  if (!expressionCode.includes('top / bottom; const pattern =')) {
    throw new Error('stripNonCode regression: division expression was mistaken for a regexp')
  }

  assertBraceFixture(
    'template interpolation',
    'const output = `literal { } ${items.map((item) => ({ value: item }))}`;',
    0
  )
  assertBraceFixture('missing opening brace remains visible', 'function broken() { if (ready) { run(); }', 1)
  assertBraceFixture('unmatched closing brace remains visible', 'function broken() { run(); }}', -1, true)
}

validateStripNonCode()

function checkFile(path) {
  const src = readFileSync(path, 'utf8')
  const rel = path.replace(import.meta.dirname + '/', '')
  const codeOnly = stripNonCode(src)

  // 1. Brace balance (code only — comments and string literals excluded)
  const braceResult = inspectBraceBalance(codeOnly)
  if (braceResult.unmatched) errors.push(`${rel}: unmatched '}'`)
  if (braceResult.braces !== 0) {
    errors.push(`${rel}: unbalanced braces (${braceResult.braces > 0 ? '+' : ''}${braceResult.braces})`)
  }

  // 2. References to removed types (code only, not comments)
  const removed = ['TokenStore', 'EncryptionProvider', 'MockTokenStore', 'NativeTokenStore', 'MockEncryptionProvider']
  for (const t of removed) {
    if (new RegExp(`\\b${t}\\b`).test(codeOnly)) errors.push(`${rel}: references removed type '${t}'`)
  }

  // 3. References to old MockWriterCoreBridge instantiation (skip AppContext — it's the DI root)
  if (!path.includes('AppContext') && /new MockWriterCoreBridge\(\)/.test(codeOnly)) {
    errors.push(`${rel}: still does 'new MockWriterCoreBridge()' — use getBridge()`)
  }

  // 4. References to old network types (code only)
  const oldNet = ['NetworkState', 'ConnectionStatus', 'NetworkType', 'ProxyConfig', 'NetworkDiagnostics']
  for (const t of oldNet) {
    if (new RegExp(`\\b${t}\\b`).test(codeOnly)) errors.push(`${rel}: references removed network type '${t}'`)
  }

  // 5. Check for @speculative count (informational)
  const specCount = (src.match(/@speculative/g) || []).length
  if (specCount > 0 && path.includes('CoreDtos')) {
    console.log(`  ℹ ${rel}: ${specCount} @speculative fields`)
  }
}

walk(ROOT)

if (errors.length > 0) {
  console.error('❌ Validation errors:')
  for (const e of errors) console.error(`  ${e}`)
  process.exit(1)
} else {
  console.log('✅ All .ets files pass basic validation')
}
