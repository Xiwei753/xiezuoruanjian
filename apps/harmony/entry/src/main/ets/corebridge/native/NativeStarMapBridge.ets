import { ResultEnvelope } from '../dto/ResultEnvelope'
import { StarMapMeta, StarMapGraph, StarMapLayout, StarMapEdgeRender, StarMapMotionPolicy } from '../dto/StarMapDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeStarMapMeta, decodeStarMapGraph, decodeStarMapLayout, decodeStarMapEdgeRender, decodeStarMapMotionPolicy, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
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

  async saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveStarMapLayout(starmapId, JSON.stringify(layout))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetStarMapLayout(starmapId)
      return this.core.decodeEnvelope<StarMapLayout>(jsonStr, decodeStarMapLayout)
    } catch (e) { return nativeError<StarMapLayout>(e) }
  }

  async getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetStarMapMotionPolicy()
      return this.core.decodeEnvelope<StarMapMotionPolicy>(jsonStr, decodeStarMapMotionPolicy)
    } catch (e) { return nativeError<StarMapMotionPolicy>(e) }
  }

  async computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeComputeStarMapEdgeRenders(JSON.stringify(graph))
      return this.core.decodeEnvelope<StarMapEdgeRender[]>(jsonStr,
        (d: Object) => decodeArray<StarMapEdgeRender>(d, decodeStarMapEdgeRender, 'StarMapEdgeRender'))
    } catch (e) { return nativeError<StarMapEdgeRender[]>(e) }
  }
}
