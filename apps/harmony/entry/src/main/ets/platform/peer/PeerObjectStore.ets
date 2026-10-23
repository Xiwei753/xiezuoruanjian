// PeerObjectStore — 附近设备同步的 ETS 对象库（Issue #988）。
//
// 这里不发明第二套同步格式，也不把对端文件直接覆盖 live 目录：
// - 对象空间仍是 `app/`、`projects/<id>/`；
// - `manifest.sync.json` 的 content_hash 保持 Core 现有 MD5 语义，
//   传输分片的完整性使用独立的 SHA-256（见 PeerSyncTransport）；
// - App 对象按 Global / StarmapIndex / Starmap / StatsDevice 分组读取/更新各自
//   的 group manifest，旧总 manifest 只作为未迁移组的回退；
// - 章节正文属于三路保护对象：本地和远端都变化时保留本地并落一份冲突副本，
//   不用 mtime 直接覆盖；删除墓碑也不会因为文件暂时不存在而丢失。

import fs from '@ohos.file.fs'
import emitter from '@ohos.events.emitter'
import { getAppDataRoot, getAppContext } from '../../app/AppContext'
import { HarmonyDeviceIdentity } from '../identity/HarmonyDeviceIdentity'
import { PeerCrypto } from './PeerCrypto'

const SYNC_CONTENT_CHANGED_EVENT_ID: number = 1001

export interface PeerStoredObject {
  path: string
  // 现有 manifest 语义的 MD5（不是传输完整性 SHA-256）。
  contentHash: string
  modifiedAtMs: number
  deleted: boolean
  deviceId: string
}

export interface PeerStoredContent {
  object: PeerStoredObject
  bytes: Uint8Array
}

export interface PeerObjectCommitResult {
  success: boolean
  applied: number
  message: string
}

interface ManifestFileRecord {
  path: string
  content_hash?: string
  updated_at_ms?: number
  deleted_at_ms?: number | null
  device_id?: string
  op?: string
  schema_version?: number
}

interface SyncManifest {
  files: ManifestFileRecord[]
}

interface StagedPeerObject extends PeerStoredObject {
  bytes: Uint8Array
}

interface LocalObjectTarget {
  absolutePath: string
  manifestPath: string
  relativePath: string
  projectScoped: boolean
}

function failed(message: string): PeerObjectCommitResult {
  return { success: false, applied: 0, message: message }
}

function encodePathSegment(value: string): string {
  const bytes: Uint8Array = PeerCrypto.utf8Encode(value)
  let encoded: string = ''
  for (let i = 0; i < bytes.length; i++) {
    encoded += bytes[i].toString(16).padStart(2, '0')
  }
  return encoded
}

export class PeerObjectStore {
  private root: string
  private localDeviceId: string
  private staged: Map<string, StagedPeerObject> = new Map<string, StagedPeerObject>()

  constructor(root: string = getAppDataRoot()) {
    this.root = root
    const context = getAppContext().getAbilityContext()
    this.localDeviceId = context === null
      ? 'unknown'
      : HarmonyDeviceIdentity.getOrCreateDeviceId(context)
  }

  listLocalObjects(): PeerStoredObject[] {
    const objects: Map<string, PeerStoredObject> = new Map<string, PeerStoredObject>()
    this.collectAppObjects(objects)
    this.collectProjectObjects(objects)
    return Array.from(objects.values())
  }

  readLocalObject(path: string): PeerStoredContent | null {
    const target: LocalObjectTarget | null = this.resolveTarget(path)
    if (target === null || target.relativePath.endsWith('manifest.sync.json')) {
      return null
    }
    try {
      if (!fs.accessSync(target.absolutePath)) {
        return null
      }
      const bytes: Uint8Array = this.readBytes(target.absolutePath)
      const record: ManifestFileRecord | null = this.findManifestRecord(target, path)
      const stat = fs.statSync(target.absolutePath)
      return {
        object: {
          path: path,
          contentHash: record !== null && record.op !== 'delete' && record.content_hash !== undefined
            ? record.content_hash : PeerCrypto.md5Hex(bytes),
          modifiedAtMs: record !== null && record.op !== 'delete'
            ? (record.updated_at_ms ?? stat.mtime * 1000)
            : stat.mtime * 1000,
          deleted: false,
          deviceId: record !== null && record.device_id !== undefined
            ? record.device_id : this.localDeviceId
        },
        bytes: bytes
      }
    } catch (_e) {
      return null
    }
  }

