import { Project, ProjectTree, Volume, WritingStats } from '../dto/ProjectDtos'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeProject, decodeProjectTree, decodeVolume, decodeWritingStats, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeProjectBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListProjects()
      return this.core.decodeEnvelope<Project[]>(jsonStr,
        (d: Object) => decodeArray<Project>(d, decodeProject, 'Project'))
    } catch (e) { return nativeError<Project[]>(e) }
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetProjectTree(projectId)
      return this.core.decodeEnvelope<ProjectTree>(jsonStr, decodeProjectTree)
    } catch (e) { return nativeError<ProjectTree>(e) }
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateProject(name)
      return this.core.decodeEnvelope<Project>(jsonStr, decodeProject)
    } catch (e) { return nativeError<Project>(e) }
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRenameProject(projectId, newName)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteProject(projectId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetProjectStats(projectId)
      return this.core.decodeEnvelope<WritingStats>(jsonStr, decodeWritingStats)
    } catch (e) { return nativeError<WritingStats>(e) }
  }

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListVolumes(projectId)
      return this.core.decodeEnvelope<Volume[]>(jsonStr,
        (d: Object) => decodeArray<Volume>(d, decodeVolume, 'Volume'))
    } catch (e) { return nativeError<Volume[]>(e) }
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateVolume(projectId, name)
      return this.core.decodeEnvelope<Volume>(jsonStr, decodeVolume)
    } catch (e) { return nativeError<Volume>(e) }
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRenameVolume(projectId, volumeId, newName)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteVolume(projectId, volumeId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeReorderVolumes(projectId, JSON.stringify(orderedVolumeIds))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
