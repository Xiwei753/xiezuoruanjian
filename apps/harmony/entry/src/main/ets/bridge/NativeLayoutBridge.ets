import { ChangedEntity, ResultEnvelope, WindowMetrics, LayoutPlan, ScreenPolicy, ScreenRole, ShellMode } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeLayoutBridge {
  constructor(private core: NativeCoreModule) {}

  async resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveLayout(JSON.stringify(metrics))
      return this.core.parseEnvelope<LayoutPlan>(jsonStr)
    } catch (e) { return nativeError<LayoutPlan>(e) }
  }

  async resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ResultEnvelope<ScreenPolicy>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveScreenPolicy(
        JSON.stringify(screenRole),
        JSON.stringify(shellMode)
      )
      return this.core.parseEnvelope<ScreenPolicy>(jsonStr)
    } catch (e) { return nativeError<ScreenPolicy>(e) }
  }
}
