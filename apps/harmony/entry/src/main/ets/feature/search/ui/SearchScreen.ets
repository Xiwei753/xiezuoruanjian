// SearchScreen — 全局搜索界面。
//
// 窄屏：作为 Routes.Search 路由页压在现有页面之上，返回只 pop 搜索页，原页面/滚动位置不变。
// 宽屏：由 SearchOverlayController 作为窗口内浮层承载，关闭只 dismiss 对话框，不动底层路由栈。
//
// 结果列表来自 Core 分页结果；每个 item 是一次独立命中（同一章多个命中逐条显示）。
// summaryMatchRanges 是 UTF-8 byte 偏移，先按真实 summary 原文转成 UTF-16 再切片高亮。

import {
  Routes, TAB_HOME, TAB_STARMAP, SearchRouteParams, WritingRouteParams, WorkspaceRouteParams,
  StarMapRouteParams, SettingsRouteParams
} from '../../../app/navigation/Routes'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole } from '../../../app/navigation/ScreenSemantics'
import { SearchContext, SearchResult, SearchScope } from '../../../corebridge/dto/SearchDtos'
import { TextOffsetMapper } from '../../editor/input/TextOffsetMapper'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { SettingsSectionContext } from '../../settings/presentation/SettingsSectionContext'
import { SettingsOverlayController } from '../../settings/presentation/SettingsOverlayController'
import { SearchController } from '../presentation/SearchController'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'

interface SummarySegment {
  text: string
  highlighted: boolean
}