  stageRemoteObject(path: string, bytes: Uint8Array, versionToken: string,
    modifiedAtMs: number, deleted: boolean, deviceId: string): PeerObjectCommitResult {
    const target: LocalObjectTarget | null = this.resolveTarget(path)
    if (target === null) {
      return failed(`不支持的同步对象路径：${path}`)
    }
    if (deleted) {
      this.staged.set(path, {
        path: path, contentHash: versionToken, modifiedAtMs: modifiedAtMs, deleted: true,
        deviceId: deviceId, bytes: new Uint8Array(0)
      })
      return { success: true, applied: 1, message: 'staged tombstone' }
    }
    const actualVersion: string = PeerCrypto.md5Hex(bytes)
    if (versionToken.length > 0 && versionToken !== actualVersion) {
      return failed('对象版本哈希不一致，已拒绝暂存')
    }
    this.staged.set(path, {
      path: path, contentHash: actualVersion, modifiedAtMs: modifiedAtMs, deleted: false,
      deviceId: deviceId, bytes: bytes
    })
    return { success: true, applied: 1, message: 'staged' }
  }

  async commitRemoteObjects(): Promise<PeerObjectCommitResult> {
    let applied: number = 0
    try {
      const pending: StagedPeerObject[] = Array.from(this.staged.values())
      // 先全部完成规则判断，再逐个原子落盘；单文件 rename 不是整轮两阶段事务，
      // 但不会在判断阶段中途改写 live 数据。
      for (const object of pending) {
        const target: LocalObjectTarget | null = this.resolveTarget(object.path)
        if (target === null) {
          throw new Error(`无法解析对象路径：${object.path}`)
        }
        if (object.path.endsWith('manifest.sync.json')) {
          continue
        }
        if (this.isLocalOwnedStatsPartition(object.path)) {
          continue
        }
        const localRecord: ManifestFileRecord | null = this.findManifestRecord(target, object.path)
        const localBytes: Uint8Array | null = this.readIfExists(target.absolutePath)
        const localHash: string = localBytes === null ? '' : PeerCrypto.md5Hex(localBytes)
        const baseHash: string = localRecord !== null && localRecord.content_hash !== undefined
          ? localRecord.content_hash : ''
        const localChanged: boolean = localHash.length > 0 && localHash !== baseHash
        const localExists: boolean = localBytes !== null

        if (object.deleted) {
          // 删除墓碑遇到本地修改（或未登记的本地文件）时不能直接删，保留本地事实。
          if (localChanged || (localExists && localRecord === null)) {
            this.writeConflict(object, target, 'delete')
            continue
          }
          if (fs.accessSync(target.absolutePath)) {
            fs.unlinkSync(target.absolutePath)
          }
          this.updateManifest(target, object, true)
          applied++
          continue
        }

        // 章节正文：本地和远端相对共同基线都变化时，绝不静默覆盖本地。
        if (this.isChapterText(object.path) && localChanged && localHash !== object.contentHash) {
          this.writeConflict(object, target, 'update')
          continue
        }

        if (!this.remoteWins(object, localRecord, localHash)) {
          continue
        }
        this.atomicWrite(target.absolutePath, object.bytes)
        this.updateManifest(target, object, false)
        applied++
      }
      this.staged.clear()
      if (applied > 0) {
        emitter.emit({ eventId: SYNC_CONTENT_CHANGED_EVENT_ID })
      }
      return { success: true, applied: applied,
        message: applied > 0 ? `已合并 ${applied} 个附近设备对象` : '没有需要提交的对象' }
    } catch (e) {
      return failed(`提交附近设备对象失败：${String(e)}`)
    }
  }

  clearStaged(): void {
    this.staged.clear()
  }

  private collectAppObjects(objects: Map<string, PeerStoredObject>): void {
    // 必须带 `app/` 前缀扫描；否则 settings.sync.json 会被错误截成 ings.sync.json。
    this.scanDirectory(this.root, 'app/', false, objects, new Set<string>())
    this.collectGroupManifests(objects)
    this.collectLegacyManifest(objects)
  }

  private collectProjectObjects(objects: Map<string, PeerStoredObject>): void {
    const projectsRoot: string = `${this.root}/projects`
    let entries: string[] = []
    try {
      entries = fs.listFileSync(projectsRoot)
    } catch (_e) {
      return
    }
    for (const projectId of entries) {
      const projectRoot: string = `${projectsRoot}/${projectId}`
      const stat = fs.statSync(projectRoot)
      if (!stat.isDirectory()) {
        continue
      }
      this.scanDirectory(projectRoot, `projects/${projectId}/`, true, objects, new Set<string>())
      const manifest: SyncManifest =
        this.readManifest(`${projectRoot}/app-meta/sync/manifest.sync.json`)
      manifest.files.forEach((record: ManifestFileRecord): void => {
        if (record.path === 'app-meta/sync/manifest.sync.json' ||
          !this.isProjectWhitelisted(record.path)) {
          return
        }
        this.addManifestRecord(objects, `projects/${projectId}/${record.path}`, record,
          `${projectRoot}/${record.path}`)
      })
    }
  }

