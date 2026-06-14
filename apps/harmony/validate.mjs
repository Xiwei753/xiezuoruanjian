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

function checkFile(path) {
  const src = readFileSync(path, 'utf8')
  const rel = path.replace(import.meta.dirname + '/', '')
  const lines = src.split('\n')

  // 1. Brace balance
  let braces = 0
  for (const ch of src) {
    if (ch === '{') braces++
    if (ch === '}') braces--
    if (braces < 0) { errors.push(`${rel}: unmatched '}'`); break }
  }
  if (braces !== 0) errors.push(`${rel}: unbalanced braces (${braces > 0 ? '+' : ''}${braces})`)

  // Strip comments for code-only checks
  const codeOnly = lines
    .map(l => l.replace(/\/\/.*$/, '').replace(/\/\*.*?\*\//g, ''))
    .join('\n')

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