function cloneSearchContext(context: SearchContext): SearchContext {
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

@ComponentV2
export struct SearchScreen {
  @Param navPathStack: NavPathStack = new NavPathStack()
  /** 路由页所在的一级 tab，用于隐藏底栏时上报正确活动 tab。 */
  @Param screenTabIndex: number = 0
  /** true = 宽屏窗口内浮层；false = 窄屏路由页。 */
  @Param overlayLayout: boolean = false
  /** 浮层关闭；路由页由系统返回/pop 处理。 */
  @Param onRequestClose: () => void = () => {}
  /** 浮层所在页面需要一起退出时（例如设置浮层上的搜索跳走），由打开方注入。 */
  @Param onNavigateAway: () => void = () => {}
  /** 浮层由打开方直接注入上下文；路由页从 SearchRouteParams 读取。 */
  @Param originContext: SearchContext | null = null
  @Param initialQuery: string = ''

  @Local controller: SearchController = new SearchController()
  @Local queryText: string = ''

  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()
  private openedWithSettingsOrigin: boolean = false
  private continuationContext: SearchContext | null = null

  aboutToAppear(): void {
    let context: SearchContext | null = this.originContext
    let initial: string = this.initialQuery
    if (context === null) {
      const paramsArr = this.navPathStack.getParamByName(Routes.Search) as SearchRouteParams[]
      const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
      if (params !== null) {
        context = params.context
        if (params.initialQuery !== undefined) {
          initial = params.initialQuery
        }
      }
    }
    const resolved: SearchContext = context !== null
      ? cloneSearchContext(context)
      : { originScreen: 'general' }
    if (resolved.originScreen === 'settings' && resolved.currentSettingsSection === undefined) {
      resolved.currentSettingsSection = SettingsSectionContext.getCurrentSection()
    }
    this.openedWithSettingsOrigin = resolved.originScreen === 'settings'
    this.continuationContext = resolved
    this.controller.setContext(resolved)
    if (initial.length > 0) {
      this.queryText = initial
      this.controller.setQuery(initial, true)
    }
    this.updateContinuationSnapshot(resolved, initial)
  }

  // Issue #988：搜索页接续保留来源上下文和查询词；返回时仍只 pop 搜索页。
  private updateContinuationSnapshot(context: SearchContext, query: string): void {
    const payload: ContinuationPayload = {
      screen: 'search',
      tabIndex: TAB_HOME,
      searchQuery: query,
      searchOriginScreen: context.originScreen
    }
    if (context.currentProjectId !== undefined) {
      payload.searchProjectId = context.currentProjectId
    }
    if (context.currentVolumeId !== undefined) {
      payload.searchVolumeId = context.currentVolumeId
    }
    if (context.currentChapterId !== undefined) {
      payload.searchChapterId = context.currentChapterId
    }
    if (context.currentStarmapId !== undefined) {
      payload.searchStarmapId = context.currentStarmapId
    }
    if (context.currentSettingsSection !== undefined) {
      payload.searchSettingsSection = context.currentSettingsSection
    }
    AppContinuationService.getInstance().updateCurrentScreen(payload)
  }

  aboutToDisappear(): void {
    this.controller.dispose()
  }

  private closeSearch(): void {
    if (this.overlayLayout) {
      this.onRequestClose()
      return
    }
    this.navCoordinator.safePopWithStack(this.navPathStack)
  }

  // ── 结果点击 ──

  private async openResult(item: SearchResult): Promise<void> {
    const exists: boolean = await this.controller.targetExists(item)
    if (!exists) {
      // 目标已删除时只提示，不清空搜索结果，用户可以继续点其他命中。
      try {
        this.getUIContext().getPromptAction().showToast({ message: '目标已不存在' })
      } catch (_e) {
        // Toast 失败不影响保留搜索列表。
      }
      return
    }
    this.navigateToTarget(item)
  }

  private navigateToTarget(item: SearchResult): void {
    const target = item.target

    if (item.scope === 'setting' || target.settingKey !== null) {
      const section: string = target.settingSection !== null ? target.settingSection : ''
      const settingKey: string = target.settingKey !== null ? target.settingKey : ''
      this.navigateToSetting(section, settingKey)
      return
    }

    if (target.chapterId !== null) {
      const writingParams: WritingRouteParams = {
        chapterId: target.chapterId,
        chapterTitle: item.title,
        projectId: target.projectId !== null ? target.projectId : '',
        volumeId: target.volumeId !== null ? target.volumeId : ''
      }
      if (item.scope === 'chapterBody' || item.scope === 'chapterTitle') {
        writingParams.searchByteStart = item.byteStart
        writingParams.searchByteEnd = item.byteEnd
        writingParams.searchField = item.field
      }
      // 正文/章节属于作品 tab；即使搜索从星图 tab 发起也不能压到星图栈。
      void this.pushRoute(TAB_HOME, Routes.Writing, writingParams, this.resultDedupeKey(item))
      return
    }

    if (target.starmapId !== null) {
      const starmapParams: StarMapRouteParams = {
        starmapId: target.starmapId,
        starmapTitle: item.scope === 'starmapTitle' ? item.title : this.lastPathSegment(item.path)
      }
      if (target.nodeId !== null) {
        starmapParams.focusKind = 'node'
        starmapParams.focusId = target.nodeId
      } else if (target.edgeId !== null) {
        starmapParams.focusKind = 'edge'
        starmapParams.focusId = target.edgeId
      } else if (target.linkId !== null) {
        starmapParams.focusKind = 'link'
        starmapParams.focusId = target.linkId
      } else if (target.hyperlinkId !== null) {
        starmapParams.focusKind = 'hyperlink'
        starmapParams.focusId = target.hyperlinkId
      } else if (target.embedId !== null) {
        starmapParams.focusKind = 'embed'
        starmapParams.focusId = target.embedId
      }
      // 星图目标固定进入星图 tab 自己的栈。
      void this.pushRoute(TAB_STARMAP, Routes.StarMap, starmapParams, this.resultDedupeKey(item))
      return
    }

    if (target.projectId !== null || target.volumeId !== null) {
      const workspaceParams: WorkspaceRouteParams = {
        projectId: target.projectId !== null ? target.projectId : '',
        projectName: item.scope === 'projectTitle' ? item.title : this.firstPathSegment(item.path)
      }
      if (target.volumeId !== null) {
        workspaceParams.focusVolumeId = target.volumeId
      }
      void this.pushRoute(TAB_HOME, Routes.Workspace, workspaceParams, this.resultDedupeKey(item))
    }
  }

  // Issue #982：设置目标走现有设置页 / 宽屏浮层；settingKey 作为一次性定位意图，
  // 不再只展开分类。已从设置页发起搜索时只更新原设置页并关闭搜索。
  private navigateToSetting(section: string, settingKey: string): void {
    if (this.openedWithSettingsOrigin) {
      SettingsSectionContext.requestLocate(section, settingKey)
      this.closeSearch()
      return
    }
    if (SettingsOverlayController.getInstance().isOverlayLayout()) {
      SettingsSectionContext.requestLocate(section, settingKey)
      SettingsOverlayController.getInstance().open(section, settingKey)
      if (this.overlayLayout) {
        this.onNavigateAway()
        this.onRequestClose()
      } else {
        // 窄屏搜索路由页浮在页面上时切换成宽屏：设置以窗口浮层出现，搜索页只关闭自己。
        this.navPathStack.removeByName(Routes.Search)
      }
      return
    }
    const settingsParams: SettingsRouteParams = {}
    if (section.length > 0) {
      settingsParams.section = section
    }
    if (settingKey.length > 0) {
      settingsParams.settingKey = settingKey
    }
    void this.pushRoute(
      this.screenTabIndex, Routes.Settings, settingsParams, `setting:${settingKey}:${section}`
    )
  }

  private resultDedupeKey(item: SearchResult): string {
    return `${item.scope}:${item.objectId}:${item.field}:${item.byteStart}`
  }

  // Issue #982：所有路由目标都通过 coordinator 进入正确的一级 tab 栈。
  // coordinator 内部先走 leave/save guard，再切 tab、取目标栈、push；只有 push 成功后才关闭搜索。
  private async pushRoute(
    tabIndex: number,
    name: string,
    param: Object,
    dedupeKey: string
  ): Promise<void> {
    const navigated: boolean = await this.navCoordinator.navigateToTabStack(tabIndex, name, param, dedupeKey)
    if (!navigated) {
      this.showToast('无法打开目标页面')
      return
    }
    if (this.overlayLayout) {
      // 浮层只是窗口级展示层；目标已压入目标 tab 栈，关闭浮层即露出新页面。
      this.onNavigateAway()
      this.onRequestClose()
      return
    }
    // 窄屏搜索路由页：目标已经进入正确 tab 栈，再移除来源栈上的搜索页。
    // removeByName 只移除 Search destination，不重建底层页面，原页面与滚动状态保持不变。
    this.navPathStack.removeByName(Routes.Search)
  }

  private showToast(message: string): void {
    try {
      this.getUIContext().getPromptAction().showToast({ message: message })
    } catch (_e) {
      // Toast 失败不影响搜索结果保留。
    }
  }

  private firstPathSegment(path: string): string {
    const parts: string[] = path.split(' / ')
    return parts.length > 0 ? parts[0] : path
  }

  private lastPathSegment(path: string): string {
    const parts: string[] = path.split(' / ')
    return parts.length > 0 ? parts[parts.length - 1] : path
  }

  // ── 摘要高亮 ──

  private buildSummarySegments(item: SearchResult): SummarySegment[] {
    const text: string = item.summary
    if (text.length === 0 || item.summaryMatchRanges.length === 0) {
      return [{ text: text, highlighted: false }]
    }

    const ranges: number[][] = []
    for (let i = 0; i < item.summaryMatchRanges.length; i++) {
      const range: number[] = item.summaryMatchRanges[i]
      if (range.length < 2) {
        continue
      }
      const start: number = TextOffsetMapper.utf8ToUtf16(text, range[0])
      const end: number = TextOffsetMapper.utf8ToUtf16(text, range[1])
      if (end > start) {
        ranges.push([start, end])
      }
    }
    ranges.sort((a: number[], b: number[]): number => a[0] - b[0])

    const segments: SummarySegment[] = []
    let cursor: number = 0
    for (let i = 0; i < ranges.length; i++) {
      const start: number = Math.max(cursor, Math.min(ranges[i][0], text.length))
      const end: number = Math.max(start, Math.min(ranges[i][1], text.length))
      if (start > cursor) {
        segments.push({ text: text.substring(cursor, start), highlighted: false })
      }
      if (end > start) {
        segments.push({ text: text.substring(start, end), highlighted: true })
      }
      cursor = Math.max(cursor, end)
    }
    if (cursor < text.length) {
      segments.push({ text: text.substring(cursor), highlighted: false })
    }
    if (segments.length === 0) {
      return [{ text: text, highlighted: false }]
    }
    return segments
  }

  private scopeLabel(scope: SearchScope): string {
    if (scope === 'chapterBody') { return '章节正文' }
    if (scope === 'chapterTitle') { return '章节标题' }
    if (scope === 'chapterNote') { return '章节备注' }
    if (scope === 'projectTitle') { return '作品' }
    if (scope === 'volumeTitle') { return '卷' }
    if (scope === 'starmapTitle') { return '星图' }
    if (scope === 'starmapNode') { return '星图节点' }
    if (scope === 'starmapEdgeLabel') { return '星图连线' }
    if (scope === 'starmapHyperlink') { return '星图超链接' }
    if (scope === 'starmapLink') { return '星图链接' }
    if (scope === 'starmapEmbed') { return '星图嵌入' }
    if (scope === 'setting') { return '设置' }
    return '全部'
  }

  // ── UI ──

  @Builder
  SearchInput() {
    Search({ value: this.queryText, placeholder: '搜索作品、章节、星图与设置' })
      .width('100%')
      .height(44)
      .margin({ left: 16, right: 16, top: 12, bottom: 8 })
      .backgroundColor($r('sys.color.comp_background_gray'))
      .borderRadius(DesignTokens.inputFieldRadius)
      .onChange((value: string): void => {
        this.queryText = value
        this.controller.setQuery(value)
        if (this.continuationContext !== null) {
          this.updateContinuationSnapshot(this.continuationContext, value)
        }
      })
      .onSubmit((): void => {
        this.controller.setQuery(this.queryText, true)
      })
  }

  @Builder
  SearchResultRow(item: SearchResult) {
    Column() {
      Row() {
        Text(this.scopeLabel(item.scope))
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .backgroundColor($r('sys.color.comp_background_gray'))
          .borderRadius(4)
          .padding({ left: 6, right: 6, top: 2, bottom: 2 })

        Text(item.title)
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .margin({ left: 8 })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Text(item.path)
        .fontSize(12)
        .fontColor($r('sys.color.font_secondary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%')
        .margin({ top: 4 })

      Text() {
        ForEach(this.buildSummarySegments(item), (segment: SummarySegment, index: number) => {
          Span(segment.text)
            .fontColor(segment.highlighted ? $r('sys.color.font_emphasize') : $r('sys.color.font_primary'))
            .fontWeight(segment.highlighted ? FontWeight.Bold : FontWeight.Normal)
        }, (segment: SummarySegment, index: number) => `${index}:${segment.text}`)
      }
      .fontSize(14)
      .lineHeight(21)
      .maxLines(3)
      .textOverflow({ overflow: TextOverflow.Ellipsis })
      .width('100%')
      .margin({ top: 6 })
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 12, bottom: 12 })
    .alignItems(HorizontalAlign.Start)
    .onClick((): void => {
      void this.openResult(item)
    })
  }

  @Builder
  ResultBody() {
    if (this.controller.isLoading) {
      CompactPageState({
        kind: CompactPageStateKind.Loading,
        title: '搜索中…'
      })
    } else if (this.controller.errorMessage.length > 0) {
      CompactPageState({
        kind: CompactPageStateKind.Error,
        title: this.controller.errorMessage,
        onRetry: (): void => { this.controller.retry() }
      })
    } else if (!this.controller.hasSearched) {
      CompactPageState({
        kind: CompactPageStateKind.Empty,
        title: '搜索作品、章节、星图与设置',
        description: '输入关键词后显示匹配位置，点击结果直接打开原文。'
      })
    } else if (this.controller.items.length === 0) {
      CompactPageState({
        kind: CompactPageStateKind.Empty,
        title: '没有找到匹配结果',
        description: '换个关键词试试。'
      })
    } else {
      List() {
        ForEach(this.controller.items, (item: SearchResult) => {
          ListItem() {
            this.SearchResultRow(item)
          }
        }, (item: SearchResult): string =>
          `${item.objectId}:${item.field}:${item.scope}:${item.byteStart}:${item.byteEnd}`)

        if (this.controller.hasMore || this.controller.isLoadingMore) {
          ListItem() {
            Row() {
              if (this.controller.isLoadingMore) {
                LoadingProgress().width(20).height(20)
              } else {
                Text('加载更多')
                  .fontSize(14)
                  .fontColor($r('sys.color.font_emphasize'))
              }
            }
            .width('100%')
            .height(48)
            .justifyContent(FlexAlign.Center)
            .onClick((): void => { this.controller.loadMore() })
          }
        }
      }
      .width('100%')
      .layoutWeight(1)
      .onReachEnd((): void => { this.controller.loadMore() })
    }
  }

  @Builder
  SearchContent() {
    Column() {
      this.SearchInput()
      this.ResultBody()
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  @Builder
  OverlayFrame() {
    Column() {
      Row() {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          Image($r('sys.media.ohos_ic_public_arrow_left'))
            .width(20)
            .height(20)
            .fillColor($r('sys.color.font_primary'))
        }
        .width(40)
        .height(40)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .accessibilityText('关闭搜索')
        .onClick((): void => { this.closeSearch() })

        Text('全局搜索')
          .fontSize(22)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .margin({ left: 12 })
      }
      .width('100%')
      .padding({ left: '6%', right: '6%', top: '2%', bottom: '1%' })
      .alignItems(VerticalAlign.Center)

      Column() {
        this.SearchContent()
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: '3%', right: '3%', bottom: '2%' })
    }
    .width('82.4%')
    .height('84%')
    .backgroundColor($r('sys.color.background_secondary'))
    .borderRadius(DesignTokens.dialogRadius)
    .clip(true)
  }

  build() {
    if (this.overlayLayout) {
      this.OverlayFrame()
    } else {
      NavDestination() {
        this.SearchContent()
      }
      .hideTitleBar(false)
      .title('全局搜索')
      .onShown((): void => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Search, this.screenTabIndex)
      })
      .onBackPressed((): boolean => {
        this.closeSearch()
        return true
      })
    }
  }
}
