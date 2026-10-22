// SearchController — 全局搜索的查询、防抖、分页与请求序号管理。
//
// 平台只负责输入节奏和分页状态；搜索、排序、命中范围与游标全部由 Core 返回。
// - debounce：输入停顿后才查询，避免每个字符扫一遍索引；
// - sequence guard：新查询或页面释放后，旧请求的返回直接丢弃；
// - hasMore / nextCursor：游标由 Core 生成，客户端只原样回传；
// - targetExists：点击结果前检查目标是否已被删除，删除时保留当前搜索列表。

import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { SearchContext, SearchResult, SearchResultPage } from '../../../corebridge/dto/SearchDtos'
import { getBridge } from '../../../app/AppContext'

const SEARCH_DEBOUNCE_MS: number = 260
const SEARCH_PAGE_SIZE: number = 40

function cloneContext(context: SearchContext): SearchContext {
  const copy: SearchContext = { originScreen: context.originScreen }
  if (context.currentProjectId !== undefined) { copy.currentProjectId = context.currentProjectId }
  if (context.currentVolumeId !== undefined) { copy.currentVolumeId = context.currentVolumeId }
  if (context.currentChapterId !== undefined) { copy.currentChapterId = context.currentChapterId }
  if (context.currentStarmapId !== undefined) { copy.currentStarmapId = context.currentStarmapId }
  if (context.currentSettingsSection !== undefined) {
    copy.currentSettingsSection = context.currentSettingsSection
  }
  return copy
}

@ObservedV2
export class SearchController {
  @Trace query: string = ''
  @Trace items: SearchResult[] = []
  @Trace isLoading: boolean = false
  @Trace isLoadingMore: boolean = false
  @Trace hasSearched: boolean = false
  @Trace errorMessage: string = ''
  @Trace hasMore: boolean = false
  @Trace originScreen: string = 'general'

  private bridge: IWriterCoreBridge = getBridge()
  private context: SearchContext = { originScreen: 'general' }
  private scope: string = 'all'
  private cursor: string | null = null
  private requestSerial: number = 0
  private debounceTimer: number = -1
  private disposed: boolean = false

  /** 设置发起搜索的页面上下文（project/chapter/starmap/settings）。 */
  setContext(context: SearchContext): void {
    this.context = cloneContext(context)
    this.originScreen = context.originScreen
  }

  /** 可选范围；默认 all，由 Core 做排序而不是平台过滤。 */
  setScope(scope: string): void {
    this.scope = scope
  }

  getContext(): SearchContext {
    return cloneContext(this.context)
  }

  /**
   * 输入变化入口。空串会立即清空结果；非空按 debounce 合并。
   * immediate=true 用于初始关键词或显式提交（搜索按钮）。
   */
  setQuery(value: string, immediate: boolean = false): void {
    this.query = value
    this.cancelDebounce()
    // 新输入先让所有在途请求失效，旧结果不能覆盖新关键词。
    this.requestSerial++
    const normalized: string = value.trim()
    if (normalized.length === 0) {
      this.resetResults()
      return
    }

    this.items = []
    this.hasSearched = false
    this.hasMore = false
    this.errorMessage = ''
    this.cursor = null
    this.isLoading = true
    this.isLoadingMore = false

    if (immediate) {
      void this.searchFirstPage()
      return
    }
    this.debounceTimer = setTimeout((): void => {
      this.debounceTimer = -1
      void this.searchFirstPage()
    }, SEARCH_DEBOUNCE_MS)
  }

  /** 重新执行当前关键词（错误重试）。 */
  retry(): void {
    if (this.query.trim().length === 0) {
      return
    }
    this.cancelDebounce()
    this.items = []
    this.hasMore = false
    this.errorMessage = ''
    this.cursor = null
    this.isLoading = true
    void this.searchFirstPage()
  }

  /** 加载下一页。没有游标、正在查询或已释放时是 no-op。 */
  loadMore(): void {
    if (this.disposed || this.isLoading || this.isLoadingMore || !this.hasMore || this.cursor === null) {
      return
    }
    void this.requestPage(this.cursor, true)
  }

  /** 页面销毁时取消防抖并让在途回调失效。 */
  dispose(): void {
    this.disposed = true
    this.cancelDebounce()
    this.requestSerial++
  }

