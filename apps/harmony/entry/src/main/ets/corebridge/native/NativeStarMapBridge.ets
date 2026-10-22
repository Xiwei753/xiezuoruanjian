import { ResultEnvelope } from '../dto/ResultEnvelope'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapPoint, StarMapTargetPath, StarMapPathSegment, StarMapResolvedPath, StarMapNodeContent, StarMapAnchor, StarMapPortal, StarMapNodeStyle, StarMapProvenance, CreateStarmapChildEmbedResult, StarMapCrossLayerConnectionPlan, StarMapNodeUpdatePatch } from '../dto/StarMapDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeStarMapMeta, decodeStarMapGraph, decodeStarMapNode, decodeStarMapEdge, decodeStarMapEmbed, decodeCreateStarmapChildEmbedResult, decodeArray } from '../codec/CoreWireDecoders'
import util from '@ohos.util'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

/**
 * StarMapNodePatchInputDto 请求体（#984）：platform 只发要改的字段。
 *
 * 字段都是可选的，不传的字段用 undefined 表示；JSON.stringify 会省略 undefined，
 * Core 的 patch 语义是"省略 = 不改"，所以不能拿 null 占位（null 表示要清空）。
 */
interface StarMapNodePatchBody {
  title?: string
  content?: StarMapNodeContent
  position?: StarMapPoint
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

/** StarMapNodeDto 请求体：完整节点 JSON，由平台端构造。 */
interface StarMapNodeAddBody {
  id: string
  title: string
  kind: string
  payload: string | null
  tags: string[]
  content: StarMapNodeContent
  anchors: StarMapAnchor[]
  portal: StarMapPortal | null
  position: StarMapPoint
  style: StarMapNodeStyle
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
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
   * 更新节点（#984）：按 patch 只发 title / content / position 里实际要改的字段。
   * 仍然统一走 nativeUpdateStarMapNode(starmapId, nodeId, JSON.stringify(body))，
   * 不另建第二套 Core 调用。
   */
  async updateStarMapNode(starmapId: string, nodeId: string, patch: StarMapNodeUpdatePatch): Promise<ResultEnvelope<StarMapNode>> {
    try {
      const body: StarMapNodePatchBody = {}
      if (patch.title !== undefined) {
        body.title = patch.title
      }
      if (patch.content !== undefined) {
        body.content = patch.content
      }
      if (patch.position !== undefined) {
        body.position = patch.position
      }
      const jsonStr: string = this.core.getNativeModule().nativeUpdateStarMapNode(starmapId, nodeId, JSON.stringify(body))
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
        title: title,
        kind: kind,
        payload: null,
        tags: [],
        content: { type: 'empty', summary: null, body: null, projectId: null, volumeId: null, chapterId: null, rangeStart: null, rangeEnd: null, entityType: null, entityId: null, uri: null, label: null },
        anchors: [],
        portal: null,
        position: position,
        style: { fillColor: null },
        provenance: { source: 'Human', sourceId: null, generatedBy: null, promptId: null, reviewStatus: 'Accepted', createdFromAnchor: null },
        createdAt: now,
        updatedAt: now
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
          target: { type: 'starmap', nodeId: null, anchorId: null, projectId: null, volumeId: null, chapterId: null, rangeStart: null, rangeEnd: null, entityType: null, entityId: null, uri: null }
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
    } catch (e) { return nativeError<StarMapCrossLayerConnectionPlan>(e) }
  }
}
