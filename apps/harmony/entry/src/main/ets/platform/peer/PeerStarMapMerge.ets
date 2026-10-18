// PeerStarMapMerge — 附近设备同步的星图索引 / 图元语义合并（Issue #988）。
//
// 为什么不能走普通文件的 mtime LWW：
// - `starmaps/index.json` 的 starmapIds / rootStarmapIds / mainStarmapByProject 是集合与映射，
//   一端新增 A、另一端新增 B 时，整文件谁新谁赢会直接丢掉另一端的星图登记；
// - `starmaps/<id>/graph.json` 是 GraphMeta，成员 ID 列表、revision、派生索引都必须按稳定 ID
//   合并，否则一张图的新增节点会被另一台的旧 graph.json 覆盖，而对象文件还在磁盘上。
//
// 这里只做“稳定 ID 合并 + 同 ID 保留本机”，不解析节点正文、不访问文件系统；
// 对象级三方冲突仍由 PeerObjectStore 记录到现有 conflicts.json，不新增冲突格式。

import { PeerCrypto } from './PeerCrypto'

export interface PeerStarMapMergeResult {
  bytes: Uint8Array
  changed: boolean
  conflict: boolean
  description: string
}

interface PeerStarMapIndexRecord {
  schemaVersion?: number
  starmapIds?: string[]
  rootStarmapIds?: string[]
  mainStarmapByProject?: Record<string, string>
  updatedAt?: number
}

interface PeerEdgeRelationEntry {
  edgeId?: string
}

interface PeerEmbedHostEntry {
  instanceId?: string
}

interface PeerLinkRelationEntry {
  linkId?: string
}

interface PeerHyperlinkRelationEntry {
  hyperlinkId?: string
}

interface PeerDeletionEntry {
  objectType?: string
  objectId?: string
  deletedAtRevision?: number
}

interface PeerDeletedSinceLastSync {
  entries?: PeerDeletionEntry[]
}

interface PeerStarMapGraphMeta {
  schemaVersion?: string
  starmapId?: string
  nodeIds?: string[]
  edgeIds?: string[]
  embedInstanceIds?: string[]
  linkIds?: string[]
  hyperlinkIds?: string[]
  edgeRelationIndex?: PeerEdgeRelationEntry[]
  embedHostIndex?: PeerEmbedHostEntry[]
  linkRelationIndex?: PeerLinkRelationEntry[]
  hyperlinkRelationIndex?: PeerHyperlinkRelationEntry[]
  nodeKindCounts?: Record<string, number>
  nodeRevisions?: Record<string, number>
  edgeRevisions?: Record<string, number>
  embedRevisions?: Record<string, number>
  linkRevisions?: Record<string, number>
  hyperlinkRevisions?: Record<string, number>
  packageRevision?: number
  updatedAt?: number
  deletedSinceLastSync?: PeerDeletedSinceLastSync
}

function unionIds(local: string[] | undefined, remote: string[] | undefined): string[] {
  const merged: string[] = []
  const seen: Set<string> = new Set<string>()
  const add = (values: string[] | undefined): void => {
    if (values === undefined) {
      return
    }
    values.forEach((value: string): void => {
      if (value.length > 0 && !seen.has(value)) {
        seen.add(value)
        merged.push(value)
      }
    })
  }
  add(local)
  add(remote)
  return merged
}

function maxNumber(left: number | undefined, right: number | undefined): number {
  return Math.max(left ?? 0, right ?? 0)
}

function maxVersion(left: string | undefined, right: string | undefined): string {
  const leftNumber: number = Number(left ?? '0')
  const rightNumber: number = Number(right ?? '0')
  if (Number.isNaN(leftNumber) || Number.isNaN(rightNumber)) {
    return (left ?? '') >= (right ?? '') ? (left ?? '') : (right ?? '')
  }
  return String(Math.max(leftNumber, rightNumber))
}

function mergeMapKeys(local: Record<string, string> | undefined,
  remote: Record<string, string> | undefined, allowRemote: boolean,
  onConflict: () => void): Record<string, string> {
  const merged: Record<string, string> = {}
  if (local !== undefined) {
    Object.keys(local).forEach((key: string): void => {
      merged[key] = local[key]
    })
  }
  if (remote !== undefined) {
    Object.keys(remote).forEach((key: string): void => {
      const current: string | undefined = merged[key]
      if (current === undefined) {
        if (allowRemote) {
          merged[key] = remote[key]
        }
      } else if (current !== remote[key]) {
        // 同 key 不同值：保留本机映射，并让调用方记录结构化冲突。
        onConflict()
      }
    })
  }
  return merged
}

function mergeNumberMap(local: Record<string, number> | undefined,
  remote: Record<string, number> | undefined): Record<string, number> {
  const merged: Record<string, number> = {}
  if (local !== undefined) {
    Object.keys(local).forEach((key: string): void => {
      merged[key] = local[key]
    })
  }
  if (remote !== undefined) {
    Object.keys(remote).forEach((key: string): void => {
      const current: number | undefined = merged[key]
      merged[key] = current === undefined ? remote[key] : Math.max(current, remote[key])
    })
  }
  return merged
}

