// StarMapListScreen — 星图列表页。
// 作为一级"星图"tab 的真正根页面：
// - aboutToAppear 调 bridge.listStarMaps() 只加载列表，不打开任何星图
// - 卡片列表展示星图（标题/描述/更新时间），点击进入画布
// - 右下角 FAB 新建星图（必须先询问名称，不允许无名星图）
// - 卡片更多菜单支持重命名/删除
// - 空列表显示"暂无星图"
// Issue #932：窄屏内容层 + FAB 统一走 CompactPrimaryActionLayer（动作来自 ScreenSemantics），
// 加载/错误/空状态走 CompactPageState，创建/改名/删除框走窗口级 CompactDialogs。
// Issue #932 复核：窄屏（CompactPrimaryActionLayer）与宽屏（自绘内容层 + 右下角 FAB）
// 是两个并列形态，由 prefersWideLayout 分流——与导航壳 WidePrimaryShell 同一门槛，
// 宽屏不再借名为 Compact 的组件布局。

import hilog from '@ohos.hilog'
import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import { StarMapMeta } from '../../../corebridge/dto/StarMapDtos'
import { getBridge } from '../../../app/AppContext'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SPACING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  COMPACT_LIST_BOTTOM_INSET_VP,
  PRIMARY_FAB_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
// Issue #932：窄屏共享构件 — 内容层 + 右下角 FAB、公共状态视图、窗口级对话框。
import { CompactPrimaryActionLayer } from '../../../ui/components/CompactPrimaryActionLayer'
// Issue #932 复核：宽屏内容层直接复用 PrimaryFab 本体，与 CompactPrimaryActionLayer 内部是同一个按钮。
import { PrimaryFab } from '../../../ui/components/PrimaryFab'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { CompactConfirmDialogArgs, CompactTextDialogArgs, openCompactConfirmDialog, openCompactTextDialog } from '../../../ui/components/CompactDialogs'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ScreenRole, ActionSlot, ActionRegion, ActionRole, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { handleRootBack } from '../../../app/navigation/RootBackHandler'
@ComponentV2
export struct StarMapListScreen {
  // 由 PageMapForStack 显式传入 ownerStack，不使用全局 NavigationTransactionCoordinator
  @Param navPathStack: NavPathStack = new NavPathStack()

  // 页面 UI 状态（V2 @Local）
  @Local starMaps: StarMapMeta[] = []
  @Local isLoading: boolean = true
  @Local errorMessage: string = ''
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local useHdsNav: boolean = false
  @Local headerActions: ActionSlot[] = []

  // 对话框状态
  @Local showCreateDialog: boolean = false
  @Local newStarMapName: string = ''
  @Local newStarMapDescription: string = ''
  @Local renameTarget: StarMapMeta | null = null
  @Local renameInput: string = ''
  @Local deleteTarget: StarMapMeta | null = null

  // 更多菜单上下文
  @Local currentMenuStarMapId: string = ''

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  async aboutToAppear() {
    hilog.info(0x0000, 'StarMapListScreen', 'route name=StarMapList')
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      this.headerActions = resolveScreenPolicy(ScreenRole.StarMap).actionSlots
    }

    this.loadStarMaps()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ─── 数据加载 ───

