import { ResultEnvelope } from '../dto/ResultEnvelope'
import { SearchContext, SearchResultPage } from '../dto/SearchDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeSearchResultPage } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
}

/**
 * Global search bridge。
 *
 * 只负责把查询参数交给 Core 并把 ResultEnvelope 解码成强类型；
 * 防抖、分页状态和页面导航都在 SearchController / SearchScreen。
 */
export class NativeSearchBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async globalSearchPage(
    query: string,
    scope: string,
    limit: number,
    cursor: string | null,
    context: SearchContext
  ): Promise<ResultEnvelope<SearchResultPage>> {
    try {
      const contextJson: string = JSON.stringify(context)
      const cursorArg: string = cursor === null ? '' : cursor
      const jsonStr: string = await (this.core.getNativeModule()
        .nativeGlobalSearchPage(query, scope, limit, cursorArg, contextJson) as Promise<string>)
      return this.core.decodeEnvelope<SearchResultPage>(jsonStr, decodeSearchResultPage)
    } catch (e) {
      return nativeError<SearchResultPage>(e)
    }
  }
}
