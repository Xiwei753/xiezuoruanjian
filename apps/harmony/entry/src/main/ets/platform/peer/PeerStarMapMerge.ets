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

interface PeerMemberMergeResult {
  ids: string[]
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
  edgeRelationIndex?: PeerEdgeRelationEntry[]
  embedHostIndex?: PeerEmbedHostEntry[]
  linkRelationIndex?: PeerLinkRelationEntry[]
  nodeKindCounts?: Record<string, number>
  nodeRevisions?: Record<string, number>
  edgeRevisions?: Record<string, number>
  embedRevisions?: Record<string, number>
  linkRevisions?: Record<string, number>
  packageRevision?: number
  updatedAt?: number
  deletedSinceLastSync?: PeerDeletedSinceLastSync
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

// 索引成员合并：对没有明确删除事实的独立 ID 做并集，不能以整个索引文件的
// updatedAt 判定某个成员已被删。三路判定需要共同基线的成员清单：
// - baseIds 提供：base 有而一端没有 → 另一端明确删除，接受删除；
//   base 没有 → 该端新增，保留。
// - baseIds 无法获取但有基线（hasBase）：无法判定新增 vs 删除，保留并报告冲突。
// - 无基线（首次同步）：两端各自新增，并集保留，不报冲突。
function mergeIndexIds(local: string[] | undefined, remote: string[] | undefined,
  baseIds: Set<string> | undefined, hasBase: boolean,
  onConflict: () => void): string[] {
  const localSet: Set<string> = new Set<string>(local ?? [])
  const remoteSet: Set<string> = new Set<string>(remote ?? [])
  const allIds: Set<string> = new Set<string>()
  ;(local ?? []).forEach((id: string): void => {
    if (id.length > 0) {
      allIds.add(id)
    }
  })
  ;(remote ?? []).forEach((id: string): void => {
    if (id.length > 0) {
      allIds.add(id)
    }
  })
  const merged: string[] = []
  const seen: Set<string> = new Set<string>()
  const append = (id: string): void => {
    if (!seen.has(id)) {
      seen.add(id)
      merged.push(id)
    }
  }
  allIds.forEach((id: string): void => {
    const localHas: boolean = localSet.has(id)
    const remoteHas: boolean = remoteSet.has(id)
    if (localHas && remoteHas) {
      append(id)
      return
    }
    if (baseIds !== undefined) {
      if (baseIds.has(id)) {
        return
      }
      append(id)
      return
    }
    if (hasBase) {
      onConflict()
    }
    append(id)
  })
  return merged
}

// 项目主图映射合并：同样不能用整索引 updatedAt 判定单端映射被删。
// 三路判定与 mergeIndexIds 一致：base 有而一端没有 → 另一端删除；
// 无 base 内容但有基线 → 保留并报告冲突；无基线 → 新增保留。
function mergeProjectMap(local: Record<string, string> | undefined,
  remote: Record<string, string> | undefined,
  baseMap: Record<string, string> | undefined, hasBase: boolean,
  onConflict: () => void): Record<string, string> {
  const keys: Set<string> = new Set<string>()
  if (local !== undefined) {
    Object.keys(local).forEach((key: string): void => { keys.add(key) })
  }
  if (remote !== undefined) {
    Object.keys(remote).forEach((key: string): void => { keys.add(key) })
  }
  const merged: Record<string, string> = {}
  keys.forEach((projectId: string): void => {
    const localId: string | undefined = local === undefined ? undefined : local[projectId]
    const remoteId: string | undefined = remote === undefined ? undefined : remote[projectId]
    if (localId !== undefined && remoteId !== undefined) {
      if (localId !== remoteId) {
        onConflict()
        merged[projectId] = localId
      } else {
        merged[projectId] = localId
      }
      return
    }
    if (localId !== undefined) {
      if (baseMap !== undefined) {
        if (baseMap[projectId] !== undefined) {
          return
        }
        merged[projectId] = localId
        return
      }
      if (hasBase) {
        onConflict()
      }
      merged[projectId] = localId
      return
    }
    if (baseMap !== undefined) {
      if (baseMap[projectId] !== undefined) {
        return
      }
      merged[projectId] = remoteId as string
      return
    }
    if (hasBase) {
      onConflict()
    }
    merged[projectId] = remoteId as string
  })
  return merged
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

function filterRelationArray<T>(entries: T[] | undefined, ids: Set<string>,
  keyOf: (entry: T) => string): T[] {
  return (entries ?? []).filter((entry: T): boolean => ids.has(keyOf(entry)))
}

function filterNumberMap(entries: Record<string, number> | undefined,
  ids: Set<string>): Record<string, number> {
  const filtered: Record<string, number> = {}
  if (entries !== undefined) {
    Object.keys(entries).forEach((key: string): void => {
      if (ids.has(key)) {
        filtered[key] = entries[key]
      }
    })
  }
  return filtered
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
  remote: PeerStarMapIndexRecord, base: PeerStarMapIndexRecord | undefined,
  hasBase: boolean): PeerStarMapMergeResult {
  let conflict: boolean = false
  const baseIds: Set<string> | undefined = base === undefined ? undefined
    : new Set<string>(base.starmapIds ?? [])
  const starmapIds: string[] = mergeIndexIds(local.starmapIds, remote.starmapIds,
    baseIds, hasBase, (): void => { conflict = true })
  const existing: Set<string> = new Set<string>(starmapIds)
  const baseRootIds: Set<string> | undefined = base === undefined ? undefined
    : new Set<string>(base.rootStarmapIds ?? [])
  const rootStarmapIds: string[] = mergeIndexIds(local.rootStarmapIds,
    remote.rootStarmapIds, baseRootIds, hasBase, (): void => { conflict = true })
    .filter((id: string): boolean => existing.has(id))
  // 普通星图新增映射允许直接吸收；同 project 指向不同 starmap 时保留本机，
  // 由 PeerObjectStore 记录冲突，不擅自修改项目当前主图。
  const baseMap: Record<string, string> | undefined = base === undefined
    ? undefined : base.mainStarmapByProject
  const main: Record<string, string> = mergeProjectMap(local.mainStarmapByProject,
    remote.mainStarmapByProject, baseMap, hasBase, (): void => {
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
    ? '附近设备星图索引合并存在无法判定的删除或主图冲突，已保留本机'
    : '')
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

function readDeletionRevision(tombstones: Map<string, number>, id: string): number {
  const revision: number | undefined = tombstones.get(id)
  return revision === undefined ? -1 : revision
}

function collectTombstones(local: PeerDeletedSinceLastSync | undefined,
  remote: PeerDeletedSinceLastSync | undefined, objectType: string): Map<string, number> {
  const tombstones: Map<string, number> = new Map<string, number>()
  const collect = (entries: PeerDeletionEntry[] | undefined): void => {
    (entries ?? []).forEach((entry: PeerDeletionEntry): void => {
      if (entry.objectType !== objectType || entry.objectId === undefined ||
        entry.objectId.length === 0) {
        return
      }
      const current: number = tombstones.get(entry.objectId) ?? -1
      tombstones.set(entry.objectId, Math.max(current, entry.deletedAtRevision ?? 0))
    })
  }
  collect(local?.entries)
  collect(remote?.entries)
  return tombstones
}

function readRevision(revisions: Record<string, number> | undefined, id: string): number {
  const revision: number | undefined = revisions === undefined ? undefined : revisions[id]
  return revision === undefined ? 0 : revision
}

function mergeMembersWithDeletions(localIds: string[] | undefined,
  remoteIds: string[] | undefined, localRevisions: Record<string, number> | undefined,
  remoteRevisions: Record<string, number> | undefined, objectType: string,
  localDeletions: PeerDeletedSinceLastSync | undefined,
  remoteDeletions: PeerDeletedSinceLastSync | undefined): PeerMemberMergeResult {
  const localSet: Set<string> = new Set<string>(localIds ?? [])
  const remoteSet: Set<string> = new Set<string>(remoteIds ?? [])
  const tombstones: Map<string, number> = collectTombstones(localDeletions, remoteDeletions,
    objectType)
  const ids: string[] = []
  const seen: Set<string> = new Set<string>()
  let conflict: boolean = false
  const append = (id: string): void => {
    if (!seen.has(id)) {
      seen.add(id)
      ids.push(id)
    }
  }
  const visit = (id: string): void => {
    if (id.length === 0) {
      return
    }
    const localHas: boolean = localSet.has(id)
    const remoteHas: boolean = remoteSet.has(id)
    const deletedAt: number = readDeletionRevision(tombstones, id)
    if (deletedAt < 0) {
      if (localHas || remoteHas) {
        append(id)
      }
      return
    }
    if (localHas && remoteHas) {
      // 两边仍保留成员：说明删除事实已过期，成员继续存在，稍后过滤墓碑。
      append(id)
      return
    }
    if (localHas) {
      const localRevision: number = readRevision(localRevisions, id)
      if (localRevision <= deletedAt) {
        // 本机没有在删除之后修改过：接受远端删除，不复活成员。
        return
      }
      // 删除与后续修改冲突：保留当前图成员并记录冲突。
      append(id)
      conflict = true
      return
    }
    const remoteRevision: number = readRevision(remoteRevisions, id)
    if (remoteRevision <= deletedAt) {
      // 远端成员没有在删除之后修改过：接受本机删除。
      return
    }
    // 远端在删除后修改过：保留当前图（本机无该成员），记录冲突，不擅自复活。
    conflict = true
  }
  ;(localIds ?? []).forEach(visit)
  ;(remoteIds ?? []).forEach(visit)
  return {
    ids: ids,
    conflict: conflict,
    description: conflict ? '附近设备星图对象删除与修改冲突，已保留本机图' : ''
  }
}

function filterDeletions(deletions: PeerDeletedSinceLastSync,
  nodeIds: Set<string>, edgeIds: Set<string>, embedIds: Set<string>,
  linkIds: Set<string>): PeerDeletedSinceLastSync {
  const filtered: PeerDeletionEntry[] = []
  ;(deletions.entries ?? []).forEach((entry: PeerDeletionEntry): void => {
    const id: string = entry.objectId ?? ''
    let exists: boolean = false
    if (entry.objectType === 'node') {
      exists = nodeIds.has(id)
    } else if (entry.objectType === 'edge') {
      exists = edgeIds.has(id)
    } else if (entry.objectType === 'embed') {
      exists = embedIds.has(id)
    } else if (entry.objectType === 'link') {
      exists = linkIds.has(id)
    }
    if (!exists) {
      filtered.push(entry)
    }
  })
  return { entries: filtered }
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
  const nodeMerge: PeerMemberMergeResult = mergeMembersWithDeletions(local.nodeIds,
    remote.nodeIds, local.nodeRevisions, remote.nodeRevisions, 'node',
    local.deletedSinceLastSync, remote.deletedSinceLastSync)
  const edgeMerge: PeerMemberMergeResult = mergeMembersWithDeletions(local.edgeIds,
    remote.edgeIds, local.edgeRevisions, remote.edgeRevisions, 'edge',
    local.deletedSinceLastSync, remote.deletedSinceLastSync)
  const embedMerge: PeerMemberMergeResult = mergeMembersWithDeletions(local.embedInstanceIds,
    remote.embedInstanceIds, local.embedRevisions, remote.embedRevisions, 'embed',
    local.deletedSinceLastSync, remote.deletedSinceLastSync)
  const linkMerge: PeerMemberMergeResult = mergeMembersWithDeletions(local.linkIds,
    remote.linkIds, local.linkRevisions, remote.linkRevisions, 'link',
    local.deletedSinceLastSync, remote.deletedSinceLastSync)
  const nodeSet: Set<string> = new Set<string>(nodeMerge.ids)
  const edgeSet: Set<string> = new Set<string>(edgeMerge.ids)
  const embedSet: Set<string> = new Set<string>(embedMerge.ids)
  const linkSet: Set<string> = new Set<string>(linkMerge.ids)
  conflict = nodeMerge.conflict || edgeMerge.conflict || embedMerge.conflict ||
    linkMerge.conflict
  let description: string = ''
  const descriptions: string[] = [
    nodeMerge.description,
    edgeMerge.description,
    embedMerge.description,
    linkMerge.description
  ]
  descriptions.forEach((text: string): void => {
    if (description.length === 0 && text.length > 0) {
      description = text
    }
  })
  const merged: PeerStarMapGraphMeta = {
    schemaVersion: local.schemaVersion,
    starmapId: local.starmapId,
    nodeIds: local.nodeIds,
    edgeIds: local.edgeIds,
    embedInstanceIds: local.embedInstanceIds,
    linkIds: local.linkIds,
    edgeRelationIndex: local.edgeRelationIndex,
    embedHostIndex: local.embedHostIndex,
    linkRelationIndex: local.linkRelationIndex,
    nodeKindCounts: local.nodeKindCounts,
    nodeRevisions: local.nodeRevisions,
    edgeRevisions: local.edgeRevisions,
    embedRevisions: local.embedRevisions,
    linkRevisions: local.linkRevisions,
    packageRevision: local.packageRevision,
    updatedAt: local.updatedAt,
    deletedSinceLastSync: local.deletedSinceLastSync
  }
  if (merged.starmapId === undefined) {
    merged.starmapId = remote.starmapId
  }
  merged.schemaVersion = maxVersion(local.schemaVersion, remote.schemaVersion)
  merged.nodeIds = nodeMerge.ids
  merged.edgeIds = edgeMerge.ids
  merged.embedInstanceIds = embedMerge.ids
  merged.linkIds = linkMerge.ids
  merged.edgeRelationIndex = filterRelationArray(mergeRelationArray(
    local.edgeRelationIndex, remote.edgeRelationIndex, relationKey), edgeSet, relationKey)
  merged.embedHostIndex = filterRelationArray(mergeRelationArray(
    local.embedHostIndex, remote.embedHostIndex, embedHostKey), embedSet, embedHostKey)
  merged.linkRelationIndex = filterRelationArray(mergeRelationArray(
    local.linkRelationIndex, remote.linkRelationIndex, linkKey), linkSet, linkKey)
  merged.nodeKindCounts = mergeNumberMap(local.nodeKindCounts, remote.nodeKindCounts)
  merged.nodeRevisions = filterNumberMap(mergeNumberMap(local.nodeRevisions,
    remote.nodeRevisions), nodeSet)
  merged.edgeRevisions = filterNumberMap(mergeNumberMap(local.edgeRevisions,
    remote.edgeRevisions), edgeSet)
  merged.embedRevisions = filterNumberMap(mergeNumberMap(local.embedRevisions,
    remote.embedRevisions), embedSet)
  merged.linkRevisions = filterNumberMap(mergeNumberMap(local.linkRevisions,
    remote.linkRevisions), linkSet)
  merged.packageRevision = maxNumber(local.packageRevision, remote.packageRevision)
  merged.updatedAt = maxNumber(local.updatedAt, remote.updatedAt)
  merged.deletedSinceLastSync = filterDeletions(mergeDeletions(local.deletedSinceLastSync,
    remote.deletedSinceLastSync), nodeSet, edgeSet, embedSet, linkSet)
  return finalize(local, merged, conflict, description)
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
    remoteBytes: Uint8Array, baseBytes?: Uint8Array,
    hasBase: boolean = false): PeerStarMapMergeResult {
    const localText: string = PeerCrypto.utf8Decode(localBytes)
    const remoteText: string = PeerCrypto.utf8Decode(remoteBytes)
    if (path === 'app/starmaps/index.json') {
      const baseRecord: PeerStarMapIndexRecord | undefined = baseBytes === undefined
        ? undefined
        : JSON.parse(PeerCrypto.utf8Decode(baseBytes)) as PeerStarMapIndexRecord
      return mergeIndex(JSON.parse(localText) as PeerStarMapIndexRecord,
        JSON.parse(remoteText) as PeerStarMapIndexRecord, baseRecord, hasBase)
    }
    return mergeGraph(JSON.parse(localText) as PeerStarMapGraphMeta,
      JSON.parse(remoteText) as PeerStarMapGraphMeta)
  }
}