  private collectGroupManifests(objects: Map<string, PeerStoredObject>): void {
    const groupsRoot: string = `${this.root}/app-meta/sync/groups`
    let groupDirs: string[] = []
    try {
      groupDirs = fs.listFileSync(groupsRoot)
    } catch (_e) {
      return
    }
    for (const groupDir of groupDirs) {
      const manifest: SyncManifest =
        this.readManifest(`${groupsRoot}/${groupDir}/manifest.sync.json`)
      manifest.files.forEach((record: ManifestFileRecord): void => {
        if (!this.isAppWhitelisted(record.path)) {
          return
        }
        this.addManifestRecord(objects, `app/${record.path}`, record,
          `${this.root}/${record.path}`)
      })
    }
  }

  // 旧总 manifest 只补齐尚未迁移的组，不覆盖新 group manifest。
  private collectLegacyManifest(objects: Map<string, PeerStoredObject>): void {
    const legacy: SyncManifest = this.readManifest(`${this.root}/app-meta/sync/manifest.sync.json`)
    legacy.files.forEach((record: ManifestFileRecord): void => {
      if (!this.isAppWhitelisted(record.path)) {
        return
      }
      const storageKey: string = this.appGroupStorageKey(record.path)
      if (this.appGroupManifestExists(storageKey)) {
        return
      }
      this.addManifestRecord(objects, `app/${record.path}`, record,
        `${this.root}/${record.path}`)
    })
  }

  private scanDirectory(directory: string, remotePrefix: string, projectScoped: boolean,
    objects: Map<string, PeerStoredObject>, visited: Set<string>): void {
    if (visited.has(directory)) {
      return
    }
    visited.add(directory)
    let entries: string[] = []
    try {
      entries = fs.listFileSync(directory)
    } catch (_e) {
      return
    }
    for (const name of entries) {
      if (name === 'manifest.sync.json' || name.endsWith('.peer-tmp') || name.endsWith('.lock') ||
        name.endsWith('.tmp')) {
        continue
      }
      const fullPath: string = `${directory}/${name}`
      const remotePath: string = `${remotePrefix}${name}`
      const stat = fs.statSync(fullPath)
      if (stat.isDirectory()) {
        this.scanDirectory(fullPath, `${remotePath}/`, projectScoped, objects, visited)
        continue
      }
      const relativePath: string = projectScoped
        ? remotePath.substring(`projects/`.length).split('/').slice(1).join('/')
        : remotePath.substring(`app/`.length)
      const allowed: boolean = projectScoped
        ? this.isProjectWhitelisted(relativePath)
        : this.isAppWhitelisted(relativePath)
      if (!allowed) {
        continue
      }
      let bytes: Uint8Array
      try {
        bytes = this.readBytes(fullPath)
      } catch (_e) {
        continue
      }
      const target: LocalObjectTarget | null = this.resolveTarget(remotePath)
      const record: ManifestFileRecord | null = target === null
        ? null : this.findManifestRecord(target, remotePath)
      const actualHash: string = PeerCrypto.md5Hex(bytes)
      const hash: string = actualHash
      objects.set(remotePath, {
        path: remotePath,
        contentHash: hash,
        modifiedAtMs: record !== null && record.content_hash === actualHash
          ? (record.deleted_at_ms ?? record.updated_at_ms ?? stat.mtime * 1000)
          : stat.mtime * 1000,
        deleted: false,
        deviceId: record !== null && record.device_id !== undefined
          ? record.device_id : this.localDeviceId
      })
    }
  }

