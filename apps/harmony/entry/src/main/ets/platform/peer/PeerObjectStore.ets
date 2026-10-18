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
import { PeerStarMapMerge, PeerStarMapMergeResult } from './PeerStarMapMerge'

const SYNC_CONTENT_CHANGED_EVENT_ID: number = 1001

// 用户正文：不能按 mtime 直接跳过来自附近设备的版本，必须进入三路保护。
export function isPeerDocumentPath(path: string): boolean {
  return path.endsWith('/chapter.md')
}

// 星图索引 / GraphMeta：必须按稳定 ID 合并，不能整文件 LWW。
export function isPeerStarMapSemanticPath(path: string): boolean {
  return PeerStarMapMerge.isSemanticPath(path)
}

// 星图节点/边/嵌入等对象文件：按对象 ID 独立存在，双方都改时保留本机并记录冲突。
export function isPeerStarMapObjectPath(path: string): boolean {
  return path.startsWith('app/starmaps/') && !isPeerStarMapSemanticPath(path) &&
    !path.endsWith('/manifest.sync.json')
}

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
  conflicts: number
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

// 与 Core `SyncConflict` 的线格式保持一致；冲突页通过 listAllSyncConflicts 读取。
interface SyncConflictRecord {
  local_path: string
  remote_path: string
  local_hash: string
  remote_hash: string
  base_hash: string
  created_at: number
  description: string
  kind: string
  remote_snapshot_path: string | null
  remote_graph_snapshot_dir: string | null
}

// 只声明附近设备需要维护的字段；JSON.parse 后未声明字段仍会原样保留。
interface SyncStateFile {
  last_sync_time?: number | null
  last_error?: string | null
  known_files?: Record<string, string>
  conflicts?: SyncConflictRecord[]
  tombstones?: Object[]
  deleted_files?: string[]
  device_id?: string
  known_files_updated_at?: Record<string, number>
  conflicted_files?: string[]
  pending_take_remote?: string[]
}

interface SyncStatePaths {
  statePath: string
  conflictsPath: string
  conflictDir: string
  syncRoot: string
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
  return { success: false, applied: 0, conflicts: 0, message: message }
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

  stagedPaths(): string[] {
    return Array.from(this.staged.keys())
  }