function mergeIndex(local: PeerStarMapIndexRecord,
  remote: PeerStarMapIndexRecord): PeerStarMapMergeResult {
  let conflict: boolean = false
  const starmapIds: string[] = unionIds(local.starmapIds, remote.starmapIds)
  const existing: Set<string> = new Set<string>(starmapIds)
  const rootStarmapIds: string[] = unionIds(local.rootStarmapIds, remote.rootStarmapIds)
    .filter((id: string): boolean => existing.has(id))
  // 普通星图新增映射允许直接吸收；同 project 指向不同 starmap 时保留本机，
  // 由 PeerObjectStore 记录冲突，不擅自修改项目当前主图。
  const main: Record<string, string> = mergeMapKeys(local.mainStarmapByProject,
    remote.mainStarmapByProject, true, (): void => {
      conflict = true
    })
  const mainFiltered: Record<string, string> = {}
  Object.keys(main).forEach((projectId: string): void => {
    const starmapId: string = main[projectId]
    if (existing.has(starmapId)) {
      mainFiltered[projectId] = starmapId
    }
  })
  const merged: PeerStarMapIndexRecord = {
    schemaVersion: local.schemaVersion,
    starmapIds: local.starmapIds,
    rootStarmapIds: local.rootStarmapIds,
    mainStarmapByProject: local.mainStarmapByProject,
    updatedAt: local.updatedAt
  }
  merged.schemaVersion = maxNumber(local.schemaVersion, remote.schemaVersion)
  merged.starmapIds = starmapIds
  merged.rootStarmapIds = rootStarmapIds
  merged.mainStarmapByProject = mainFiltered
  merged.updatedAt = maxNumber(local.updatedAt, remote.updatedAt)
  return finalize(local, merged, conflict, conflict
    ? '附近设备星图索引中同一项目的默认星图不同，已保留本机映射'
    : '')
}

function mergeRelationArray<T>(local: T[] | undefined,
  remote: T[] | undefined, keyOf: (entry: T) => string): T[] {
  const merged: T[] = local === undefined ? [] : local.slice()
  const indexes: Map<string, number> = new Map<string, number>()
  merged.forEach((entry: T, index: number): void => {
    indexes.set(keyOf(entry), index)
  })
  if (remote !== undefined) {
    remote.forEach((entry: T): void => {
      const key: string = keyOf(entry)
      const index: number | undefined = indexes.get(key)
      if (index === undefined) {
        indexes.set(key, merged.length)
        merged.push(entry)
      }
    })
  }
  return merged
}

function relationKey(entry: PeerEdgeRelationEntry): string {
  return entry.edgeId ?? ''
}

function embedHostKey(entry: PeerEmbedHostEntry): string {
  return entry.instanceId ?? ''
}

function linkKey(entry: PeerLinkRelationEntry): string {
  return entry.linkId ?? ''
}

function hyperlinkKey(entry: PeerHyperlinkRelationEntry): string {
  return entry.hyperlinkId ?? ''
}

function mergeDeletions(local: PeerDeletedSinceLastSync | undefined,
  remote: PeerDeletedSinceLastSync | undefined): PeerDeletedSinceLastSync {
  const merged: PeerDeletionEntry[] = local?.entries === undefined ? [] : local.entries.slice()
  const indexes: Map<string, number> = new Map<string, number>()
  merged.forEach((entry: PeerDeletionEntry, index: number): void => {
    indexes.set(`${entry.objectType ?? ''}\u0000${entry.objectId ?? ''}`, index)
  })
  remote?.entries?.forEach((entry: PeerDeletionEntry): void => {
    const key: string = `${entry.objectType ?? ''}\u0000${entry.objectId ?? ''}`
    const index: number | undefined = indexes.get(key)
    if (index === undefined) {
      indexes.set(key, merged.length)
      merged.push(entry)
      return
    }
    const current: PeerDeletionEntry = merged[index]
    const revision: number = Math.max(current.deletedAtRevision ?? 0,
      entry.deletedAtRevision ?? 0)
    merged[index] = {
      objectType: current.objectType,
      objectId: current.objectId,
      deletedAtRevision: revision
    }
  })
  return { entries: merged }
}

