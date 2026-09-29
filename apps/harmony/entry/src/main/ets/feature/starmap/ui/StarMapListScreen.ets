// StarMapListScreen — 星图列表页。
// 作为一级"星图"tab 的真正根页面：
// - aboutToAppear 调 bridge.listStarMaps() 只加载列表，不打开任何星图
// - 卡片列表展示星图（标题/描述/更新时间），点击进入画布
// - 右下角 FAB 新建星图（必须先询问名称，不允许无名星图）
// - 卡片更多菜单支持重命名/删除
// - 空列表显示"暂无星图"

import hilog from '@ohos.hilog'
import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import { StarMapMeta } from '../../../corebridge/dto/StarMapDtos'
import { getBridge } from '../../../app/AppContext'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { PRIMARY_FAB_SIZE_VP, PRIMARY_FAB_BOTTOM_INSET_VP } from '../../../ui/theme/LayoutTokens'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ScreenRole, ActionSlot } from '../../../corebridge/dto/PlatformDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../ui/adaptive/ScreenPolicyBridge'
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
      const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(ScreenRole.StarMap)
      if (policy !== null) {
        this.headerActions = policy.actionSlots
      }
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
      const result = await bridge.listStarMaps()
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

  private startRename(starmap: StarMapMeta): void {
    this.renameTarget = starmap
    this.renameInput = starmap.title
  }

  private async confirmRename(): Promise<void> {
    if (this.renameTarget === null || this.renameInput.trim().length === 0) {
      return
    }
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.renameStarMap(this.renameTarget!.starmapId, this.renameInput.trim())
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

  private startDelete(starmap: StarMapMeta): void {
    this.deleteTarget = starmap
  }

  private async confirmDelete(): Promise<void> {
    if (this.deleteTarget === null) {
      return
    }
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.deleteStarMap(this.deleteTarget!.starmapId)
      if (result.success) {
        this.starMaps = this.starMaps.filter((s: StarMapMeta) => s.starmapId !== this.deleteTarget!.starmapId)
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
    Stack() {
      Column() {
        if (this.isLoading) {
          this.LoadingView()
        } else if (this.errorMessage) {
          this.ErrorView()
        } else {
          this.StarMapListPane()
        }
      }
      .width('100%')
      .height('100%')

      // ── FAB 层：新建星图浮动按钮 ──
      if (!this.isLoading && !this.errorMessage) {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(28)
        }
        .width(PRIMARY_FAB_SIZE_VP)
        .height(PRIMARY_FAB_SIZE_VP)
        .margin({
          right: this.adaptiveState.pagePadding,
          bottom: PRIMARY_FAB_BOTTOM_INSET_VP
        })
        .onClick(() => {
          this.showCreateDialog = true
          this.newStarMapName = ''
          this.newStarMapDescription = ''
        })
      }

      // ── 对话框层 ──
      if (this.showCreateDialog) {
        this.CreateDialog()
      }
      if (this.renameTarget !== null) {
        this.RenameDialog()
      }
      if (this.deleteTarget !== null) {
        this.DeleteConfirmDialog()
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
        screenTabIndex: 1
      }) {
        this.StarMapListContent()
      }
    } else {
      NavDestination() {
        this.StarMapListContent()
      }
      .hideTitleBar(false)
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
    Stack({ alignContent: Alignment.BottomEnd }) {
      Scroll() {
        Column() {
          if (this.starMaps.length === 0) {
            this.EmptyState()
          } else {
            ForEach(this.starMaps, (starmap: StarMapMeta) => {
              this.StarMapCard(starmap)
            }, (starmap: StarMapMeta) => starmap.starmapId)
          }
        }
        .width('100%')
        .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 8, bottom: 16 })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
  }

  // ── 加载中 ──
  @Builder
  LoadingView() {
    Column() {
      LoadingProgress()
        .width(48)
        .height(48)
      Text('加载中...')
        .fontSize(14)
        .margin({ top: 12 })
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 错误视图 ──
  @Builder
  ErrorView() {
    Column() {
      Text(this.errorMessage)
        .fontSize(16)
        .margin({ bottom: 16 })

      Button('重试')
        .onClick(() => this.loadStarMaps())
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 空状态 ──
  @Builder
  EmptyState() {
    Column() {
      Text('暂无星图')
        .fontSize(18)
        .margin({ bottom: 8 })

      Text('点击右下角按钮创建第一个星图')
        .fontSize(14)
    }
    .width('100%')
    .height(200)
    .justifyContent(FlexAlign.Center)
  }

  // ── 星图卡片：左侧标题+描述，右侧更多菜单+日期 ──
  @Builder
  StarMapCard(starmap: StarMapMeta) {
    Row() {
      // 左侧：标题 + 描述
      Column() {
        Text(starmap.title)
          .fontSize(17)
          .fontWeight(FontWeight.Medium)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (starmap.description) {
          Text(starmap.description)
            .fontSize(13)
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
          .fontSize(13)
          .margin({ top: 6 })
      }
      .width(80)
      .alignItems(HorizontalAlign.End)
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 16, bottom: 16 })
    .borderRadius(DesignTokens.radiusMd)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ bottom: 10 })
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

  // ────────────────────────────────────────
  // 对话框
  // ────────────────────────────────────────

  // ── 新建星图对话框 ──
  @Builder
  CreateDialog() {
    Column() {
      Column() {
        Text('新建星图')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ placeholder: '请输入星图名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.newStarMapName = value
          })

        TextInput({ placeholder: '请输入星图描述（可选）' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .margin({ top: 12 })
          .onChange((value: string) => {
            this.newStarMapDescription = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.showCreateDialog = false
              this.newStarMapName = ''
              this.newStarMapDescription = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .enabled(this.newStarMapName.trim().length > 0)
            .onClick(() => {
              this.createStarMap()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }

  // ── 重命名对话框 ──
  @Builder
  RenameDialog() {
    Column() {
      Column() {
        Text('重命名星图')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ text: this.renameInput, placeholder: '请输入新名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.renameInput = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.cancelRename()
            })

          Blank()

          Button('确认')
            .fontSize(14)
            .height(36)
            .enabled(this.renameInput.trim().length > 0)
            .onClick(() => {
              this.confirmRename()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }

  // ── 删除确认对话框 ──
  @Builder
  DeleteConfirmDialog() {
    Column() {
      Column() {
        Text('确认删除')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 8 })

        Text(`确定要删除"${this.deleteTarget?.title}"吗？此操作不可撤销。`)
          .fontSize(14)
          .margin({ bottom: 16 })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.cancelDelete()
            })

          Blank()

          Button('删除')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.confirmDelete()
            })
        }
        .width('100%')
      }
      .width('80%')
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }
}