  // 是否存在同步共同基线。章节三路保护依赖它；没有基线时不拿当前文件冒充。
  hasKnownBase(path: string): boolean {
    const target: LocalObjectTarget | null = this.resolveTarget(path)
    if (target === null) {
      return false
    }
    const state: SyncStateFile = this.readSyncState(this.syncStatePaths(target).statePath)
    return state.known_files !== undefined &&
      state.known_files[target.relativePath] !== undefined &&
      (state.known_files[target.relativePath] as string).length > 0
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
      const actualHash: string = PeerCrypto.md5Hex(bytes)
      const record: ManifestFileRecord | null = this.findManifestRecord(target, path)
      const stat = fs.statSync(target.absolutePath)
      // 旧 manifest 记录只有在“当前字节仍是记录里的版本”时才能沿用其时间/胜者身份。
      // 否则用户刚保存的新正文还带着旧记录的版本，发送端会把旧 MD5 和新字节混在一起。
      const recordMatches: boolean = record !== null && record.op === 'upsert' &&
        record.content_hash === actualHash
      return {
        object: {
          path: path,
          contentHash: actualHash,
          modifiedAtMs: recordMatches
            ? (record as ManifestFileRecord).updated_at_ms ?? stat.mtime * 1000
            : stat.mtime * 1000,
          deleted: false,
          deviceId: recordMatches && (record as ManifestFileRecord).device_id !== undefined
            ? (record as ManifestFileRecord).device_id as string : this.localDeviceId
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
      return { success: true, applied: 1, conflicts: 0, message: 'staged tombstone' }
    }
    const actualVersion: string = PeerCrypto.md5Hex(bytes)
    if (versionToken.length > 0 && versionToken !== actualVersion) {
      return failed('对象版本哈希不一致，已拒绝暂存')
    }
    this.staged.set(path, {
      path: path, contentHash: actualVersion, modifiedAtMs: modifiedAtMs, deleted: false,
      deviceId: deviceId, bytes: bytes
    })
    return { success: true, applied: 1, conflicts: 0, message: 'staged' }
  }

  async commitRemoteObjects(): Promise<PeerObjectCommitResult> {
    let applied: number = 0
    let conflicts: number = 0
    try {
      const pending: StagedPeerObject[] = Array.from(this.staged.values())
      // 星图 graph/index 必须最后落盘：先把节点/边/嵌入对象文件提交完，
      // 再按稳定 ID 合并成员表，避免短暂出现 graph 指向尚未落盘的对象。
      pending.sort((left: StagedPeerObject, right: StagedPeerObject): number =>
        this.commitOrder(left.path) - this.commitOrder(right.path))
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
        const protectedPath: boolean = isPeerDocumentPath(object.path) ||
          isPeerStarMapSemanticPath(object.path) || isPeerStarMapObjectPath(object.path)
        const baseHash: string = this.knownBase(target, localRecord, localHash, protectedPath)
        const localModifiedAt: number = this.localModifiedAt(target, localRecord, localHash)
        const localChanged: boolean = localHash.length > 0 &&
          (baseHash.length === 0 || localHash !== baseHash)
        const localExists: boolean = localBytes !== null

        if (object.deleted) {
          // 删除墓碑遇到本地修改时不能直接删，保留本地事实并记录 remote_deleted。
          // 普通元数据仍遵守 LWW：本机修改严格晚于远端删除时，删除不生效。
          if (localExists) {
            if (localRecord === null) {
              if (!protectedPath && localModifiedAt > object.modifiedAtMs) {
                continue
              }
              this.writeConflict(object, target, 'delete', localHash, baseHash)
              conflicts++
              continue
            }
            if (localChanged) {
              if (!protectedPath && localModifiedAt > object.modifiedAtMs) {
                continue
              }
              this.writeConflict(object, target, 'delete', localHash, baseHash)
              conflicts++
              continue
            }
          }
          if (fs.accessSync(target.absolutePath)) {
            fs.unlinkSync(target.absolutePath)
          }
          this.updateManifest(target, object, true)
          this.removeKnown(target)
          applied++
          continue
        }

        if (localHash.length > 0 && localHash === object.contentHash) {
          continue
        }

        // 星图索引 / GraphMeta：按稳定对象 ID 合并，绝不整文件 mtime LWW。
        if (isPeerStarMapSemanticPath(object.path)) {
          if (localBytes !== null) {
            let merged: PeerStarMapMergeResult
            try {
              merged = PeerStarMapMerge.merge(object.path, localBytes, object.bytes)
            } catch (e) {
              throw new Error(`星图语义合并失败：${String(e)}`)
            }
            if (merged.changed) {
              this.atomicWrite(target.absolutePath, merged.bytes)
              const mergedHash: string = PeerCrypto.md5Hex(merged.bytes)
              this.updateManifest(target, object, false, mergedHash)
              if (!merged.conflict) {
                this.markKnown(target, mergedHash)
              }
              applied++
            }
            if (merged.conflict) {
              this.writeConflict(object, target, 'update', localHash, baseHash,
                merged.description)
              conflicts++
            }
            continue
          }
          this.applyRemoteObject(target, object)
          applied++
          continue
        }

        // 用户正文与星图对象文件：本地和远端相对共同基线都变化时，
        // 绝不静默覆盖本地。没有基线时也保留本机并记录冲突。
        const remoteChanged: boolean = baseHash.length === 0 ||
          object.contentHash !== baseHash
        const protectedObject: boolean = isPeerDocumentPath(object.path) ||
          isPeerStarMapObjectPath(object.path)
        if (protectedObject && localChanged && remoteChanged) {
          this.writeConflict(object, target, 'update', localHash, baseHash)
          conflicts++
          continue
        }
        if (!remoteChanged) {
          continue
        }
        // 本地相对共同基线未变，远端有变化：三路语义下直接采用远端，
        // 不能再拿文件 mtime 把远端新内容挡掉（本地 mtime 可能只是复制/打开时间）。
        if (!localChanged) {
          this.applyRemoteObject(target, object)
          applied++
          continue
        }

        if (!this.remoteWins(object, localRecord, localHash, localModifiedAt)) {
          continue
        }
        this.applyRemoteObject(target, object)
        applied++
      }
      this.staged.clear()
      if (applied > 0 || conflicts > 0) {
        emitter.emit({ eventId: SYNC_CONTENT_CHANGED_EVENT_ID })
      }
      return { success: true, applied: applied, conflicts: conflicts,
        message: this.commitMessage(applied, conflicts) }
    } catch (e) {
      return failed(`提交附近设备对象失败：${String(e)}`)
    }
  }

