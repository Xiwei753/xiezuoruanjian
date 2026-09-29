import { ResultEnvelope } from '../dto/ResultEnvelope'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapPoint, StarMapTargetPath } from '../dto/StarMapDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeStarMapMeta, decodeStarMapGraph, decodeStarMapNode, decodeStarMapEdge, decodeStarMapEmbed, decodeArray } from '../codec/CoreWireDecoders'
import util from '@ohos.util'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

/** StarMapNodePatchInputDto 请求体：platform 只发要改的字段（position）。 */
interface StarMapNodePatchBody {
  position: StarMapPoint
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

  async updateStarMapNode(starmapId: string, nodeId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>> {
    try {
      // StarMapNodePatchInputDto：只发 position（position 是节点位置的唯一真相）
      const body: StarMapNodePatchBody = { position: position }
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
}