  /**
   * 点击结果前验证目标。只有在 Core 明确返回目标列表且目标不在其中时才判为已删除；
   * 查询本身失败时不冒充“已删除”，交给目标页面展示真实错误。
   */
  async targetExists(item: SearchResult): Promise<boolean> {
    if (item.scope === 'setting' || item.target.settingKey !== null) {
      return true
    }
    const projectId: string | null = item.target.projectId
    try {
      if (item.target.chapterId !== null || item.target.volumeId !== null) {
        if (projectId === null) {
          return true
        }
        const tree = await this.bridge.getProjectTree(projectId)
        if (!tree.success || tree.data === undefined) {
          return true
        }
        for (const volumeTree of tree.data.volumes) {
          if (item.target.volumeId !== null && item.target.chapterId === null) {
            if (volumeTree.volume.id === item.target.volumeId) {
              return true
            }
          }
          if (item.target.chapterId !== null) {
            for (const chapter of volumeTree.chapters) {
              if (chapter.id === item.target.chapterId) {
                return true
              }
            }
          }
        }
        return false
      }

      if (item.target.starmapId !== null) {
        if (item.target.nodeId !== null || item.target.edgeId !== null
          || item.target.embedId !== null || item.target.linkId !== null
          || item.target.hyperlinkId !== null) {
          const graph = await this.bridge.getStarMapGraph(item.target.starmapId)
          if (!graph.success || graph.data === undefined) {
            return true
          }
          if (item.target.nodeId !== null) {
            for (const node of graph.data.nodes) {
              if (node.id === item.target.nodeId) {
                return true
              }
            }
            return false
          }
          if (item.target.edgeId !== null) {
            for (const edge of graph.data.edges) {
              if (edge.id === item.target.edgeId) {
                return true
              }
            }
            return false
          }
          if (item.target.embedId !== null) {
            for (const embed of graph.data.embeds) {
              if (embed.instanceId === item.target.embedId) {
                return true
              }
            }
            return false
          }
          if (item.target.linkId !== null) {
            for (const link of graph.data.links) {
              if (link.linkId === item.target.linkId) {
                return true
              }
            }
            return false
          }
          if (item.target.hyperlinkId !== null) {
            for (const hyperlink of graph.data.hyperlinks) {
              if (hyperlink.hyperlinkId === item.target.hyperlinkId) {
                return true
              }
            }
            return false
          }
        }
        const starmap = await this.bridge.getStarMap(item.target.starmapId)
        return starmap.success && starmap.data !== undefined
      }

      if (projectId !== null) {
        const projects = await this.bridge.listProjects()
        if (!projects.success || projects.data === undefined) {
          return true
        }
        for (const project of projects.data) {
          if (project.id === projectId) {
            return true
          }
        }
        return false
      }
      return true
    } catch (_e) {
      return true
    }
  }

  private async searchFirstPage(): Promise<void> {
    if (this.disposed || this.query.trim().length === 0) {
      return
    }
    this.cursor = null
    await this.requestPage(null, false)
  }

  private async requestPage(cursor: string | null, append: boolean): Promise<void> {
    const serial: number = ++this.requestSerial
    const text: string = this.query.trim()
    if (append) {
      this.isLoadingMore = true
    } else {
      this.isLoading = true
    }
    this.errorMessage = ''

    let response: ResultEnvelope<SearchResultPage>
    try {
      response = await this.bridge.globalSearchPage(
        text, this.scope, SEARCH_PAGE_SIZE, cursor, this.context)
    } catch (e) {
      response = { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }

    // 新查询/页面释放后到的旧响应直接丢弃，不写入任何 @Trace 状态。
    if (this.disposed || serial !== this.requestSerial) {
      return
    }

    this.isLoading = false
    this.isLoadingMore = false
    this.hasSearched = true

    if (!response.success || response.data === undefined) {
      if (!append) {
        this.items = []
        this.cursor = null
        this.hasMore = false
      }
      this.errorMessage = response.userMessage !== undefined && response.userMessage.length > 0
        ? response.userMessage
        : '搜索失败，请稍后重试'
      return
    }

    const page: SearchResultPage = response.data
    this.items = append ? this.items.concat(page.items) : page.items
    this.cursor = page.nextCursor
    this.hasMore = page.nextCursor !== null
  }

  private resetResults(): void {
    this.items = []
    this.hasSearched = false
    this.isLoading = false
    this.isLoadingMore = false
    this.errorMessage = ''
    this.hasMore = false
    this.cursor = null
  }

  private cancelDebounce(): void {
    if (this.debounceTimer !== -1) {
      clearTimeout(this.debounceTimer)
      this.debounceTimer = -1
    }
  }
}
