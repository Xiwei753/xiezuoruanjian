import { ResultEnvelope } from '../dto/ResultEnvelope'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapPoint, StarMapTargetPath, StarMapPathSegment, StarMapResolvedPath, StarMapNodeContent, StarMapAnchor, StarMapPortal, StarMapNodeStyle, StarMapProvenance, CreateStarmapChildEmbedResult, StarMapCrossLayerConnectionPlan, StarMapNodeUpdatePatch, StarMapLibraryView, StarMapLibraryFolder, StarMapLibraryEntry, StarMapAssetReference } from '../dto/StarMapDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeStarMapMeta, decodeStarMapGraph, decodeStarMapNode, decodeStarMapEdge, decodeStarMapEmbed, decodeCreateStarmapChildEmbedResult, decodeArray, decodeStarMapLibraryView, decodeStarMapLibraryFolder, decodeStarMapLibraryEntry, decodeStarMapAssetReference } from '../codec/CoreWireDecoders'
import util from '@ohos.util'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

/**
 * StarMapNodePatchInputDto 请求体（#1060）：platform 只发要改的字段。
 *
 * 字段都是可选的，不传的字段用 undefined 表示；JSON.stringify 会省略 undefined，
 * Core 的 patch 语义是"省略 = 不改"。
 * - 内容编辑带 `expectedDefinitionRevision`，不带 `position`；
 * - `payload` / `portal` 的清空在 C ABI 里用 `clearPayload` / `clearPortal` 表达。
 */
interface StarMapNodePatchBody {
  title?: string
  kind?: string
  payload?: string
  clearPayload?: boolean
  content?: StarMapNodeContent
  anchors?: StarMapAnchor[]
  portal?: StarMapPortal
  clearPortal?: boolean
  style?: StarMapNodeStyle
  provenance?: StarMapProvenance
  expectedDefinitionRevision?: number
}

/** StarMapEmbedPatchInputDto 请求体：platform 只发要改的字段（position）。 */
interface StarMapEmbedPatchBody {
  position: StarMapPoint
}

/** StarMapEdgeDto 请求体：edge id / 时间戳由平台端生成。 */
interface StarMapEdgeAddBody {
  id: string
  from: StarMapTargetPath
  to: StarMapTargetPath
  kind: string
  label: string | null
  payload: string | null
  createdAt: number
  updatedAt: number
}

/** StarMapNodeDto 请求体：完整节点 JSON，由平台端构造（#1060：无 tags）。 */
interface StarMapNodeAddBody {
  id: string
  assetId: string
  title: string
  kind: string
  payload: string | null
  content: StarMapNodeContent
  anchors: StarMapAnchor[]
  portal: StarMapPortal | null
  position: StarMapPoint
  style: StarMapNodeStyle
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
  definitionRevision: number
}

/** StarMapEmbedDto 请求体：完整 Embed JSON。 */
interface StarMapEmbedAddBody {
  instanceId: string
  targetStarmapId: string
  label: string | null
  position: StarMapPoint
  hostPath: StarMapTargetPath
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
}