  private commitOrder(path: string): number {
    if (path === 'app/starmaps/index.json') {
      return 3
    }
    if (isPeerStarMapSemanticPath(path)) {
      return 2
    }
    return path.startsWith('app/starmaps/') ? 1 : 0
  }

  private applyRemoteObject(target: LocalObjectTarget, object: StagedPeerObject): void {
    this.atomicWrite(target.absolutePath, object.bytes)
    this.updateManifest(target, object, false)
    this.markKnown(target, object.contentHash)
  }

  private knownBase(target: LocalObjectTarget, localRecord: ManifestFileRecord | null,
    localHash: string, requireStateBaseline: boolean): string {
    const state: SyncStateFile = this.readSyncState(this.syncStatePaths(target).statePath)
    const known: string | undefined = state.known_files === undefined
      ? undefined : state.known_files[target.relativePath]
    if (known !== undefined && known.length > 0) {
      return known
    }
    // 正文、星图对象与星图语义文件必须使用 Core 维护的共同基线 known_files；
    // manifest 里的 hash 可能只是本机当前版本，不能冒充三方合并的 base，
    // 否则会把“本机有新内容”误判成未修改，被远端 LWW 覆盖。
    if (requireStateBaseline) {
      return ''
    }
    // 旧数据可能还没有 state.local.json；普通 LWW 元数据允许 manifest 作兼容回退。
    return localRecord !== null && localRecord.op === 'upsert' &&
      localRecord.content_hash !== undefined && localRecord.content_hash !== localHash
      ? localRecord.content_hash : ''
  }

  private localModifiedAt(target: LocalObjectTarget, localRecord: ManifestFileRecord | null,
    localHash: string): number {
    if (localHash.length === 0) {
      return localRecord !== null ? (localRecord.deleted_at_ms ?? localRecord.updated_at_ms ?? 0) : 0
    }
    if (localRecord !== null && localRecord.op === 'upsert' &&
      localRecord.content_hash === localHash && localRecord.updated_at_ms !== undefined) {
      return localRecord.updated_at_ms
    }
    try {
      return fs.statSync(target.absolutePath).mtime * 1000
    } catch (_e) {
      return 0
    }
  }

  private markKnown(target: LocalObjectTarget, contentHash: string): void {
    const paths: SyncStatePaths = this.syncStatePaths(target)
    const state: SyncStateFile = this.readSyncState(paths.statePath)
    const known: Record<string, string> = state.known_files ?? {}
    known[target.relativePath] = contentHash
    state.known_files = known
    const updated: Record<string, number> = state.known_files_updated_at ?? {}
    updated[target.relativePath] = Date.now()
    state.known_files_updated_at = updated
    if (state.device_id === undefined || state.device_id.length === 0) {
      state.device_id = this.localDeviceId
    }
    this.atomicWrite(paths.statePath, PeerCrypto.utf8Encode(JSON.stringify(state)))
  }

  private removeKnown(target: LocalObjectTarget): void {
    const paths: SyncStatePaths = this.syncStatePaths(target)
    const state: SyncStateFile = this.readSyncState(paths.statePath)
    const known: Record<string, string> = {}
    if (state.known_files !== undefined) {
      Object.keys(state.known_files).forEach((key: string): void => {
        if (key !== target.relativePath) {
          known[key] = (state.known_files as Record<string, string>)[key]
        }
      })
    }
    state.known_files = known
    const updated: Record<string, number> = {}
    if (state.known_files_updated_at !== undefined) {
      Object.keys(state.known_files_updated_at).forEach((key: string): void => {
        if (key !== target.relativePath) {
          updated[key] = (state.known_files_updated_at as Record<string, number>)[key]
        }
      })
    }
    state.known_files_updated_at = updated
    this.atomicWrite(paths.statePath, PeerCrypto.utf8Encode(JSON.stringify(state)))
  }

