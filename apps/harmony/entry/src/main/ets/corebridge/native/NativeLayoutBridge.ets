import { WindowCapabilities, LayoutContract, ScreenPolicy, ScreenRole } from '../dto/PlatformDtos'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeLayoutBridge {
  constructor(private core: NativeCoreModule) {}

  async resolveLayout(capabilities: WindowCapabilities): Promise<ResultEnvelope<LayoutContract>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveLayout(JSON.stringify(capabilities))
      return this.core.parseEnvelope<LayoutContract>(jsonStr)
    } catch (e) { return nativeError<LayoutContract>(e) }
  }

  async resolveScreenPolicy(screenRole: ScreenRole): Promise<ResultEnvelope<ScreenPolicy>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveScreenPolicy(
        JSON.stringify(screenRole)
      )
      return this.core.parseEnvelope<ScreenPolicy>(jsonStr)
    } catch (e) { return nativeError<ScreenPolicy>(e) }
  }
}
