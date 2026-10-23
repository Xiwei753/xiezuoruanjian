import { Routes, WorkspaceRouteParams, WritingRouteParams } from '../../../app/navigation/Routes'
import { WideLayoutMode } from '../../../app/navigation/wide/WideLayoutMode'
import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { SYNC_CONTENT_CHANGED_EVENT_ID } from '../../../feature/settings/presentation/SyncOperationController'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SPACING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  COMPACT_LIST_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
// Issue #932：窄屏 FAB 定位、窗口级对话框、公共加载/错误/空状态统一复用共享构件，
// 页面不再自绘 Stack + PrimaryFab、自绘全屏遮罩对话框。
import { CompactPrimaryActionLayer } from '../../../ui/components/CompactPrimaryActionLayer'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import {
  CompactConfirmDialogArgs,
  CompactTextDialogArgs,
  openCompactConfirmDialog,
  openCompactTextDialog
} from '../../../ui/components/CompactDialogs'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ProjectWorkspaceViewModel, VolumeCardState, ChapterRowState, RenameTarget, DeleteTarget } from '../presentation/ProjectWorkspaceViewModel'
import { ActionSlot, ActionTarget, ActionRole, ScreenRole, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsWorkspaceDestinationApi23 } from '../../../app/navigation/impl/api23/HdsWorkspaceDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'

// Issue #629 评论14 第1项：WorkspaceScreen 只保留项目/卷/章节浏览和目录操作。
// 章节正文只有 WritingScreen 一个页面级 owner。
// 删除所有 editor session/input/IME/save/wordCount/正文状态和 RightPanel() 内联写作区。
// 章节点击无论窄屏还是宽屏都进入 Routes.Writing，不要因为 isTwoPane 就换另一套编辑事务。
//
// Issue #760 评论 5821290182 问题5+7：
// - ScreenPolicy 真正消费：菜单按 action slot 渲染，Rename 接 renameVolume/renameChapter，Delete 按 requiresConfirmation
// - 事务分层收口：ViewModel 改为 @ObservedV2 + @Trace，页面改为 @ComponentV2 + @Local，不再维护 @State 副本
// - 页面只派发意图，ViewModel 内部管理 async 和状态
@ComponentV2
export struct WorkspaceScreen {
  // Issue #768 评论5838296806：通过 @Param 从父组件接收 NavPathStack，不再依赖全局 activeTabStack
  @Param navPathStack: NavPathStack = new NavPathStack()
  // Issue #972 复核：宽窄形态由外壳唯一传入，页面不再读取 AdaptiveContext 自行判断。
  @Param layoutMode: WideLayoutMode = WideLayoutMode.Compact
  // 保留 navCoordinator 用于 safePopWithStack 调用
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: ProjectWorkspaceViewModel = new ProjectWorkspaceViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  // Issue #768 评论5835171266 第3项：API23+ 时用 HdsNavDestination 替代 NavDestination，
  // 获得 HDS 沉浸光感材质效果。
  @Local useHdsNav: boolean = false
  // Issue #784 评论5857397503 第4点：API < 23 时通过 ScreenPolicy 加载 HeaderTrailing 动作，
  // 传入 NavDestination .menus() 渲染右上角操作项。
  @Local headerActions: ActionSlot[] = []

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  async aboutToAppear() {
    hilog.info(0x0000, 'WorkspaceScreen', 'route name=Workspace')
    // Issue #768 评论5835171266 第3项：检测 API Level，决定是否使用 HdsNavDestination。
    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    // Issue #905：窄屏恢复 HDS 原生标题栏，页头动作由 HdsWorkspaceDestinationApi23 的
    // stackBuilder 自绘。这里的 headerActions 只供 API < 23 的 NavDestination.menus 兜底。
    this.headerActions = resolveScreenPolicy(ScreenRole.ProjectWorkspace).actionSlots
    const paramsArr = this.navPathStack.getParamByName(Routes.Workspace) as WorkspaceRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.viewModel.projectId = params.projectId
      this.viewModel.projectName = params.projectName
      this.viewModel.loadData(params.projectId).then((): void => {
        if (params.focusVolumeId !== undefined && params.focusVolumeId.length > 0) {
          this.viewModel.expandVolume(params.focusVolumeId)
        }
      })
      // Issue #988：记录真实页面与项目；不再用只有 projectId 的旧快照。
      AppContinuationService.getInstance().updateCurrentScreen({
        screen: 'workspace',
        tabIndex: 0,
        projectId: params.projectId
      })
    }

