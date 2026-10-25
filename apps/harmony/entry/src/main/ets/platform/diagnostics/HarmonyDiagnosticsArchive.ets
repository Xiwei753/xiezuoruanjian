// HarmonyDiagnosticsArchive — native-module-independent diagnostics ZIP writer.
// Only callers' explicitly assembled staging trees are archived. Text and JSON are
// sanitized before they are written, so a ZIP never includes raw source attachments.

import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'
import zlib from '@ohos.zlib'

const LOG_TAG = 'HarmonyDiagnosticsArchive'
const REDACTED = '[REDACTED]'

const SENSITIVE_JSON_KEYS: string[] = [
  'token', 'access_token', 'refresh_token', 'authorization', 'password', 'passwd',
  'secret', 'private_key', 'ssh_private_key', 'content', 'text', 'body', 'chapter',
  'chapter_content', 'chapterContent', 'path', 'uri', 'url', 'external_log',
  'exception', 'stack'
]

export class HarmonyDiagnosticsArchive {
  private constructor() {}

  // Archive a completed staging tree to a temporary ZIP, then atomically publish it.
  // @ohos.zlib.compressFile accepts an application-sandbox file or directory.
  static async compressDirectory(
    sourceDir: string,
    outputDir: string,
    fileStem: string,
    keepExactName: boolean = false
  ): Promise<string> {
    HarmonyDiagnosticsArchive.ensureDirectory(outputDir)
    const suffix = HarmonyDiagnosticsArchive.timestampSuffix()
    const zipPath = `${outputDir}/${fileStem}${keepExactName ? '' : `-${suffix}`}.zip`
    const tempPath = `${outputDir}/.${fileStem}-${suffix}.tmp.zip`
    HarmonyDiagnosticsArchive.safeDelete(tempPath)
    try {
      await zlib.compressFile(sourceDir, tempPath, {
        level: zlib.CompressLevel.COMPRESS_LEVEL_DEFAULT_COMPRESSION
      })
      if (!fs.accessSync(tempPath) || fs.statSync(tempPath).size <= 0) {
        throw new Error('diagnostics archive is empty')
      }
      if (fs.accessSync(zipPath)) {
        fs.unlinkSync(zipPath)
      }
      fs.renameSync(tempPath, zipPath)
      return zipPath
    } catch (e) {
      HarmonyDiagnosticsArchive.safeDelete(tempPath)
      throw e instanceof Error ? e : new Error(String(e))
    }
  }

