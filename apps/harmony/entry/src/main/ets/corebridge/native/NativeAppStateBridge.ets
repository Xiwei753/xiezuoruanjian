import { ChangedEntity, ResultEnvelope, AppSummary } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'

/**
 * App State 查询 Bridge。
 *
 * 新 Core API 边界：平台自己提供 app_data_root 与 projects_root，
 * 通过 NativeWriterCoreBridge.initialize 注入；Core 不再创建/验证/打开 workspace。
 * 此 Bridge 仅保留查询类入口（list/get state）。
 */
export class NativeAppStateBridge {
  constructor(private core: NativeCoreModule) {}

  async listAppSummaries(): Promise<ResultEnvelope<AppSummary[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListAppSummaries()
      return this.core.parseEnvelope<AppSummary[]>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<AppSummary[]>()
    }
  }

  async getAppState(): Promise<ResultEnvelope<AppSummary>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetAppState()
      return this.core.parseEnvelope<AppSummary>(jsonStr)
    } catch (e) {
      return this.core.notImplemented<AppSummary>()
    }
  }
}
