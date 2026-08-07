import { ChangedEntity, ResultEnvelope, WorkspaceSummary } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

/**
 * Workspace 查询 Bridge。
 *
 * 新 Core API 边界：平台自己提供 app_data_root 与 projects_root，
 * 通过 NativeWriterCoreBridge.initialize 注入；Core 不再创建/验证/打开 workspace。
 * 此 Bridge 仅保留查询类入口（list/get state）。
 */
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

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWorkspaceState()
      return this.core.parseEnvelope<WorkspaceSummary>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<WorkspaceSummary>()
    }
  }
}