  // Writes one whitelisted relative file into a staging tree. Reject traversal and
  // absolute paths even though current callers use constants and sanitized event keys.
  static writeTextFile(rootDir: string, relativePath: string, content: string): void {
    if (!HarmonyDiagnosticsArchive.isSafeRelativePath(relativePath)) {
      throw new Error('invalid diagnostics attachment path')
    }
    const segments = relativePath.split('/')
    let parentDir = rootDir
    for (let i = 0; i < segments.length - 1; i++) {
      parentDir = `${parentDir}/${segments[i]}`
      HarmonyDiagnosticsArchive.ensureDirectory(parentDir)
    }
    const path = `${rootDir}/${relativePath}`
    const safeContent = relativePath.toLowerCase().endsWith('.json')
      ? HarmonyDiagnosticsArchive.sanitizeJson(content)
      : HarmonyDiagnosticsArchive.sanitizeText(content)
    const file = fs.openSync(path, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
    try {
      fs.writeSync(file.fd, safeContent)
    } finally {
      fs.closeSync(file)
    }
  }

  // Rust writer logs and system fault logs are UTF-8 diagnostic text. Keep their
  // useful lines while removing credentials, user content, URLs and absolute paths.
  static sanitizeText(content: string): string {
    let safe = content
    safe = safe.replace(/ssh_private_key\s*[:=]\s*[\s\S]*?-----END[^\r\n]*PRIVATE KEY-----/gi,
      `ssh_private_key=${REDACTED}`)
    safe = safe.replace(/-----BEGIN[^\r\n]*PRIVATE KEY-----[\s\S]*?-----END[^\r\n]*PRIVATE KEY-----/g,
      '[REDACTED_PEM]')
    safe = safe.replace(/\b(authorization)\s*[:=]\s*Bearer\s+\S+/gi,
      `Authorization: Bearer ${REDACTED}`)
    safe = safe.replace(/\b(token|access_token|refresh_token|authorization|password|passwd|secret|private_key)\s*[:=]\s*("[^"]*"|'[^']*'|\S+)/gi,
      `$1=${REDACTED}`)
    safe = safe.replace(/\b(content|text|body|chapter|chapter_content|chapterContent)\s*[:=]\s*("[^"]*"|'[^']*'|[^,}\]\r\n]+)/gi,
      `$1=${REDACTED}`)
    safe = safe.replace(/Bearer\s+[A-Za-z0-9\-._~+/]+=*/gi, `Bearer ${REDACTED}`)
    safe = safe.replace(/ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{82}/g, REDACTED)
    safe = safe.replace(/https?:\/\/[^\s"'<>|,;)}\]]+/gi, '[REDACTED_URL]')
    // Absolute local paths often include the app's private sandbox or a user's home.
    safe = safe.replace(/\/(?:[A-Za-z0-9._-]+\/){2,}[A-Za-z0-9._-]*/g, '[REDACTED_PATH]')
    return safe
  }

  // Apply structured redaction to JSON without running free-text substitutions over
  // JSON syntax. Malformed input becomes a generic safe error object.
  static sanitizeJson(content: string): string {
    try {
      const parsed: Object = JSON.parse(content) as Object
      return JSON.stringify(HarmonyDiagnosticsArchive.sanitizeJsonValue(parsed), null, 2)
    } catch (_) {
      return JSON.stringify({ error: 'diagnostics attachment json parse failed' }, null, 2)
    }
  }

  static removeTree(path: string): void {
    try {
      if (!fs.accessSync(path)) {
        return
      }
      const stat = fs.statSync(path)
      if (!stat.isDirectory()) {
        fs.unlinkSync(path)
        return
      }
      const entries = fs.listFileSync(path)
      for (const entry of entries) {
        HarmonyDiagnosticsArchive.removeTree(`${path}/${entry}`)
      }
      fs.rmdirSync(path)
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'staging cleanup failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
    }
  }

  private static sanitizeJsonValue(value: Object): Object {
    if (value === null || value === undefined) {
      return value
    }
    if (Array.isArray(value)) {
      const safeItems: Object[] = []
      for (const item of value) {
        safeItems.push(HarmonyDiagnosticsArchive.sanitizeJsonValue(item as Object))
      }
      return safeItems as Object
    }
    if (typeof value === 'string') {
      return HarmonyDiagnosticsArchive.sanitizeText(value) as Object
    }
    if (typeof value !== 'object') {
      return value
    }
    const source = value as Record<string, Object>
    const safeObject: Record<string, Object> = {}
    for (const key of Object.keys(source)) {
      if (HarmonyDiagnosticsArchive.isSensitiveJsonKey(key)) {
        safeObject[key] = REDACTED
      } else {
        safeObject[key] = HarmonyDiagnosticsArchive.sanitizeJsonValue(source[key])
      }
    }
    return safeObject as Object
  }

  private static isSensitiveJsonKey(key: string): boolean {
    const normalized = key.toLowerCase()
    return SENSITIVE_JSON_KEYS.some((sensitive: string): boolean => sensitive.toLowerCase() === normalized)
  }

  private static isSafeRelativePath(path: string): boolean {
    if (path.length === 0 || path.startsWith('/') || path.includes('\\')) {
      return false
    }
    const segments = path.split('/')
    for (const segment of segments) {
      if (segment.length === 0 || segment === '.' || segment === '..' || !/^[A-Za-z0-9._-]+$/.test(segment)) {
        return false
      }
    }
    return true
  }

  private static ensureDirectory(path: string): void {
    if (!fs.accessSync(path)) {
      fs.mkdirSync(path, true)
    }
  }

  private static safeDelete(path: string): void {
    try {
      if (fs.accessSync(path)) {
        fs.unlinkSync(path)
      }
    } catch (_) {
    }
  }

  private static timestampSuffix(): string {
    const now = new Date()
    const year = now.getFullYear()
    const month = (now.getMonth() + 1).toString().padStart(2, '0')
    const day = now.getDate().toString().padStart(2, '0')
    const hours = now.getHours().toString().padStart(2, '0')
    const minutes = now.getMinutes().toString().padStart(2, '0')
    const seconds = now.getSeconds().toString().padStart(2, '0')
    const millis = now.getMilliseconds().toString().padStart(3, '0')
    return `${year}${month}${day}-${hours}${minutes}${seconds}-${millis}`
  }
}
