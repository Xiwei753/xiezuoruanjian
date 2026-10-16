#!/usr/bin/env node
// Minimal .ets syntax validator — checks brace balance, import refs, removed types.
import { readFileSync, readdirSync, statSync, existsSync } from 'fs'
import { join, extname } from 'path'

const ROOT = join(import.meta.dirname, 'entry/src/main/ets')
const errors = []

function walkEts(dir, fn) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walkEts(p, fn)
    else if (extname(p) === '.ets') fn(p)
  }
}

function walk(dir) {
  walkEts(dir, checkFile)
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

// Issue #914：DLP 防窥受限能力必须与普通构建解耦。
// 普通 default product 不编译、不打包 DLP 实现，也不声明受限权限；
// 只有 acl product 装配 privacy_acl HSP，并由 entry/src/acl 的 Provider 引用。
// 这里对源码和构建配置做静态断言，防止回退成「普通包也带 DLP」。
function checkDlpDecoupling() {
  const dir = import.meta.dirname
  const rel = (p) => p.replace(dir + '/', '')

  // 1. 两个 product 共享的 main，以及普通包的 standard sourceRoot，
  //    代码里不得出现 DLP Kit、dlpAntiPeep API 或 privacy_acl 依赖。
  //    只查代码：注释/字符串里的说明文字不算引用。
  const decoupledRoots = [
    join(dir, 'entry/src/main/ets'),
    join(dir, 'entry/src/standard/ets'),
  ]
  const forbidden = ['@kit.DeviceSecurityKit', 'dlpAntiPeep', 'privacy_acl']
  for (const root of decoupledRoots) {
    if (!existsSync(root)) continue
    walkEts(root, (p) => {
      const codeOnly = stripNonCode(readFileSync(p, 'utf8'))
      for (const needle of forbidden) {
        if (codeOnly.includes(needle)) {
          errors.push(`${rel(p)}: 普通构建不得引用 '${needle}'（Issue #914 解耦）`)
        }
      }
    })
  }

  // 2. 普通 entry 的权限清单不得声明 DLP 受限权限；该权限只属于 privacy_acl HSP。
  const entryModule = join(dir, 'entry/src/main/module.json5')
  if (existsSync(entryModule) && readFileSync(entryModule, 'utf8').includes('DLP_GET_HIDE_STATUS')) {
    errors.push('entry/src/main/module.json5: 普通构建不得声明 DLP_GET_HIDE_STATUS（Issue #914 解耦）')
  }
  const aclModule = join(dir, 'privacy_acl/src/main/module.json5')
  if (existsSync(aclModule) && !readFileSync(aclModule, 'utf8').includes('DLP_GET_HIDE_STATUS')) {
    errors.push('privacy_acl/src/main/module.json5: 受限权限应由 privacy_acl HSP 声明（Issue #914 解耦）')
  }

  // 3. 普通 entry 的模块依赖不得静态声明 privacy_acl；acl 依赖由 hvigorfile.ts 按 product 注入。
  const entryOhPackage = join(dir, 'entry/oh-package.json5')
  if (existsSync(entryOhPackage) && readFileSync(entryOhPackage, 'utf8').includes('privacy_acl')) {
    errors.push('entry/oh-package.json5: 普通模块不得静态依赖 privacy_acl（Issue #914 解耦）')
  }

  // 4. privacy_acl 模块只能装配到 acl product，不能 applyToProducts: ['default']。
  const projectProfile = join(dir, 'build-profile.json5')
  if (existsSync(projectProfile)) {
    const profile = readFileSync(projectProfile, 'utf8')
    const match = profile.match(/name:\s*'privacy_acl'[\s\S]*?applyToProducts:\s*\[([^\]]*)\]/)
    if (!match) {
      errors.push("build-profile.json5: 未找到 privacy_acl 的 applyToProducts（Issue #914 解耦）")
    } else if (!match[1].includes("'acl'") || /'default'/.test(match[1])) {
      errors.push("build-profile.json5: privacy_acl 只能 applyToProducts: ['acl']（Issue #914 解耦）")
    }
  }
}
checkDlpDecoupling()

if (errors.length > 0) {
  console.error('❌ Validation errors:')
  for (const e of errors) console.error(`  ${e}`)
  process.exit(1)
} else {
  console.log('✅ All .ets files pass basic validation')
}