  private async loadStarMaps(): Promise<void> {
    this.isLoading = true
    this.errorMessage = ''
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.listRootStarMaps()
      if (result.success && result.data) {
        this.starMaps = result.data
      } else {
        this.errorMessage = result.userMessage ?? '加载星图列表失败'
      }
    } catch (err) {
      this.errorMessage = '加载星图列表失败'
      hilog.error(0x0000, 'StarMapListScreen', `loadStarMaps error: ${JSON.stringify(err)}`)
    } finally {
      this.isLoading = false
    }
  }

  /** 打开「新建星图」命名框（原 FAB 的 onTap 行为）。 */
  // Issue #932：命名框改为窗口级 CompactDialogs.openCompactTextDialog；
  // showCreateDialog / newStarMapName / newStarMapDescription 的设置与清理时机不变，
  // 但不再作为页面渲染开关，点遮罩关闭由 onDidDisappear 负责清理。
  private requestCreateStarMap(): void {
    this.showCreateDialog = true
    this.newStarMapName = ''
    this.newStarMapDescription = ''
    const args: CompactTextDialogArgs = {
      title: '新建星图',
      placeholder: '请输入星图名称',
      initialText: '',
      // 星图创建框保留标题 + 描述两栏，字段语义与旧自绘实现一致。
      secondaryLabel: '描述',
      secondaryPlaceholder: '请输入星图描述（可选）',
      secondaryInitialText: '',
      confirmLabel: '创建',
      onConfirm: (text: string, secondaryText: string): void => {
        this.newStarMapName = text
        this.newStarMapDescription = secondaryText
        this.createStarMap()
      },
      onCancel: (): void => {
        this.showCreateDialog = false
        this.newStarMapName = ''
        this.newStarMapDescription = ''
      }
    }
    // 点遮罩 / 返回键关闭时同样清理创建状态。
    openCompactTextDialog(this.getUIContext(), args, (): void => {
      this.showCreateDialog = false
      this.newStarMapName = ''
      this.newStarMapDescription = ''
    })
  }

  /**
   * 检查星图页 PrimaryAction 中是否有 CreateStarMap（决定 FAB 是否显示）。
   * Issue #932：星图「+」的动作语义由 ScreenSemantics 统一声明，页面只按同名动作槽消费。
   */
  private hasCreateStarMapPrimaryAction(): boolean {
    const policy = resolveScreenPolicy(ScreenRole.StarMap)
    for (const action of policy.actionSlots) {
      if (action.region === ActionRegion.PrimaryAction && action.role === ActionRole.CreateStarMap) {
        return true
      }
    }
    return false
  }

  /**
   * 主操作 FAB 的显示条件：加载完成、无错误、且动作表里确实有 CreateStarMap。
   * Issue #932 复核：窄屏 / 宽屏两个内容层共用这一份判定，不各自抄一份条件式。
   */
  private hasStarMapPrimaryAction(): boolean {
    return !this.isLoading && !this.errorMessage && this.hasCreateStarMapPrimaryAction()
  }

  // ─── UI 事件处理 ───

  private navigateToStarMap(starmapId: string, starmapTitle: string): void {
    const param: StarMapRouteParams = { starmapId, starmapTitle }
    this.navPathStack.pushPath({ name: Routes.StarMap, param })
  }

  // ─── 新建星图 ───

  private async createStarMap(): Promise<void> {
    if (this.newStarMapName.trim().length === 0) {
      return
    }
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.createStarMap(this.newStarMapName.trim(), this.newStarMapDescription.trim())
      if (result.success && result.data) {
        this.starMaps.push(result.data)
      } else {
        this.errorMessage = result.userMessage ?? '创建星图失败'
      }
    } catch (err) {
      hilog.error(0x0000, 'StarMapListScreen', `createStarMap error: ${JSON.stringify(err)}`)
    } finally {
      this.showCreateDialog = false
      this.newStarMapName = ''
      this.newStarMapDescription = ''
    }
  }

  // ─── 重命名星图 ───

  // Issue #932：改名框改为窗口级，trigger 仍是卡片菜单的「重命名」。
  private startRename(starmap: StarMapMeta): void {
    this.renameTarget = starmap
    this.renameInput = starmap.title
    const args: CompactTextDialogArgs = {
      title: '重命名星图',
      placeholder: '请输入新名称',
      initialText: starmap.title,
      secondaryLabel: '',
      secondaryPlaceholder: '',
      secondaryInitialText: '',
      confirmLabel: '确认',
      onConfirm: (text: string, _secondaryText: string): void => {
        this.renameInput = text
        this.confirmRename()
      },
      onCancel: (): void => {
        this.cancelRename()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理重命名状态。
    openCompactTextDialog(this.getUIContext(), args, (): void => {
      this.cancelRename()
    })
  }

  private async confirmRename(): Promise<void> {
    const target: StarMapMeta | null = this.renameTarget
    if (target === null || this.renameInput.trim().length === 0) {
      return
    }
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.renameStarMap(target.starmapId, this.renameInput.trim())
      if (result.success) {
        // StarMapMeta 是普通 DTO interface，不是 @ObservedV2/@Trace 对象，
        // 原地修改 title 不会触发可靠刷新。重新从 Core 加载列表确保数据一致。
        await this.loadStarMaps()
      }
    } catch (err) {
      hilog.error(0x0000, 'StarMapListScreen', `renameStarMap error: ${JSON.stringify(err)}`)
    } finally {
      this.renameTarget = null
      this.renameInput = ''
    }
  }

  private cancelRename(): void {
    this.renameTarget = null
    this.renameInput = ''
  }

  // ─── 删除星图 ───

  // Issue #932：删除确认框改为窗口级，trigger 仍是卡片菜单的「删除」。
  private startDelete(starmap: StarMapMeta): void {
    this.deleteTarget = starmap
    const args: CompactConfirmDialogArgs = {
      title: '确认删除',
      message: `确定要删除"${starmap.title}"吗？此操作不可撤销。`,
      destructive: true,
      confirmLabel: '删除',
      onConfirm: (): void => {
        this.confirmDelete()
      },
      onCancel: (): void => {
        this.cancelDelete()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理删除目标。
    openCompactConfirmDialog(this.getUIContext(), args, (): void => {
      this.cancelDelete()
    })
  }

  private async confirmDelete(): Promise<void> {
    // Issue #932：窗口级对话框关闭后的 onDidDisappear 清理可能先于 await 返回执行，
    // 先把删除目标捕获到局部变量，不再依赖 this.deleteTarget 在 await 后仍非空。
    const target: StarMapMeta | null = this.deleteTarget
    if (target === null) {
      return
    }
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.deleteStarMap(target.starmapId)
      if (result.success) {
        this.starMaps = this.starMaps.filter((s: StarMapMeta) => s.starmapId !== target.starmapId)
      }
    } catch (err) {
      hilog.error(0x0000, 'StarMapListScreen', `deleteStarMap error: ${JSON.stringify(err)}`)
    } finally {
      this.deleteTarget = null
    }
  }

  private cancelDelete(): void {
    this.deleteTarget = null
  }

  // ─── 格式化工具 ───

  private formatTimestamp(timestamp: number): string {
    if (!timestamp || timestamp <= 0) {
      return ''
    }
    const date = new Date(timestamp)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes}分钟前`
    } else if (hours < 24) {
      return `${hours}小时前`
    } else if (days < 7) {
      return `${days}天前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  // ========== 构建 UI ==========

  @Builder
  StarMapListContent() {
    // Issue #932 复核：与作品页 HomeContent 的 prefersWideLayout 分流对齐。
    // 判定与 PrimaryTabShell 的 WidePrimaryShell 同源（下一轮精确等于 widthVp >= 840），
    // 宽屏不再无条件套窄屏的 CompactPrimaryActionLayer 位置约定。
    if (this.adaptiveState.prefersWideLayout) {
      this.WideStarMapListContent()
    } else {
      this.CompactStarMapListContent()
    }
  }

  // Issue #932：窄屏内容层 + 右下角 FAB 统一由 CompactPrimaryActionLayer 定位，
  // 删除 RelativeContainer 的 FAB alignRules 锚点；业务回调仍是 requestCreateStarMap()。
  // 创建 / 改名 / 删除框已改为窗口级 CompactDialogs（在各自 trigger 打开），页面不再自绘遮罩层。
  @Builder
  CompactStarMapListContent() {
    CompactPrimaryActionLayer({
      visible: this.hasStarMapPrimaryAction(),
      pagePadding: this.adaptiveState.pagePadding,
      onTap: () => {
        this.requestCreateStarMap()
      }
    }) {
      this.StarMapListBody()
    }
  }

  // Issue #932 复核：宽屏内容层只负责宽屏自己的创建入口位置与布局（列表 + 右下角主 FAB），
  // 与窄屏分支是并列形态——宽屏下用户实际看到的就是这个视觉，不再借名为 Compact 的组件。
  // 「+」按钮本体仍是共享的 PrimaryFab（毛玻璃 / 按压视效 / 圆环边缘高光只有一份），
  // 位置口径与 CompactPrimaryActionLayer 完全一致：Stack BottomEnd + pagePadding + 底部偏移。
  @Builder
  WideStarMapListContent() {
    Stack({ alignContent: Alignment.BottomEnd }) {
      this.StarMapListBody()

      if (this.hasStarMapPrimaryAction()) {
        PrimaryFab({
          onTap: () => {
            this.requestCreateStarMap()
          }
        })
          .margin({
            right: this.adaptiveState.pagePadding,
            bottom: PRIMARY_FAB_BOTTOM_INSET_VP
          })
      }
    }
    .width('100%')
    .height('100%')
  }

  // 列表主体：加载 / 出错重试 / 星图列表；窄屏与宽屏两个内容层共用，不复制业务。
  // 外层背景保持 background_secondary。
  @Builder
  StarMapListBody() {
    Column() {
      if (this.isLoading) {
        CompactPageState({
          kind: CompactPageStateKind.Loading,
          title: '加载中...'
        })
      } else if (this.errorMessage) {
        CompactPageState({
          kind: CompactPageStateKind.Error,
          title: this.errorMessage,
          onRetry: () => {
            this.loadStarMaps()
          }
        })
      } else {
        this.StarMapListPane()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: '星图',
        screenRole: ScreenRole.StarMap,
        screenTabIndex: 1,
        showTitleBar: !this.adaptiveState.prefersWideLayout
      }) {
        this.StarMapListContent()
      }
    } else {
      NavDestination() {
        this.StarMapListContent()
      }
      .hideTitleBar(this.adaptiveState.prefersWideLayout)
      .hideBackButton(true)
      .onBackPressed(() => handleRootBack())
      .title('星图')
      .menus(buildNavMenuItems(this.headerActions))
      .onShown((_reason: VisibilityChangeReason) => {
        hilog.info(0x0000, 'StarMapListScreen', 'StarMapList NavDestination onShown')
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.StarMap, 1)
      })
    }
  }

  // ────────────────────────────────────────
  // 星图列表内容
  // ────────────────────────────────────────

  @Builder
  StarMapListPane() {
    if (this.starMaps.length === 0) {
      // Issue #932：空状态改用公共 CompactPageState（文案不变），作为 StarMapListBody Column 的
      // 直接子项占满剩余高度，不再放在 Scroll 里自绘。
      CompactPageState({
        kind: CompactPageStateKind.Empty,
        title: '暂无星图',
        description: '点击右下角按钮创建第一个星图'
      })
    } else {
      Scroll() {
        Column() {
          ForEach(this.starMaps, (starmap: StarMapMeta) => {
            this.StarMapCard(starmap)
          }, (starmap: StarMapMeta) => starmap.starmapId)
        }
        .width('100%')
        .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        .padding({
          left: this.adaptiveState.pagePadding,
          right: this.adaptiveState.pagePadding,
          // Issue #932：列表顶部边距从原来的固定 8vp 收回统一 pagePadding。
          top: this.adaptiveState.pagePadding,
          // Issue #932：列表末端留白统一用公共 token，避免最后一张卡贴住悬浮底栏。
          bottom: COMPACT_LIST_BOTTOM_INSET_VP
        })
      }
      .width('100%')
      .height('100%')
      .scrollBar(BarState.Off)
      // Scroll 的 align 默认是 Alignment.Center，星图数量少、内容不满一屏时
      // 整列会被垂直居中，卡片悬在页面中间。这里与 HomeScreen / WorkspaceScreen 对齐，
      // 显式顶到上方，内容超出后仍正常滚动。
      .align(Alignment.Top)
    }
  }

  // ── 星图卡片：左侧标题+描述，右侧更多菜单+日期 ──
  // Issue #932：卡片度量统一到公共 token（内边距 / 圆角 / 标题字号 / 辅助字号 / 卡间距）。
  @Builder
  StarMapCard(starmap: StarMapMeta) {
    Row() {
      // 左侧：标题 + 描述
      Column() {
        Text(starmap.title)
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (starmap.description) {
          Text(starmap.description)
            .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
            .margin({ top: 6 })
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      // 右侧：更多菜单 + 更新时间
      Column() {
        // 右上：更多菜单
        Row() {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(20)
            .height(20)
            .fillColor($r('sys.color.icon_primary'))
            .onClick(() => {
              this.currentMenuStarMapId = starmap.starmapId
            })
            .bindMenu(this.StarMapMoreMenu)
        }
        .width('100%')
        .justifyContent(FlexAlign.End)

        // 右下：更新时间
        Text(this.formatTimestamp(starmap.updatedAt))
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .margin({ top: 6 })
      }
      .width(80)
      .alignItems(HorizontalAlign.End)
    }
    .width('100%')
    .padding(COMPACT_CARD_PADDING_VP)
    // Issue #932：卡片圆角用统一的 DesignTokens.cardRadius。
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ bottom: COMPACT_CARD_SPACING_VP })
    .onClick(() => this.navigateToStarMap(starmap.starmapId, starmap.title))
  }

  // ── 星图更多菜单：重命名 + 删除 ──
  @Builder
  StarMapMoreMenu() {
    Menu() {
      MenuItem({ content: '重命名' })
        .onClick(() => {
          const target = this.starMaps.find((s: StarMapMeta) => s.starmapId === this.currentMenuStarMapId)
          if (target) {
            this.startRename(target)
          }
        })

      MenuItem({ content: '删除' })
        .onClick(() => {
          const target = this.starMaps.find((s: StarMapMeta) => s.starmapId === this.currentMenuStarMapId)
          if (target) {
            this.startDelete(target)
          }
        })
    }
  }
}