  private addManifestRecord(objects: Map<string, PeerStoredObject>, remotePath: string,
    record: ManifestFileRecord, absolutePath: string): void {
    if (record.op === 'delete') {
      let fileExists: boolean = false
      try {
        fileExists = fs.accessSync(absolutePath)
      } catch (_e) {
        fileExists = false
      }
      if (!fileExists) {
        objects.set(remotePath, {
          path: remotePath,
          contentHash: record.content_hash ?? '',
          modifiedAtMs: record.deleted_at_ms ?? record.updated_at_ms ?? 0,
          deleted: true,
          deviceId: record.device_id ?? 'unknown'
        })
        return
      }
    }
    try {
      const stat = fs.statSync(absolutePath)
      const bytes: Uint8Array = this.readBytes(absolutePath)
      const actualHash: string = PeerCrypto.md5Hex(bytes)
      const hash: string = actualHash
      const unchanged: boolean = record.content_hash === actualHash
      objects.set(remotePath, {
        path: remotePath,
        contentHash: hash,
        modifiedAtMs: unchanged ? (record.updated_at_ms ?? stat.mtime * 1000) : stat.mtime * 1000,
        deleted: false,
        deviceId: record.device_id ?? this.localDeviceId
      })
    } catch (_e) {
      // 清单仍是 upsert、文件已不存在：保留删除/冲突判断给业务层，不伪造墓碑。
    }
  }

  private updateManifest(target: LocalObjectTarget, object: StagedPeerObject, deleted: boolean): void {
    const manifest: SyncManifest = this.readManifest(target.manifestPath)
    const next: ManifestFileRecord = {
      path: target.relativePath,
      content_hash: object.contentHash,
      updated_at_ms: object.modifiedAtMs,
      deleted_at_ms: deleted ? object.modifiedAtMs : null,
      device_id: object.deviceId,
      op: deleted ? 'delete' : 'upsert',
      schema_version: 1
    }
    let replaced: boolean = false
    for (let i = 0; i < manifest.files.length; i++) {
      if (manifest.files[i].path === target.relativePath) {
        manifest.files[i] = next
        replaced = true
        break
      }
    }
    if (!replaced) {
      manifest.files.push(next)
    }
    this.atomicWrite(target.manifestPath, PeerCrypto.utf8Encode(JSON.stringify(manifest)))
  }

  private findManifestRecord(target: LocalObjectTarget, remotePath: string): ManifestFileRecord | null {
    const manifest: SyncManifest = this.readManifest(target.manifestPath)
    const relative: string = target.relativePath
    for (const record of manifest.files) {
      if (record.path === relative) {
        return record
      }
    }
    return null
  }

  private readManifest(path: string): SyncManifest {
    try {
      if (!fs.accessSync(path)) {
        return { files: [] }
      }
      const text: string = PeerCrypto.utf8Decode(this.readBytes(path))
      const parsed: SyncManifest = JSON.parse(text) as SyncManifest
      if (parsed.files === undefined || !Array.isArray(parsed.files)) {
        return { files: [] }
      }
      return parsed
    } catch (_e) {
      return { files: [] }
    }
  }

  private readIfExists(path: string): Uint8Array | null {
    try {
      return fs.accessSync(path) ? this.readBytes(path) : null
    } catch (_e) {
      return null
    }
  }

  private remoteWins(object: StagedPeerObject, local: ManifestFileRecord | null,
    localHash: string): boolean {
    if (localHash.length === 0 && local === null) {
      return true
    }
    if (local === null) {
      return true
    }
    const localTime: number = local.deleted_at_ms ?? local.updated_at_ms ?? 0
    if (object.modifiedAtMs > localTime) {
      return true
    }
    if (object.modifiedAtMs < localTime) {
      return false
    }
    const localDevice: string = local.device_id ?? ''
    return object.deviceId >= localDevice
  }

  private writeConflict(object: StagedPeerObject, target: LocalObjectTarget, action: string): void {
    try {
      const conflictRoot: string = target.projectScoped
        ? `${this.root}/projects/${this.projectIdForTarget(target)}/app-meta/sync/conflicts`
        : `${this.root}/app-meta/sync/conflicts`
      fs.mkdirSync(conflictRoot, true)
      const baseName: string = target.relativePath.replace(/[\\/]/g, '_')
      const suffix: string = action === 'delete' ? 'delete' : 'remote'
      const conflictPath: string =
        `${conflictRoot}/peer-${Date.now()}-${suffix}-${baseName}.conflict`
      this.atomicWrite(conflictPath, object.bytes)
    } catch (_e) {
      // 冲突副本写失败时仍保留本地文件，绝不因副本失败覆盖本地正文。
    }
  }

  private projectIdForTarget(target: LocalObjectTarget): string {
    const marker: string = `${this.root}/projects/`
    const rest: string = target.absolutePath.substring(marker.length)
    const slash: number = rest.indexOf('/')
    return slash < 0 ? rest : rest.substring(0, slash)
  }

  private isChapterText(path: string): boolean {
    return path.endsWith('/chapter.md')
  }