function mergeGraph(local: PeerStarMapGraphMeta,
  remote: PeerStarMapGraphMeta): PeerStarMapMergeResult {
  let conflict: boolean = false
  if (local.starmapId !== undefined && remote.starmapId !== undefined &&
    local.starmapId !== remote.starmapId) {
    // graph.json 路径已经是稳定 starmapId；文件内 id 不一致说明对象身份已损坏。
    // 整图保留本机，不把另一张图的成员合并进来。
    const localBytes: Uint8Array = PeerCrypto.utf8Encode(JSON.stringify(local))
    return {
      bytes: localBytes,
      changed: false,
      conflict: true,
      description: '附近设备星图图元标识不一致，已保留本机图'
    }
  }
  const merged: PeerStarMapGraphMeta = {
    schemaVersion: local.schemaVersion,
    starmapId: local.starmapId,
    nodeIds: local.nodeIds,
    edgeIds: local.edgeIds,
    embedInstanceIds: local.embedInstanceIds,
    linkIds: local.linkIds,
    hyperlinkIds: local.hyperlinkIds,
    edgeRelationIndex: local.edgeRelationIndex,
    embedHostIndex: local.embedHostIndex,
    linkRelationIndex: local.linkRelationIndex,
    hyperlinkRelationIndex: local.hyperlinkRelationIndex,
    nodeKindCounts: local.nodeKindCounts,
    nodeRevisions: local.nodeRevisions,
    edgeRevisions: local.edgeRevisions,
    embedRevisions: local.embedRevisions,
    linkRevisions: local.linkRevisions,
    hyperlinkRevisions: local.hyperlinkRevisions,
    packageRevision: local.packageRevision,
    updatedAt: local.updatedAt,
    deletedSinceLastSync: local.deletedSinceLastSync
  }
  if (local.starmapId !== undefined && remote.starmapId !== undefined &&
    local.starmapId !== remote.starmapId) {
    conflict = true
  }
  if (merged.starmapId === undefined) {
    merged.starmapId = remote.starmapId
  }
  merged.schemaVersion = maxVersion(local.schemaVersion, remote.schemaVersion)
  merged.nodeIds = unionIds(local.nodeIds, remote.nodeIds)
  merged.edgeIds = unionIds(local.edgeIds, remote.edgeIds)
  merged.embedInstanceIds = unionIds(local.embedInstanceIds, remote.embedInstanceIds)
  merged.linkIds = unionIds(local.linkIds, remote.linkIds)
  merged.hyperlinkIds = unionIds(local.hyperlinkIds, remote.hyperlinkIds)
  merged.edgeRelationIndex = mergeRelationArray(local.edgeRelationIndex,
    remote.edgeRelationIndex, relationKey)
  merged.embedHostIndex = mergeRelationArray(local.embedHostIndex,
    remote.embedHostIndex, embedHostKey)
  merged.linkRelationIndex = mergeRelationArray(local.linkRelationIndex,
    remote.linkRelationIndex, linkKey)
  merged.hyperlinkRelationIndex = mergeRelationArray(local.hyperlinkRelationIndex,
    remote.hyperlinkRelationIndex, hyperlinkKey)
  merged.nodeKindCounts = mergeNumberMap(local.nodeKindCounts, remote.nodeKindCounts)
  merged.nodeRevisions = mergeNumberMap(local.nodeRevisions, remote.nodeRevisions)
  merged.edgeRevisions = mergeNumberMap(local.edgeRevisions, remote.edgeRevisions)
  merged.embedRevisions = mergeNumberMap(local.embedRevisions, remote.embedRevisions)
  merged.linkRevisions = mergeNumberMap(local.linkRevisions, remote.linkRevisions)
  merged.hyperlinkRevisions = mergeNumberMap(local.hyperlinkRevisions,
    remote.hyperlinkRevisions)
  merged.packageRevision = maxNumber(local.packageRevision, remote.packageRevision)
  merged.updatedAt = maxNumber(local.updatedAt, remote.updatedAt)
  merged.deletedSinceLastSync = mergeDeletions(local.deletedSinceLastSync,
    remote.deletedSinceLastSync)
  return finalize(local, merged, conflict, conflict
    ? '附近设备星图图元标识不一致，已保留本机图'
    : '')
}

function finalize(local: Object, merged: Object, conflict: boolean,
  description: string): PeerStarMapMergeResult {
  const text: string = JSON.stringify(merged)
  const bytes: Uint8Array = PeerCrypto.utf8Encode(text)
  const localText: string = JSON.stringify(local)
  return {
    bytes: bytes,
    changed: text !== localText,
    conflict: conflict,
    description: description
  }
}

export class PeerStarMapMerge {
  static isSemanticPath(path: string): boolean {
    return path === 'app/starmaps/index.json' ||
      /^app\/starmaps\/[^/]+\/graph\.json$/.test(path)
  }

  static merge(path: string, localBytes: Uint8Array,
    remoteBytes: Uint8Array): PeerStarMapMergeResult {
    const localText: string = PeerCrypto.utf8Decode(localBytes)
    const remoteText: string = PeerCrypto.utf8Decode(remoteBytes)
    if (path === 'app/starmaps/index.json') {
      return mergeIndex(JSON.parse(localText) as PeerStarMapIndexRecord,
        JSON.parse(remoteText) as PeerStarMapIndexRecord)
    }
    return mergeGraph(JSON.parse(localText) as PeerStarMapGraphMeta,
      JSON.parse(remoteText) as PeerStarMapGraphMeta)
  }
}
