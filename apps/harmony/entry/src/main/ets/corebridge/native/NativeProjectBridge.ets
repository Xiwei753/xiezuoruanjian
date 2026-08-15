import { Project, ProjectTree, Volume, WritingStats } from '../dto/ProjectDtos'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeProjectBridge {
  constructor(private core: NativeCoreModule) {}

  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListProjects()
      return this.core.parseEnvelope<Project[]>(jsonStr)
    } catch (e) { return nativeError<Project[]>(e) }
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetProjectTree(projectId)
      return this.core.parseEnvelope<ProjectTree>(jsonStr)
    } catch (e) { return nativeError<ProjectTree>(e) }
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateProject(name)
      return this.core.parseEnvelope<Project>(jsonStr)
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
      return this.core.parseEnvelope<WritingStats>(jsonStr)
    } catch (e) { return nativeError<WritingStats>(e) }
  }

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListVolumes(projectId)
      return this.core.parseEnvelope<Volume[]>(jsonStr)
    } catch (e) { return nativeError<Volume[]>(e) }
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateVolume(projectId, name)
      return this.core.parseEnvelope<Volume>(jsonStr)
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