  private isLocalOwnedStatsPartition(path: string): boolean {
    const match = /^app\/stats\/events\/([^/]+)\//.exec(path)
    return match !== null && match[1] === this.localDeviceId
  }

  private readBytes(path: string): Uint8Array {
    const stat = fs.statSync(path)
    const buffer: ArrayBuffer = new ArrayBuffer(stat.size)
    const file = fs.openSync(path, fs.OpenMode.READ_ONLY)
    try {
      fs.readSync(file.fd, buffer)
    } finally {
      fs.closeSync(file)
    }
    return new Uint8Array(buffer)
  }

  private atomicWrite(path: string, bytes: Uint8Array): void {
    const parent: string = path.substring(0, path.lastIndexOf('/'))
    if (parent.length > 0) {
      fs.mkdirSync(parent, true)
    }
    const temp: string = `${path}.peer-tmp-${Date.now()}-${Math.floor(Math.random() * 100000)}`
    const file = fs.openSync(temp,
      fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
    try {
      fs.writeSync(file.fd, bytes.buffer)
      fs.fsyncSync(file.fd)
    } finally {
      fs.closeSync(file)
    }
    fs.renameSync(temp, path)
  }

  private resolveTarget(path: string): LocalObjectTarget | null {
    if (!this.isAllowedPath(path)) {
      return null
    }
    if (path.startsWith('projects/')) {
      const rest: string = path.substring('projects/'.length)
      const slash: number = rest.indexOf('/')
      if (slash <= 0) {
        return null
      }
      const projectId: string = rest.substring(0, slash)
      const relativePath: string = rest.substring(slash + 1)
      const projectRoot: string = `${this.root}/projects/${projectId}`
      return {
        absolutePath: `${projectRoot}/${relativePath}`,
        manifestPath: `${projectRoot}/app-meta/sync/manifest.sync.json`,
        relativePath: relativePath,
        projectScoped: true
      }
    }
    const relativePath: string = path.substring('app/'.length)
    return {
      absolutePath: `${this.root}/${relativePath}`,
      manifestPath: `${this.root}/app-meta/sync/groups/${this.appGroupStorageKey(relativePath)}/manifest.sync.json`,
      relativePath: relativePath,
      projectScoped: false
    }
  }

  private appGroupStorageKey(relativePath: string): string {
    if (relativePath === 'settings.sync.json' || relativePath.startsWith('themes/palettes/')) {
      return 'global'
    }
    if (relativePath === 'starmaps/index.json') {
      return 'starmap-index'
    }
    const statsMatch = /^stats\/events\/([^/]+)\//.exec(relativePath)
    if (statsMatch !== null) {
      return `stats-device-${encodePathSegment(statsMatch[1])}`
    }
    const starmapMatch = /^starmaps\/([^/]+?)(?:\.meta\.json|\/.*)/.exec(relativePath)
    if (starmapMatch !== null) {
      return `starmap-${encodePathSegment(starmapMatch[1])}`
    }
    return 'global'
  }

  private appGroupManifestExists(storageKey: string): boolean {
    try {
      return fs.accessSync(`${this.root}/app-meta/sync/groups/${storageKey}/manifest.sync.json`)
    } catch (_e) {
      return false
    }
  }

  private isAllowedPath(path: string): boolean {
    if (path.length === 0 || path.startsWith('/') || path.includes('\\') || path.includes('..')) {
      return false
    }
    if (path.startsWith('app/')) {
      return this.isAppWhitelisted(path.substring('app/'.length))
    }
    if (path.startsWith('projects/')) {
      const rest: string = path.substring('projects/'.length)
      const slash: number = rest.indexOf('/')
      if (slash <= 0) {
        return false
      }
      return this.isProjectWhitelisted(rest.substring(slash + 1))
    }
    return false
  }

  private isProjectWhitelisted(path: string): boolean {
    if (path === 'project.json' || path === 'app-meta/sync/manifest.sync.json') {
      return true
    }
    if (path.startsWith('characters/')) {
      return true
    }
    if (path.startsWith('volumes/')) {
      return path.endsWith('/volume.json') || path.endsWith('/chapter.md') ||
        path.endsWith('/chapter.meta.json')
    }
    return false
  }

  private isAppWhitelisted(path: string): boolean {
    if (path === 'settings.sync.json' || path === 'app-meta/sync/manifest.sync.json') {
      return true
    }
    if (path.startsWith('starmaps/') || path.startsWith('themes/palettes/')) {
      return true
    }
    return /^stats\/events\/[^/]+\/\d{4}-\d{2}-\d{2}\.(events|sessions)\.jsonl$/.test(path)
  }
}