    // 注册同步完成事件监听
    this.subscribeSyncEvent()

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
  }

  aboutToDisappear() {
    // 清理观察者
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    // 取消同步完成事件监听
    this.unsubscribeSyncEvent()
  }

  // ─── UI 事件处理（只做事件派发，不写业务逻辑） ───

  handleToggleVolume(volumeId: string) {
    this.viewModel.toggleVolume(volumeId)
  }

  handleCreateVolume() {
    this.viewModel.createVolume()
  }

  /** 检查 PrimaryAction 中是否有 CreateVolume（决定 FAB 是否显示） */
  private hasCreateVolumePrimaryAction(): boolean {
    for (const slot of this.viewModel.primaryActions) {
      if (slot.role === ActionRole.CreateVolume) {
        return true
      }
    }
    return false
  }

  handleCreateChapter(volumeId: string) {
    this.viewModel.createChapter(volumeId)
  }

  handleRetry() {
    this.viewModel.retry()
  }

  // Issue #629 评论14 第1项：章节点击统一进入 Routes.Writing，不再内联编辑。
  // 窄屏和宽屏都走同一条导航路径，屏幕宽度变化只改布局，不换 TextEditSession owner。
  private navigateToWriting(chapterId: string, chapterTitle: string, volumeId: string): void {
    const writingParam: WritingRouteParams = {
      chapterId: chapterId,
      chapterTitle: chapterTitle,
      projectId: this.viewModel.projectId,
      volumeId: volumeId
    }
    this.navPathStack.pushPath({
      name: Routes.Writing,
      param: writingParam
    })
  }

  // Issue #768 评论5838296806：返回链使用 @Param 传入的 NavPathStack，不再依赖全局 activeTabStack。
  private handleBack(): void {
    this.navCoordinator.safePopWithStack(this.navPathStack)
  }

  // ─── Context Action 处理 ───

  /** 处理卷的 Context action（按 action slot 渲染的菜单项点击） */
  private handleVolumeContextAction(slot: ActionSlot): void {
    const volumeId = this.viewModel.currentMenuVolumeId
    if (slot.role === ActionRole.Rename) {
      this.startRenameVolume(volumeId)
    } else if (slot.role === ActionRole.Delete) {
      this.startDeleteVolume(volumeId, slot.requiresConfirmation)
    } else if (slot.role === ActionRole.MoveEarlier) {
      this.viewModel.moveVolumeEarlier(volumeId)
    } else if (slot.role === ActionRole.MoveLater) {
      this.viewModel.moveVolumeLater(volumeId)
    }
  }

  /** 处理章节的 Context action（按 action slot 渲染的菜单项点击） */
  private handleChapterContextAction(slot: ActionSlot): void {
    const chapterId = this.viewModel.currentMenuChapterId
    if (slot.role === ActionRole.Rename) {
      this.startRenameChapter(chapterId)
    } else if (slot.role === ActionRole.Delete) {
      this.startDeleteChapter(chapterId, slot.requiresConfirmation)
    } else if (slot.role === ActionRole.MoveEarlier) {
      this.viewModel.moveChapterEarlier(chapterId)
    } else if (slot.role === ActionRole.MoveLater) {
      this.viewModel.moveChapterLater(chapterId)
    }
  }

  // ─── Issue #932：窗口级输入 / 确认对话框（替换页面自绘全屏遮罩） ───
  // 业务状态仍归 ViewModel：这里只把「渲染开关」改为在动作触发点直接打开系统对话框。

  /** 重命名卷：先走 ViewModel 的 startRenameVolume，再打开窗口级输入对话框 */
  private startRenameVolume(volumeId: string): void {
    this.viewModel.startRenameVolume(volumeId)
    this.openRenameDialog()
  }

  /** 重命名章节：先走 ViewModel 的 startRenameChapter，再打开窗口级输入对话框 */
  private startRenameChapter(chapterId: string): void {
    this.viewModel.startRenameChapter(chapterId)
    this.openRenameDialog()
  }

  /** 打开重命名输入框：初始文本取 ViewModel 的 renameInput，确认/取消仍走原 ViewModel 回调 */
  private openRenameDialog(): void {
    const target: RenameTarget | null = this.viewModel.renameTarget
    if (target === null) {
      return
    }
    let confirmed: boolean = false
    const args: CompactTextDialogArgs = {
      title: target.type === 'volume' ? '重命名卷' : '重命名章节',
      placeholder: '请输入新名称',
      initialText: this.viewModel.renameInput,
      // 卷/章节改名只有一个输入栏。
      secondaryLabel: '',
      secondaryPlaceholder: '',
      secondaryInitialText: '',
      confirmLabel: '确认',
      onConfirm: (text: string, _secondaryText: string): void => {
        confirmed = true
        this.viewModel.renameInput = text
        this.viewModel.confirmRename()
      },
      onCancel: (): void => {
        this.viewModel.cancelRename()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理 ViewModel 的重命名状态；
    // 已被确认文案消费的对话框不再重复清理。
    openCompactTextDialog(this.getUIContext(), args, (): void => {
      if (!confirmed) {
        this.viewModel.cancelRename()
      }
    })
  }

  /** 删除卷：requiresConfirmation 为 false 时 ViewModel 直接删除，只有需要确认才弹窗口级确认框 */
  private startDeleteVolume(volumeId: string, requiresConfirmation: boolean): void {
    this.viewModel.startDeleteVolume(volumeId, requiresConfirmation)
    this.openDeleteConfirmDialog()
  }

  /** 删除章节：requiresConfirmation 为 false 时 ViewModel 直接删除，只有需要确认才弹窗口级确认框 */
  private startDeleteChapter(chapterId: string, requiresConfirmation: boolean): void {
    this.viewModel.startDeleteChapter(chapterId, requiresConfirmation)
    this.openDeleteConfirmDialog()
  }

  /** 打开删除确认框：确认/取消仍走原 ViewModel 的 confirmDelete / cancelDelete */
  private openDeleteConfirmDialog(): void {
    const target: DeleteTarget | null = this.viewModel.deleteTarget
    if (target === null) {
      return
    }
    let confirmed: boolean = false
    const args: CompactConfirmDialogArgs = {
      title: '确认删除',
      message: `确定要删除"${target.name}"吗？此操作不可撤销。`,
      destructive: true,
      confirmLabel: getContext(this).resourceManager.getStringSync($r('app.string.delete').id),
      onConfirm: (): void => {
        confirmed = true
        this.viewModel.confirmDelete()
      },
      onCancel: (): void => {
        this.viewModel.cancelDelete()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理 ViewModel 的删除目标；
    // 已被确认文案消费的对话框不再重复清理。
    openCompactConfirmDialog(this.getUIContext(), args, (): void => {
      if (!confirmed) {
        this.viewModel.cancelDelete()
      }
    })
  }

  /** 获取 action 的显示标签 */
  private getActionLabel(role: ActionRole): string {
    if (role === ActionRole.Rename) {
      return '重命名'
    } else if (role === ActionRole.Delete) {
      return getContext(this).resourceManager.getStringSync($r('app.string.delete').id)
    } else if (role === ActionRole.MoveEarlier) {
      return '上移'
    } else if (role === ActionRole.MoveLater) {
      return '下移'
    }
    return role
  }

  // 格式化时间（UI 层负责，因为需要资源字符串本地化）
  private formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) {
      return getContext(this).resourceManager.getStringSync($r('app.string.just_now').id)
    } else if (minutes < 60) {
      return getContext(this).resourceManager.getStringSync($r('app.string.minutes_ago_format').id).replace('%s', `${minutes}`)
    } else if (hours < 24) {
      return getContext(this).resourceManager.getStringSync($r('app.string.hours_ago_format').id).replace('%s', `${hours}`)
    } else if (days < 7) {
      return getContext(this).resourceManager.getStringSync($r('app.string.days_ago_format').id).replace('%s', `${days}`)
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  // ─── 同步完成事件监听 ───

  // emitter.on(event, callback) 返回 void，没有订阅 ID 可保存。
  // 保存回调函数本身，才能用 emitter.off(eventId, callback) 精确解绑本页面，
  // 不会像 emitter.off(eventId) 那样一次取消所有页面（首页 / 工作区）的订阅。
  private syncChangedHandler: ((data: emitter.EventData) => void) | null = null

  /**
   * 注册同步完成事件监听
   * 收到通知后刷新工作区数据
   */
  private subscribeSyncEvent(): void {
    if (this.syncChangedHandler !== null) {
      return
    }
    const handler = (_data: emitter.EventData): void => {
      hilog.info(0x0000, 'WorkspaceScreen', 'received sync content changed event, refreshing data')
      // 同步完成后刷新工作区数据
      if (this.viewModel.projectId) {
        this.viewModel.loadData(this.viewModel.projectId)
      }
    }
    this.syncChangedHandler = handler
    try {
      emitter.on({ eventId: SYNC_CONTENT_CHANGED_EVENT_ID }, handler)
      hilog.info(0x0000, 'WorkspaceScreen', 'subscribed to sync content changed event')
    } catch (e) {
      this.syncChangedHandler = null
      hilog.error(0x0000, 'WorkspaceScreen', 'failed to subscribe sync event: %{public}s', String(e))
    }
  }

  /**
   * 取消同步完成事件监听
   */
  private unsubscribeSyncEvent(): void {
    const handler = this.syncChangedHandler
    if (handler !== null) {
      try {
        emitter.off(SYNC_CONTENT_CHANGED_EVENT_ID, handler)
        hilog.info(0x0000, 'WorkspaceScreen', 'unsubscribed from sync content changed event')
      } catch (e) {
        hilog.error(0x0000, 'WorkspaceScreen', 'failed to unsubscribe sync event: %{public}s', String(e))
      }
      this.syncChangedHandler = null
    }
  }

  // ========== 构建 UI ==========

  // Issue #768 评论5835171266 第3项：NavDestination 内容部分抽取为 @Builder，
  // 供 HdsNavDestination 和 NavDestination 两个条件渲染分支共用。
  @Builder
  NavContent() {
    Column() {
      // Issue #932：原来 if (sizeClass === Compact) else 两边都调同一个布局，收成单一窄屏布局调用。
      // 卷章浏览不再按窗口宽度切换第二套结构。
      this.CompactWorkspaceLayout()
    }
    .width('100%')
    .height('100%')
    // Issue #888：页面底色用系统 background_secondary，与 HomeScreen 一致。
    // 卷卡用 ohos_id_color_card_bg，与页面底色拉开色阶，深浅模式自动跟随系统。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    // Issue #768：启用 NavDestination 原生标题栏，显示项目名和系统返回按钮。
    // Issue #768 评论5835171266 第3项：API23+ 用 HdsNavDestination 替代 NavDestination，
    // 获得 HDS 沉浸光感材质效果；API < 23 保持普通 NavDestination。
    if (this.useHdsNav) {
      // Issue #773 评论5844518092：改用 V2 官方尾随闭包传 @BuilderParam，
      // 让 NavContent 始终在页面上下文执行，避免 this 变成壳组件导致 adaptiveState undefined。
      HdsWorkspaceDestinationApi23({
        title: this.viewModel.projectName,
        onBack: () => { this.handleBack() },
        onShown: () => { NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.ProjectWorkspace, 0) },
        screenRole: ScreenRole.ProjectWorkspace,
        // Issue #827 评论 1 第 6 点：宽屏顶部由 PrimaryTabShell 提供，
        // 工作区页再画一条 HDS 标题栏就会在双栏内容上方留出整宽空白。
        // Issue #905：窄屏恢复 HDS 原生标题栏（showTitleBar = !wideLayout），
        // 与作品页 / 星图页 / 统计页共用同一个壳。
        showTitleBar: this.layoutMode !== WideLayoutMode.Wide
      }) {
        this.NavContent()
      }
    } else {
      NavDestination() {
        this.NavContent()
      }
      .hideTitleBar(this.layoutMode === WideLayoutMode.Wide)
      .title(this.viewModel.projectName)
      .menus(buildNavMenuItems(this.headerActions))
      .onBackPressed(() => {
        this.handleBack()
        return true
      })
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.ProjectWorkspace, 0)
      })
    }
  }

  // ────────────────────────────────────────
  // 窄屏布局（Compact — Issue #760 卷卡可展开设计）
  // ────────────────────────────────────────

  @Builder
  CompactWorkspaceLayout() {
    // Issue #932：窄屏主操作 FAB 统一由 CompactPrimaryActionLayer 承载，
    // 删除页面自己画的 Stack(Alignment.BottomEnd) + PrimaryFab 定位。
    CompactPrimaryActionLayer({
      visible: !this.viewModel.isLoadingState && !this.viewModel.errorMessageState && this.hasCreateVolumePrimaryAction(),
      pagePadding: this.adaptiveState.pagePadding,
      onTap: () => {
        this.handleCreateVolume()
      }
    }) {
      Column() {
        // ── 内容区 ──
        if (this.viewModel.isLoadingState) {
          CompactPageState({
            kind: CompactPageStateKind.Loading,
            title: getContext(this).resourceManager.getStringSync($r('app.string.loading').id)
          })
        } else if (this.viewModel.errorMessageState) {
          CompactPageState({
            kind: CompactPageStateKind.Error,
            title: this.viewModel.errorMessageState,
            onRetry: () => {
              this.handleRetry()
            }
          })
        } else if (this.viewModel.volumeCardStates.length === 0) {
          // Issue #932：卷章为空时也走公共空状态视觉（图标 + 标题 + 说明）。
          CompactPageState({
            kind: CompactPageStateKind.Empty,
            title: getContext(this).resourceManager.getStringSync($r('app.string.no_volumes_chapters').id),
            description: getContext(this).resourceManager.getStringSync($r('app.string.click_new_volume_to_start').id),
            icon: $r('sys.symbol.plus')
          })
        } else {
          Scroll() {
            Column() {
              ForEach(this.viewModel.volumeCardStates, (cardState: VolumeCardState) => {
                this.VolumeCard(cardState)
              }, (cardState: VolumeCardState) => cardState.volumeId)
            }
            .width('100%')
            .justifyContent(FlexAlign.Start)
            .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
            .padding({
              left: this.adaptiveState.pagePadding,
              right: this.adaptiveState.pagePadding,
              // Issue #905：HDS 原生标题栏自行避让状态栏，内容区不再人工叠加安全区。
              top: this.adaptiveState.pagePadding,
              // Issue #932：列表末端留白统一走窄屏 token。
              bottom: COMPACT_LIST_BOTTOM_INSET_VP
            })
          }
          .layoutWeight(1)
          .scrollBar(BarState.Off)
          .align(Alignment.Top)
        }
      }
      .width('100%')
      .height('100%')
      .alignItems(HorizontalAlign.Center)
      // Issue #932：窄屏页面底色统一 background_secondary。
      .backgroundColor($r('sys.color.background_secondary'))
    }
  }

  // ── 卷卡（可展开） ──
  // 收起状态：左上卷名 / 右上新增章节按钮+更多菜单 / 左下章节数 / 中下展开箭头 / 右下卷总字数
  // 展开后：同一张卷卡下方显示章节列表
  // Issue #932：卡面、圆角、内边距与字号统一走窄屏卡片 token；章节展开结构保留。
  @Builder
  VolumeCard(cardState: VolumeCardState) {
    Column() {
      // ── 卷卡头部（上半部分） ──
      // 左上：卷名    右上：新增章节按钮 + 更多菜单
      Row() {
        Text(cardState.volumeTitle)
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        // 新增章节按钮
        Button({ type: ButtonType.Normal }) {
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(28)
            .fontColor([$r('sys.color.icon_primary')])
        }
        .width(48)
        .height(48)
        .backgroundColor(Color.Transparent)
        .onClick(() => this.handleCreateChapter(cardState.volumeId))

        // 更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
        Image($r('sys.media.ohos_ic_public_more'))
          .width(24)
          .height(24)
          .fillColor($r('sys.color.icon_primary'))
          .margin({ left: 8 })
          .onClick(() => {
            this.viewModel.currentMenuVolumeId = cardState.volumeId
          })
          .bindMenu(this.VolumeMoreMenu)
      }
      .width('100%')
      .padding({ bottom: 8 })

      // ── 卷卡底部信息行 ──
      // 左下：章节数    中下：展开箭头    右下：卷总字数
      Row() {
        // 左下：章节数
        Text(getContext(this).resourceManager.getStringSync($r('app.string.chapter_count_format').id).replace('%s', `${cardState.chapterCount}`))
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.font_secondary'))
        // 中下：展开/收起箭头
        Row() {
          Image(this.viewModel.isVolumeExpanded(cardState.volumeId)
            ? $r('sys.media.ohos_ic_public_arrow_down')
            : $r('sys.media.ohos_ic_public_arrow_right'))
            .width(14)
            .height(14)
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)

        // 右下：卷总字数
        Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${cardState.totalWordCount}`))
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.font_secondary'))
      }
      .width('100%')
      .onClick(() => this.handleToggleVolume(cardState.volumeId))

      // ── 章节列表（展开后显示） ──
      if (this.viewModel.isVolumeExpanded(cardState.volumeId)) {
        if (cardState.chapters.length === 0) {
          Column() {
            Text($r('app.string.no_chapters'))
              .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
              .fontColor($r('sys.color.font_secondary'))
              .padding({ top: 16, bottom: 16 })
          }
          .width('100%')
        } else {
          Column() {
            ForEach(cardState.chapters, (chapterState: ChapterRowState) => {
              this.ChapterRow(chapterState)
            }, (chapterState: ChapterRowState) => chapterState.chapterId)
          }
          .width('100%')
        }
      }
    }
    .width('100%')
    .margin({ bottom: COMPACT_CARD_SPACING_VP })
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .clip(true)
    // Issue #888：卷卡加系统卡片底色，与页面底色 background_secondary 拉开色阶。
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .shadow({ radius: 2, offsetX: 0, offsetY: 1  })
  }

  // ── 章节行 ──
  // 左：章节名    右侧：最后编辑日期    最右：章节字数
  // Issue #932：章节行只保留纵向内边距，左右对齐由卷卡内边距统一；辅助字走窄屏卡片 token。
  @Builder
  ChapterRow(chapterState: ChapterRowState) {
    Row() {
      // 左：章节名
      Text(chapterState.chapterTitle)
        .fontSize(15)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      // 右侧：最后编辑日期
      Text(this.formatTime(chapterState.lastEditedTime))
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ left: 8, right: 8 })

      // 最右：章节字数
      Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${chapterState.wordCount}`))
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_secondary'))
      // 章节更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
      Image($r('sys.media.ohos_ic_public_more'))
        .width(20)
        .height(20)
        .fillColor($r('sys.color.icon_primary'))
        .margin({ left: 8 })
        .onClick(() => {
          this.viewModel.currentMenuChapterId = chapterState.chapterId
        })
        .bindMenu(this.ChapterMoreMenu)
    }
    .width('100%')
    .padding({ top: 12, bottom: 12 })
    .onClick(() => this.navigateToWriting(chapterState.chapterId, chapterState.chapterTitle, chapterState.volumeId))
  }

  // ── 卷更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuVolumeId 传递上下文
  @Builder
  VolumeMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Volume), (slot: ActionSlot) => {
        MenuItem({ content: this.getActionLabel(slot.role) })
          .onClick(() => this.handleVolumeContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ── 章节更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuChapterId 传递上下文
  @Builder
  ChapterMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Chapter), (slot: ActionSlot) => {
        MenuItem({ content: this.getActionLabel(slot.role) })
          .onClick(() => this.handleChapterContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ── 重命名 / 删除确认对话框已迁至窗口级 ──
  // Issue #932：原页面自绘的全屏 Column + '#80000000' 遮罩已删除，
  // 统一走 ui/components/CompactDialogs 的 openCompactTextDialog / openCompactConfirmDialog，
  // 触发点见 openRenameDialog / openDeleteConfirmDialog。
}
