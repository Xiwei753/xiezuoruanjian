import { ChangedEntity, ResultEnvelope, WorkspaceSummary } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

export class NativeWorkspaceBridge {
  constructor(private core: NativeCoreModule) {}

  async listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListWorkspaces()
      return this.core.parseEnvelope<WorkspaceSummary[]>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<WorkspaceSummary[]>()
    }
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceSummary>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeOpenWorkspace(path)
      return this.core.parseEnvelope<WorkspaceSummary>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<WorkspaceSummary>()
    }
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeValidateWorkspace()
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWorkspaceState()
      return this.core.parseEnvelope<WorkspaceSummary>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<WorkspaceSummary>()
    }
  }
}
