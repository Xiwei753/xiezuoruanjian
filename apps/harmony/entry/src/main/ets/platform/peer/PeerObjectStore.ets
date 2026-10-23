// PeerObjectStore — 附近设备同步的 ETS 对象库（Issue #988）。
//
// 这里不发明第二套同步格式，也不把对端文件直接覆盖 live 目录：
// - 以真实数据根 `app/`、`projects/<id>/` 为对象空间；
// - 版本/修改时间来自现有 `manifest.sync.json`，未入清单的本地文件按白名单扫描；
// - 对端对象先暂存完整字节并校验 SHA-256，整轮收齐后再做 LWW 比较和原子提交；
// - 删除保留墓碑时间，不把“文件不存在”误当成删除。

import fs from '@ohos.file.fs'
import emitter from '@ohos.events.emitter'
import { getAppDataRoot, getAppContext } from '../../app/AppContext'
import { HarmonyDeviceIdentity } from '../identity/HarmonyDeviceIdentity'
import { PeerCrypto } from './PeerCrypto'

const SYNC_CONTENT_CHANGED_EVENT_ID: number = 1001

export interface PeerStoredObject {
  path: string
  contentHash: string
  modifiedAtMs: number
  deleted: boolean
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
      const stat = fs.statSync(target.absolutePath)
      return {
        object: {
          path: path,
          contentHash: PeerCrypto.sha256Hex(bytes),
          modifiedAtMs: stat.mtime * 1000,
          deleted: false
        },
        bytes: bytes
      }
    } catch (_e) {
      return null
    }
  }

  stageRemoteObject(path: string, bytes: Uint8Array, contentHash: string,
    modifiedAtMs: number, deleted: boolean): PeerObjectCommitResult {
    if (!this.isAllowedPath(path) || this.resolveTarget(path) === null) {
      return failed(`不支持的同步对象路径：${path}`)
    }
    if (deleted) {
      this.staged.set(path, {
        path: path, contentHash: contentHash, modifiedAtMs: modifiedAtMs, deleted: true,
        bytes: new Uint8Array(0)
      })
      return { success: true, applied: 1, message: 'staged tombstone' }
    }
    const actualHash: string = PeerCrypto.sha256Hex(bytes)
    if (actualHash !== contentHash) {
      return failed('对象内容哈希不一致，已拒绝暂存')
    }
    this.staged.set(path, {
      path: path, contentHash: actualHash, modifiedAtMs: modifiedAtMs, deleted: false, bytes: bytes
    })
    return { success: true, applied: 1, message: 'staged' }
  }

  async commitRemoteObjects(): Promise<PeerObjectCommitResult> {
    let applied: number = 0
    const changedPaths: string[] = []
    try {
      this.staged.forEach((object: StagedPeerObject): void => {
        const target: LocalObjectTarget | null = this.resolveTarget(object.path)
        if (target === null) {
          throw new Error(`无法解析对象路径：${object.path}`)
        }
        const localModifiedAt: number = this.localModifiedAt(target.absolutePath)
        // 同一路径按现有 LWW 语义比较：本地修改更新时保留本地，不让旧对端对象回滚。
        if (localModifiedAt > object.modifiedAtMs) {
          return
        }
        if (object.path.endsWith('manifest.sync.json')) {
          return
        }
        if (object.deleted) {
          if (fs.accessSync(target.absolutePath)) {
            fs.unlinkSync(target.absolutePath)
          }
          this.updateManifest(target, object, true)
        } else {
          this.atomicWrite(target.absolutePath, object.bytes)
          this.updateManifest(target, object, false)
        }
        applied++
        changedPaths.push(object.path)
      })
      this.staged.clear()
      if (applied > 0) {
        // 用现有同步内容变更事件通知首页/工作区/统计刷新；不伪造“云同步成功”。
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
    const manifest: SyncManifest = this.readManifest(`${this.root}/app-meta/sync/manifest.sync.json`)
    manifest.files.forEach((record: ManifestFileRecord): void => {
      if (record.path === 'app-meta/sync/manifest.sync.json' || !this.isAppWhitelisted(record.path)) {
        return
      }
      this.addManifestRecord(objects, `app/${record.path}`, record,
        `${this.root}/${record.path}`)
    })
    this.scanDirectory(`${this.root}`, '', false, objects)
  }

  private collectProjectObjects(objects: Map<string, PeerStoredObject>): void {
    const projectsRoot: string = `${this.root}/projects`
    let entries: string[] = []
    try {
      entries = fs.listFileSync(projectsRoot)
    } catch (_e) {
      return
    }
    entries.forEach((projectId: string): void => {
      const projectRoot: string = `${projectsRoot}/${projectId}`
      const stat = fs.statSync(projectRoot)
      if (!stat.isDirectory()) {
        return
      }
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
      this.scanDirectory(projectRoot, `projects/${projectId}/`, true, objects)
    })
  }

  private scanDirectory(directory: string, remotePrefix: string, projectScoped: boolean,
    objects: Map<string, PeerStoredObject>): void {
    let entries: string[] = []
    try {
      entries = fs.listFileSync(directory)
    } catch (_e) {
      return
    }
    entries.forEach((name: string): void => {
      const fullPath: string = `${directory}/${name}`
      const remotePath: string = `${remotePrefix}${name}`
      const stat = fs.statSync(fullPath)
      if (stat.isDirectory()) {
        this.scanDirectory(fullPath, `${remotePath}/`, projectScoped, objects)
        return
      }
      const relativePath: string = projectScoped
        ? remotePath.substring(`projects/`.length).split('/').slice(1).join('/')
        : remotePath.substring(`app/`.length)
      if (relativePath.endsWith('manifest.sync.json')) {
        return
      }
      const allowed: boolean = projectScoped
        ? this.isProjectWhitelisted(relativePath)
        : this.isAppWhitelisted(relativePath)
      if (!allowed) {
        return
      }
      let bytes: Uint8Array
      try {
        bytes = this.readBytes(fullPath)
      } catch (_e) {
        return
      }
      const existing: PeerStoredObject | undefined = objects.get(remotePath)
      const hash: string = existing !== undefined && existing.modifiedAtMs === stat.mtime * 1000
        ? existing.contentHash : PeerCrypto.sha256Hex(bytes)
      objects.set(remotePath, {
        path: remotePath,
        contentHash: hash,
        modifiedAtMs: stat.mtime * 1000,
        deleted: false
      })
    })
  }

  private addManifestRecord(objects: Map<string, PeerStoredObject>, remotePath: string,
    record: ManifestFileRecord, absolutePath: string): void {
    if (record.op === 'delete') {
      const deletedAt: number = record.deleted_at_ms ?? record.updated_at_ms ?? 0
      objects.set(remotePath, {
        path: remotePath,
        contentHash: record.content_hash ?? '',
        modifiedAtMs: deletedAt,
        deleted: true
      })
      return
    }
    try {
      const stat = fs.statSync(absolutePath)
      const modifiedAt: number = stat.mtime * 1000
      const hash: string = record.updated_at_ms === modifiedAt && record.content_hash !== undefined
        ? record.content_hash : PeerCrypto.sha256Hex(this.readBytes(absolutePath))
      objects.set(remotePath, {
        path: remotePath,
        contentHash: hash,
        modifiedAtMs: modifiedAt,
        deleted: false
      })
    } catch (_e) {
      // 清单里仍是 upsert、文件却已不存在：不伪造删除墓碑，等待本地扫描/清单修复。
    }
  }

  private updateManifest(target: LocalObjectTarget, object: StagedPeerObject, deleted: boolean): void {
    const manifest: SyncManifest = this.readManifest(target.manifestPath)
    const next: ManifestFileRecord = {
      path: target.relativePath,
      content_hash: object.contentHash,
      updated_at_ms: object.modifiedAtMs,
      deleted_at_ms: deleted ? object.modifiedAtMs : null,
      device_id: this.localDeviceId,
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

  private localModifiedAt(path: string): number {
    try {
      return fs.accessSync(path) ? fs.statSync(path).mtime * 1000 : 0
    } catch (_e) {
      return 0
    }
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
      manifestPath: `${this.root}/app-meta/sync/manifest.sync.json`,
      relativePath: relativePath,
      projectScoped: false
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
    return /^[^/]+\/\d{4}-\d{2}-\d{2}\.(events|sessions)\.jsonl$/.test(path)
  }
}
