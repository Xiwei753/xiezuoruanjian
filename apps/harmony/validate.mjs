#!/usr/bin/env node
// Minimal .ets syntax validator — checks brace balance, import refs, removed types.
import { readFileSync, readdirSync, statSync } from 'fs'
import { join, extname } from 'path'

const ROOT = join(import.meta.dirname, 'entry/src/main/ets')
const errors = []

function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (extname(p) === '.ets') checkFile(p)
  }
}

// Strip comments and string literals so checks only see code.
// 花括号/标识符出现在注释和字符串里时不是代码：例如注释里的裸 '{'、
// startsWith('{')、以及 HiLog 占位符 '%{public}s'。
// 模板字面量只保留 ${} 插值里的代码，插值照常参与花括号配平和类型检查。
function stripNonCode(src) {
  let out = ''
  // null | 'line-comment' | 'block-comment' | 'single' | 'double' | 'template'
  let state = null
  const interpDepths = [] // ${} 插值栈：记录当前插值内的花括号深度
  let i = 0
  while (i < src.length) {
    const ch = src[i]
    const next = src[i + 1]
    if (state === null) {
      if (ch === '/' && next === '/') { state = 'line-comment'; i += 2; continue }
      if (ch === '/' && next === '*') { state = 'block-comment'; i += 2; continue }
      if (ch === "'") { state = 'single'; i += 1; continue }
      if (ch === '"') { state = 'double'; i += 1; continue }
      if (ch === '`') { state = 'template'; i += 1; continue }
      if (interpDepths.length > 0) {
        if (ch === '{') { interpDepths[interpDepths.length - 1] += 1; out += ch; i += 1; continue }
        if (ch === '}') {
          if (interpDepths[interpDepths.length - 1] === 0) {
            interpDepths.pop()
            state = 'template' // 插值结束，回到模板字面量
            i += 1
            continue
          }
          interpDepths[interpDepths.length - 1] -= 1
          out += ch
          i += 1
          continue
        }
      }
      out += ch
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
      if (ch === '`') { state = null; i += 1; continue }
      if (ch === '$' && next === '{') { interpDepths.push(0); state = null; i += 2; continue }
      i += 1
      continue
    }
    // 单/双引号字符串内部：跳过转义和结束引号，内容一律不计入代码。
    if (ch === '\\') { i += 2; continue }
    if ((state === 'single' && ch === "'") || (state === 'double' && ch === '"')) state = null
    i += 1
  }
  return out
}

function checkFile(path) {
  const src = readFileSync(path, 'utf8')
  const rel = path.replace(import.meta.dirname + '/', '')
  const codeOnly = stripNonCode(src)

  // 1. Brace balance (code only — comments and string literals excluded)
  let braces = 0
  for (const ch of codeOnly) {
    if (ch === '{') braces++
    if (ch === '}') braces--
    if (braces < 0) { errors.push(`${rel}: unmatched '}'`); break }
  }
  if (braces !== 0) errors.push(`${rel}: unbalanced braces (${braces > 0 ? '+' : ''}${braces})`)

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