export class NativeStarMapBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListStarMaps()
      return this.core.decodeEnvelope<StarMapMeta[]>(jsonStr,
        (d: Object) => decodeArray<StarMapMeta>(d, decodeStarMapMeta, 'StarMapMeta'))
    } catch (e) { return nativeError<StarMapMeta[]>(e) }
  }

  async listRootStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListRootStarMaps()
      return this.core.decodeEnvelope<StarMapMeta[]>(jsonStr,
        (d: Object) => decodeArray<StarMapMeta>(d, decodeStarMapMeta, 'StarMapMeta'))
    } catch (e) { return nativeError<StarMapMeta[]>(e) }
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListStarMapsForProject(projectId)
      return this.core.decodeEnvelope<StarMapMeta[]>(jsonStr,
        (d: Object) => decodeArray<StarMapMeta>(d, decodeStarMapMeta, 'StarMapMeta'))
    } catch (e) { return nativeError<StarMapMeta[]>(e) }
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetStarMap(starmapId)
      return this.core.decodeEnvelope<StarMapMeta>(jsonStr, decodeStarMapMeta)
    } catch (e) { return nativeError<StarMapMeta>(e) }
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetStarMapGraph(starmapId)
      return this.core.decodeEnvelope<StarMapGraph>(jsonStr, decodeStarMapGraph)
    } catch (e) { return nativeError<StarMapGraph>(e) }
  }

  async closeStarMapStore(starmapId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCloseStarMapStore(starmapId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateStarMap(title, description)
      return this.core.decodeEnvelope<StarMapMeta>(jsonStr, decodeStarMapMeta)
    } catch (e) { return nativeError<StarMapMeta>(e) }
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteStarMap(starmapId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRenameStarMap(starmapId, newTitle)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  /**
   * 内容编辑（#1060）：只发共享素材本体字段 + expectedDefinitionRevision = node.definitionRevision。
   *
   * 位置属于画布实例，必须走 moveStarMapNode；带 position 的 patch 在这里直接拒绝，
   * 不把实例事实塞进素材命令。
   *
   * TODO(#1060)：C ABI 的 StarMapNodePatchInputDto 目前没有 trajectory 字段，
   * 拖动轨迹还不能从 Harmony 下发；等 Core 扩展该 DTO 后再接线。
   */
  async updateStarMapNodeDefinition(starmapId: string, nodeId: string, patch: StarMapNodeUpdatePatch): Promise<ResultEnvelope<StarMapNode>> {
    if (patch.position !== undefined) {
      const rejected: ResultEnvelope<StarMapNode> = {
        success: false,
        errorCode: 'INVALID_ARGUMENT',
        userMessage: 'updateStarMapNodeDefinition cannot change position; use moveStarMapNode'
      }
      return rejected
    }
    try {
      const body: StarMapNodePatchBody = {}
      if (patch.title !== undefined) {
        body.title = patch.title
      }
      if (patch.kind !== undefined) {
        body.kind = patch.kind
      }
      if (patch.payload !== undefined) {
        if (patch.payload === null) {
          body.clearPayload = true
        } else {
          body.payload = patch.payload
        }
      }
      if (patch.content !== undefined) {
        body.content = patch.content
      }
      if (patch.anchors !== undefined) {
        body.anchors = patch.anchors
      }
      if (patch.portal !== undefined) {
        if (patch.portal === null) {
          body.clearPortal = true
        } else {
          body.portal = patch.portal
        }
      }
      if (patch.style !== undefined) {
        body.style = patch.style
      }
      if (patch.provenance !== undefined) {
        body.provenance = patch.provenance
      }
      if (patch.expectedDefinitionRevision !== undefined) {
        body.expectedDefinitionRevision = patch.expectedDefinitionRevision
      }
      const jsonStr: string = this.core.getNativeModule().nativeUpdateStarMapNodeDefinition(
        starmapId, nodeId, JSON.stringify(body))
      return this.core.decodeEnvelope<StarMapNode>(jsonStr, decodeStarMapNode)
    } catch (e) { return nativeError<StarMapNode>(e) }
  }

  /**
   * 只移动（#1060）：position + expectedInstanceRevision = node.updatedAt。
   *
   * 共享内容 revision 不由本命令校验，也不会被本命令修改；版本不一致时 Core
   * 返回 Conflict，调用方必须重新读取节点后再提交。
   */
  async moveStarMapNode(starmapId: string, nodeId: string, position: StarMapPoint,
    expectedInstanceRevision: number): Promise<ResultEnvelope<StarMapNode>> {
    try {
      const positionJson: string = JSON.stringify({ x: position.x, y: position.y })
      const jsonStr: string = this.core.getNativeModule().nativeMoveStarMapNode(
        starmapId, nodeId, positionJson, String(expectedInstanceRevision))
      return this.core.decodeEnvelope<StarMapNode>(jsonStr, decodeStarMapNode)
    } catch (e) { return nativeError<StarMapNode>(e) }
  }

  async updateStarMapEmbed(starmapId: string, instanceId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>> {
    try {
      // StarMapEmbedPatchInputDto：只发 position
      const body: StarMapEmbedPatchBody = { position: position }
      const jsonStr: string = this.core.getNativeModule().nativeUpdateStarMapEmbed(starmapId, instanceId, JSON.stringify(body))
      return this.core.decodeEnvelope<StarMapEmbed>(jsonStr, decodeStarMapEmbed)
    } catch (e) { return nativeError<StarMapEmbed>(e) }
  }

  async addStarMapEdge(starmapId: string, from: StarMapTargetPath, to: StarMapTargetPath, kind: string, label: string | null): Promise<ResultEnvelope<StarMapEdge>> {
    try {
      const now = Date.now()
      const body: StarMapEdgeAddBody = {
        id: util.generateRandomUUID(false),
        from: from,
        to: to,
        kind: kind,
        label: label,
        payload: null,
        createdAt: now,
        updatedAt: now
      }
      const jsonStr: string = this.core.getNativeModule().nativeAddStarMapEdge(starmapId, JSON.stringify(body))
      return this.core.decodeEnvelope<StarMapEdge>(jsonStr, decodeStarMapEdge)
    } catch (e) { return nativeError<StarMapEdge>(e) }
  }

  async addStarMapNode(starmapId: string, title: string, kind: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>> {
    try {
      const now = Date.now()
      const nodeId = `n_${util.generateRandomUUID(false)}`
      const body: StarMapNodeAddBody = {
        id: nodeId,
        // 新节点的共享素材由 Core 创建：assetId 留空由 Core 回落到实例 id，
        // definitionRevision 从 0 起，由 Core 建库时写入。
        assetId: '',
        title: title,
        kind: kind,
        payload: null,
        content: { type: 'empty', summary: null, body: null, projectId: null, volumeId: null, chapterId: null, rangeStart: null, rangeEnd: null, entityType: null, entityId: null },
        anchors: [],
        portal: null,
        position: position,
        style: { fillColor: null },
        provenance: { source: 'Human', sourceId: null, generatedBy: null, promptId: null, reviewStatus: 'Accepted', createdFromAnchor: null },
        createdAt: now,
        updatedAt: now,
        definitionRevision: 0
      }
      const jsonStr: string = this.core.getNativeModule().nativeAddStarMapNode(starmapId, JSON.stringify(body), position.x, position.y)
      return this.core.decodeEnvelope<StarMapNode>(jsonStr, decodeStarMapNode)
    } catch (e) { return nativeError<StarMapNode>(e) }
  }

  async addStarMapEmbed(starmapId: string, targetStarmapId: string, label: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>> {
    try {
      const now = Date.now()
      const embedId = `em_${util.generateRandomUUID(false)}`
      const body: StarMapEmbedAddBody = {
        instanceId: embedId,
        targetStarmapId: targetStarmapId,
        label: label,
        position: position,
        hostPath: {
          starmapId: starmapId,
          segments: [],
          target: { type: 'starmap', nodeId: null, anchorId: null, projectId: null, volumeId: null, chapterId: null, rangeStart: null, rangeEnd: null, entityType: null, entityId: null }
        },
        provenance: { source: 'Human', sourceId: null, generatedBy: null, promptId: null, reviewStatus: 'Accepted', createdFromAnchor: null },
        createdAt: now,
        updatedAt: now
      }
      const jsonStr: string = this.core.getNativeModule().nativeAddStarMapEmbed(starmapId, JSON.stringify(body))
      return this.core.decodeEnvelope<StarMapEmbed>(jsonStr, decodeStarMapEmbed)
    } catch (e) { return nativeError<StarMapEmbed>(e) }
  }

  async createStarmapChildEmbed(hostStarmapId: string, title: string, position: StarMapPoint): Promise<ResultEnvelope<CreateStarmapChildEmbedResult>> {
    try {
      const positionJson: string = JSON.stringify({ x: position.x, y: position.y })
      const jsonStr: string = this.core.getNativeModule().nativeCreateStarMapChildEmbed(hostStarmapId, title, positionJson)
      return this.core.decodeEnvelope<CreateStarmapChildEmbedResult>(jsonStr, decodeCreateStarmapChildEmbedResult)
    } catch (e) { return nativeError<CreateStarmapChildEmbedResult>(e) }
  }

  // resolve_starmap_path：把 root + segments 交给 Core 逐段穿越，拿回真正落在的星图 ID。
  // StarMapResolvedPathDto 是单字段 DTO，这里手动校验 finalStarmapId 类型，避免依赖额外 decoder。
  async resolveStarMapPath(rootStarmapId: string, segments: StarMapPathSegment[]): Promise<ResultEnvelope<StarMapResolvedPath>> {
    try {
      const segmentsJson: string = JSON.stringify(segments)
      const jsonStr: string = this.core.getNativeModule().nativeResolveStarMapPath(rootStarmapId, segmentsJson)
      const raw = this.core.parseRawEnvelope(jsonStr)
      if (!raw.success) {
        const errResult: ResultEnvelope<StarMapResolvedPath> = {
          success: false,
          errorCode: raw.errorCode,
          userMessage: raw.userMessage
        }
        return errResult
      }
      if (raw.data === undefined || raw.data === null) {
        return this.core.schemaMismatch<StarMapResolvedPath>('data is null or undefined')
      }
      const data = raw.data as Record<string, Object>
      const finalStarmapId: Object | undefined = data['finalStarmapId']
      if (typeof finalStarmapId !== 'string' || finalStarmapId === '') {
        return this.core.schemaMismatch<StarMapResolvedPath>('finalStarmapId missing or not a non-empty string')
      }
      const result: ResultEnvelope<StarMapResolvedPath> = {
        success: true,
        data: { finalStarmapId: finalStarmapId }
      }
      return result
    } catch (e) { return nativeError<StarMapResolvedPath>(e) }
  }

  // plan_cross_layer_connection：两条从同一根星图出发的绝对端点路径交给 Core，
  // 由 Core 统一算出宿主星图、宿主 Scene 路径和两条相对宿主的端点路径。
  // 自环 / 不可解析 / 不同根星图由 Core 拒绝（返回失败 envelope），平台端不猜宿主。
  async planStarMapCrossLayerConnection(from: StarMapTargetPath, to: StarMapTargetPath): Promise<ResultEnvelope<StarMapCrossLayerConnectionPlan>> {
    try {
      const body: string = JSON.stringify({ from: from, to: to })
      const jsonStr: string = this.core.getNativeModule().nativePlanCrossLayerConnection(body)
      const raw = this.core.parseRawEnvelope(jsonStr)
      if (!raw.success) {
        const errResult: ResultEnvelope<StarMapCrossLayerConnectionPlan> = {
          success: false,
          errorCode: raw.errorCode,
          userMessage: raw.userMessage
        }
        return errResult
      }
      if (raw.data === undefined || raw.data === null) {
        return this.core.schemaMismatch<StarMapCrossLayerConnectionPlan>('data is null or undefined')
      }
      const data = raw.data as Record<string, Object>
      const hostStarmapId: Object | undefined = data['hostStarmapId']
      const hostSegments: Object | undefined = data['hostSegments']
      const fromPath: Object | undefined = data['from']
      const toPath: Object | undefined = data['to']
      if (typeof hostStarmapId !== 'string' || hostStarmapId === '' ||
        !Array.isArray(hostSegments) || fromPath === undefined || toPath === undefined) {
        return this.core.schemaMismatch<StarMapCrossLayerConnectionPlan>(
          'hostStarmapId/hostSegments/from/to missing or invalid')
      }
      const plan: StarMapCrossLayerConnectionPlan = {
        hostStarmapId: hostStarmapId,
        hostSegments: hostSegments as StarMapPathSegment[],
        from: fromPath as StarMapTargetPath,
        to: toPath as StarMapTargetPath
      }
      const result: ResultEnvelope<StarMapCrossLayerConnectionPlan> = {
        success: true,
        data: plan
      }
      return result
    } catch (e) {
      return nativeError<StarMapCrossLayerConnectionPlan>(e)
    }
  }

  // ─── 素材库（#1037）───
  // 库目录只保存归属与分类；加入画布产生引用实例，正文写回共享本体。
  async listStarMapLibrary(scopeJson: string): Promise<ResultEnvelope<StarMapLibraryView>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListStarMapLibrary(scopeJson)
      return this.core.decodeEnvelope<StarMapLibraryView>(jsonStr, decodeStarMapLibraryView)
    } catch (e) { return nativeError<StarMapLibraryView>(e) }
  }

  async createStarMapLibraryFolder(scopeJson: string, parentFolderId: string, name: string): Promise<ResultEnvelope<StarMapLibraryFolder>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateStarMapLibraryFolder(scopeJson, parentFolderId, name)
      return this.core.decodeEnvelope<StarMapLibraryFolder>(jsonStr, decodeStarMapLibraryFolder)
    } catch (e) { return nativeError<StarMapLibraryFolder>(e) }
  }

  async moveStarMapLibraryEntry(entryId: string, scopeJson: string, parentFolderId: string): Promise<ResultEnvelope<StarMapLibraryEntry>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeMoveStarMapLibraryEntry(entryId, scopeJson, parentFolderId)
      return this.core.decodeEnvelope<StarMapLibraryEntry>(jsonStr, decodeStarMapLibraryEntry)
    } catch (e) { return nativeError<StarMapLibraryEntry>(e) }
  }

  async addStarMapNodeReference(starmapId: string, assetId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeAddStarMapNodeReference(starmapId, assetId, position.x, position.y)
      return this.core.decodeEnvelope<StarMapNode>(jsonStr, decodeStarMapNode)
    } catch (e) { return nativeError<StarMapNode>(e) }
  }

  async addStarMapReference(hostStarmapId: string, targetStarmapId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeAddStarMapReference(hostStarmapId, targetStarmapId, position.x, position.y)
      return this.core.decodeEnvelope<StarMapEmbed>(jsonStr, decodeStarMapEmbed)
    } catch (e) { return nativeError<StarMapEmbed>(e) }
  }

  async copyStarMapLibraryAsset(targetJson: string, scopeJson: string, title: string): Promise<ResultEnvelope<string>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCopyStarMapLibraryAsset(targetJson, scopeJson, title)
      return this.core.parseEnvelope<string>(jsonStr)
    } catch (e) { return nativeError<string>(e) }
  }

  async listStarMapAssetReferences(targetJson: string): Promise<ResultEnvelope<StarMapAssetReference[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListStarMapAssetReferences(targetJson)
      return this.core.decodeEnvelope<StarMapAssetReference[]>(jsonStr,
        (d: Object) => decodeArray<StarMapAssetReference>(d, decodeStarMapAssetReference, 'StarMapAssetReference'))
    } catch (e) { return nativeError<StarMapAssetReference[]>(e) }
  }

  async deleteStarMapLibraryAsset(targetJson: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteStarMapLibraryAsset(targetJson)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async removeStarMapInstance(starmapId: string, instanceId: string, isEmbed: boolean): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRemoveStarMapInstance(starmapId, instanceId, isEmbed)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