  private commitMessage(applied: number, conflicts: number): string {
    if (conflicts > 0 && applied > 0) {
      return `已合并 ${applied} 个附近设备对象，另有 ${conflicts} 个冲突待处理`
    }
    if (conflicts > 0) {
      return `已记录 ${conflicts} 个附近设备冲突，本地内容未被覆盖`
    }
    return applied > 0 ? `已合并 ${applied} 个附近设备对象` : '没有需要提交的对象'
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
        if (record.path.endsWith('manifest.sync.json') || !this.isAppWhitelisted(record.path)) {
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
      if (record.path.endsWith('manifest.sync.json') || !this.isAppWhitelisted(record.path)) {
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
      const recordMatches: boolean = record !== null && record.op === 'upsert' &&
        record.content_hash === actualHash
      objects.set(remotePath, {
        path: remotePath,
        contentHash: actualHash,
        modifiedAtMs: recordMatches
          ? ((record as ManifestFileRecord).updated_at_ms ?? stat.mtime * 1000)
          : stat.mtime * 1000,
        deleted: false,
        deviceId: recordMatches && (record as ManifestFileRecord).device_id !== undefined
          ? (record as ManifestFileRecord).device_id as string : this.localDeviceId
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
      const unchanged: boolean = record.op === 'upsert' && record.content_hash === actualHash
      objects.set(remotePath, {
        path: remotePath,
        contentHash: actualHash,
        modifiedAtMs: unchanged ? (record.updated_at_ms ?? stat.mtime * 1000) : stat.mtime * 1000,
        deleted: false,
        deviceId: unchanged && record.device_id !== undefined
          ? record.device_id : this.localDeviceId
      })
    } catch (_e) {
      // 清单仍是 upsert、文件已不存在：保留删除/冲突判断给业务层，不伪造墓碑。
    }
  }

  private updateManifest(target: LocalObjectTarget, object: StagedPeerObject,
    deleted: boolean, contentHashOverride: string = ''): void {
    const manifest: SyncManifest = this.readManifest(target.manifestPath)
    const next: ManifestFileRecord = {
      path: target.relativePath,
      content_hash: contentHashOverride.length > 0 ? contentHashOverride : object.contentHash,
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
    localHash: string, localModifiedAtMs: number): boolean {
    if (localHash.length === 0 && local === null) {
      return true
    }
    if (localHash.length === 0) {
      return true
    }
    const localTime: number = local !== null && local.op === 'upsert' &&
      local.content_hash === localHash
      ? (local.updated_at_ms ?? localModifiedAtMs) : localModifiedAtMs
    if (object.modifiedAtMs > localTime) {
      return true
    }
    if (object.modifiedAtMs < localTime) {
      return false
    }
    const localDevice: string = local !== null && local.device_id !== undefined
      ? local.device_id : this.localDeviceId
    return object.deviceId >= localDevice
  }

  private writeConflict(object: StagedPeerObject, target: LocalObjectTarget, action: string,
    localHash: string, baseHash: string, descriptionOverride: string = ''): void {
    const paths: SyncStatePaths = this.syncStatePaths(target)
    const kind: string = action === 'delete' ? 'remote_deleted' : 'both_changed'
    let remoteSnapshotPath: string | null = null
    if (kind === 'both_changed') {
      fs.mkdirSync(paths.conflictDir, true)
      const baseName: string = target.relativePath.replace(/[\\/]/g, '_')
      const snapshotPath: string =
        `${paths.conflictDir}/peer-${Date.now()}-${baseName}.remote-conflict`
      this.atomicWrite(snapshotPath, object.bytes)
      remoteSnapshotPath = this.relativeToSyncRoot(snapshotPath, paths.syncRoot)
    }
    const description: string = descriptionOverride.length > 0 ? descriptionOverride
      : (kind === 'remote_deleted'
        ? `附近设备同步：远端已删除 ${target.relativePath}，本地仍有修改`
        : `附近设备同步：本地和远端都修改了 ${target.relativePath}`)
    const record: SyncConflictRecord = {
      local_path: target.relativePath,
      remote_path: object.path,
      local_hash: localHash,
      remote_hash: object.contentHash,
      base_hash: baseHash,
      created_at: Math.floor(Date.now() / 1000),
      description: description,
      kind: kind,
      remote_snapshot_path: remoteSnapshotPath,
      remote_graph_snapshot_dir: null
    }
    // 与 Core 一样，conflicts.json 是冲突页读取的 canonical record；
    // state.conflicts / conflicted_files 是同步引擎镜像，二者同路径去重。
    const conflicts: SyncConflictRecord[] = this.readConflictRecords(paths.conflictsPath)
    this.upsertConflictRecord(conflicts, record)
    const state: SyncStateFile = this.readSyncState(paths.statePath)
    if (state.device_id === undefined || state.device_id.length === 0) {
      state.device_id = this.localDeviceId
    }
    const conflictedFiles: string[] = state.conflicted_files ?? []
    if (!conflictedFiles.includes(record.local_path)) {
      conflictedFiles.push(record.local_path)
    }
    state.conflicted_files = conflictedFiles
    const stateConflicts: SyncConflictRecord[] = state.conflicts ?? []
    this.upsertConflictRecord(stateConflicts, record)
    state.conflicts = stateConflicts
    // 先写 canonical conflicts.json，再写 state mirror；任一步失败都让提交返回失败，
    // 发送端不会收到“已同步”的 ACK。
    this.atomicWrite(paths.conflictsPath, PeerCrypto.utf8Encode(JSON.stringify(conflicts)))
    this.atomicWrite(paths.statePath, PeerCrypto.utf8Encode(JSON.stringify(state)))
  }

  private syncStatePaths(target: LocalObjectTarget): SyncStatePaths {
    const syncRoot: string = target.projectScoped
      ? `${this.root}/projects/${this.projectIdForTarget(target)}`
      : this.root
    const syncDir: string =
      target.manifestPath.substring(0, target.manifestPath.lastIndexOf('/'))
    return {
      statePath: `${syncDir}/state.local.json`,
      conflictsPath: `${syncDir}/conflicts.json`,
      conflictDir: `${syncDir}/conflicts`,
      syncRoot: syncRoot
    }
  }

  private relativeToSyncRoot(absolutePath: string, syncRoot: string): string {
    const prefix: string = `${syncRoot}/`
    return absolutePath.startsWith(prefix)
      ? absolutePath.substring(prefix.length).replace(/\\/g, '/')
      : absolutePath.replace(/\\/g, '/')
  }

  private readConflictRecords(path: string): SyncConflictRecord[] {
    if (!fs.accessSync(path)) {
      return []
    }
    const text: string = PeerCrypto.utf8Decode(this.readBytes(path))
    const parsed: SyncConflictRecord[] = JSON.parse(text) as SyncConflictRecord[]
    if (!Array.isArray(parsed)) {
      throw new Error(`冲突记录格式错误：${path}`)
    }
    return parsed
  }

  private readSyncState(path: string): SyncStateFile {
    if (!fs.accessSync(path)) {
      return { device_id: this.localDeviceId, conflicted_files: [], conflicts: [] }
    }
    const text: string = PeerCrypto.utf8Decode(this.readBytes(path))
    const parsed: SyncStateFile = JSON.parse(text) as SyncStateFile
    if (parsed === null || Array.isArray(parsed)) {
      throw new Error(`同步状态格式错误：${path}`)
    }
    return parsed
  }

  private upsertConflictRecord(records: SyncConflictRecord[], record: SyncConflictRecord): void {
    for (let i = 0; i < records.length; i++) {
      if (records[i].local_path === record.local_path) {
        records[i] = record
        return
      }
    }
    records.push(record)
  }

  private projectIdForTarget(target: LocalObjectTarget): string {
    const marker: string = `${this.root}/projects/`
    const rest: string = target.absolutePath.substring(marker.length)
    const slash: number = rest.indexOf('/')
    return slash < 0 ? rest : rest.substring(0, slash)
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
