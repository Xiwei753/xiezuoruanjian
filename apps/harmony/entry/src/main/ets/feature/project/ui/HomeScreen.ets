// HomeScreen — 作品主页。
// Issue #760 评论 5822350374 问题1/3：
// - 改为 @ComponentV2 + @Local viewModel（与 WorkspaceScreen 一致）
// - 页面只派发事件，ViewModel 内部管理 async 和状态
// - 顶栏同步调用 viewModel.performSync()，不跳设置页
// - 作品菜单按 ScreenPolicy 的 Project Context actions 渲染
// - 删除 Home 内部的 BottomTabBar（全局只有 AppNavigation 一层底栏）
// Issue #827 评论 2：宽屏布局按结构图重做，删除 #760 恢复回来的
// WritingStatsCard / RecentEditsSection / ProjectsSection / QuickActions 四块统计树。
// 宽屏首页只剩作品卡流：作品卡 + 「+」卡从内容区左上开始排，
// 「+」是网格里的一张卡而不是 FAB / 按钮，永久跟在最后一张作品卡后面。
// Issue #767：FAB 改原生 Button + 去 M3 颜色改系统 token

import hilog from '@ohos.hilog'
import { Routes, WorkspaceRouteParams } from '../../../app/navigation/Routes'
import { ProjectSummary } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { PRIMARY_FAB_BOTTOM_INSET_VP } from '../../../ui/theme/LayoutTokens'
import { PrimaryFab } from '../../../ui/components/PrimaryFab'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ProjectListViewModel } from '../presentation/ProjectListViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ActionSlot, ActionTarget, ActionRole } from '../../../corebridge/dto/PlatformDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, WidthClass } from '../../../corebridge/dto/PlatformDtos'
import { NavigationHeaderActionDispatcher } from '../../../app/navigation/NavigationHeaderActionDispatcher'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../ui/adaptive/ScreenPolicyBridge'
import { handleRootBack } from '../../../app/navigation/RootBackHandler'
import { defaultLayoutMetrics } from '../../../ui/adaptive/LayoutPolicyHelper'

// Issue #827 评论 1 第 7 点：命名框 / 确认框的内容最大宽度（vp）。
// 原来写 .width('80%')，在 1244vp 宽屏上对话框会被拉到近 1000vp 宽，
// 一行输入框横跨整屏。改成「手机上能缩、宽屏有上限」：
// 宽度 100% 加上限，窄屏按屏宽收缩，宽屏停在 DIALOG_CONTENT_MAX_WIDTH。
const DIALOG_CONTENT_MAX_WIDTH: number = 420

// Issue #827 评论 2：宽屏作品卡之间的间距（vp），作品卡与「+」卡共用同一份。
const WIDE_PROJECT_CARD_GAP: number = 16

@ComponentV2
export struct HomeScreen {
  // Issue #768 评论5838624438：星图快捷入口改为回调切换一级 tab，不再 push 进 homeStack。
  // 星图已是一级 tab，push 进作品栈会制造 currentTab/screenRole 分叉。
  @Param onOpenStarMap: () => void = (): void => {}

  // navPathStack 通过 NavigationTransactionCoordinator 获取，不使用 @Consume（V1 装饰器，V2 中不可用）
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: ProjectListViewModel = new ProjectListViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local useHdsNav: boolean = false
  @Local headerActions: ActionSlot[] = []

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  async aboutToAppear() {
    hilog.info(0x0000, 'HomeScreen', 'route name=Home')
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    AppContinuationService.getInstance().clear()
    this.viewModel.loadData()

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(ScreenRole.ProjectList)
      if (policy !== null) {
        this.headerActions = policy.actionSlots
      }
    }
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ─── UI 事件处理（只做事件派发，不写业务逻辑） ───

  private handleRetry(): void {
    this.viewModel.retry()
  }

  private navigateToWorkspace(projectId: string, projectName: string): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const workspaceParam: WorkspaceRouteParams = { projectId, projectName }
    navPathStack.pushPath({ name: Routes.Workspace, param: workspaceParam })
  }

  // Issue #768 评论5838624438：星图快捷入口改为回调切换一级 tab。
  // 星图已是一级 tab，不再 push 进 homeStack（会制造 currentTab/screenRole 分叉）。
  private navigateToStarMap(): void {
    this.onOpenStarMap()
  }

  // Issue #827 评论 1 第 3 点：删除宽屏「最近编辑」区后，navigateToRecentEdit()、
  // deleteProjectWithConfirm()、extractWorkspaceName() 已无调用方
  // （删除入口走 ViewModel 的 DeleteConfirmDialog，作品名直接取 ProjectSummary.title）。
  // 按仓库规则直接删掉，不留死代码，也不留第二套入口。

  // Issue #827 评论 1 第 3 点 / 评论 2：新建作品的唯一入口。
  // 「+」卡、窄屏 FAB、空状态入口都只调它，它只打开命名框；
  // 真正的 createProject(name) 只能从命名框的「创建」回调进入，
  // 不允许任何地方拿默认名直接建作品。
  private requestCreateProject(): void {
    this.viewModel.newProjectName = ''
    this.viewModel.showCreateDialog = true
  }

  // ─── Context Action 处理 ───

  /** 处理项目的 Context action（按 action slot 渲染的菜单项点击） */
  private handleProjectContextAction(slot: ActionSlot): void {
    const projectId = this.viewModel.currentMenuProjectId
    if (slot.role === ActionRole.Rename) {
      this.viewModel.startRenameProject(projectId)
    } else if (slot.role === ActionRole.Delete) {
      this.viewModel.startDeleteProject(projectId, slot.requiresConfirmation)
    }
  }

  /** 获取 action 的显示标签 */
  private getActionLabel(role: ActionRole): string {
    if (role === ActionRole.Rename) {
      return '重命名'
    } else if (role === ActionRole.Delete) {
      return '删除'
    } else if (role === ActionRole.MoveEarlier) {
      return '上移'
    } else if (role === ActionRole.MoveLater) {
      return '下移'
    }
    return role
  }

  // ─── 格式化工具 ───

  /** 检查 PrimaryAction 中是否有 CreateProject（决定 FAB 是否显示） */
  private hasCreateProjectPrimaryAction(): boolean {
    for (const slot of this.viewModel.primaryActions) {
      if (slot.role === ActionRole.CreateProject) {
        return true
      }
    }
    return false
  }

  private formatWordCount(count: number): string {
    if (count < 10000) {
      return `${count} 字`
    }
    const wan = count / 10000.0
    return `${wan.toFixed(1)} 万字`
  }

  // Issue #827 评论 3 第 2 点：宽屏作品卡的最后编辑时间用绝对时间，
  // 形如 26/8/11 22:37 —— 与手绘稿一致，也与 Linux_Qt ProjectCard.qml 同格式。
  // 刻意不复用上面的相对时间 formatTime()/formatDate()：
  // 那两个是窄屏列表用的「3分钟前 / 2天前」，作品封面要的是可定位的日历时间。
  private formatProjectCardTime(isoString: string): string {
    if (!isoString) {
      return ''
    }
    const date = new Date(isoString)
    if (isNaN(date.getTime())) {
      return ''
    }
    const year = String(date.getFullYear()).slice(-2)
    const month = String(date.getMonth() + 1)
    const day = String(date.getDate())
    const hour = String(date.getHours()).padStart(2, '0')
    const minute = String(date.getMinutes()).padStart(2, '0')
    return `${year}/${month}/${day} ${hour}:${minute}`
  }

  private formatDate(isoString: string): string {
    if (!isoString) {
      return ''
    }
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes}分钟前`
    } else if (hours < 24) {
      return `${hours}小时前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  private formatTime(isoString: string): string {
    const date = new Date(isoString)
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
  HomeContent() {
    Stack() {
      Column() {
        if (this.adaptiveState.sizeClass === WidthClass.Expanded) {
          // ── 宽屏：只有作品卡流，从内容区左上开始排 ──
          this.WideHomeLayout()
        } else {
          // ── 窄屏 / 中屏：作品卡列表 + 右下 FAB ──
          this.SinglePaneLayout()
        }
      }
      .width('100%')
      .height('100%')

      // ── 对话框层 ──
      if (this.viewModel.showCreateDialog) {
        this.CreateDialog()
      }
      if (this.viewModel.renameTarget !== null) {
        this.RenameDialog()
      }
      if (this.viewModel.deleteTarget !== null) {
        this.DeleteConfirmDialog()
      }
    }
    .width('100%')
    .height('100%')
    // 页面底色用系统 background_secondary。
    // 实机实测本机浅色模式：background_primary = #FFFFFF（纯白，与背景融为一体，悬浮底栏
    // 背后没有任何可折射内容）；background_secondary = #F2F3F5（带蓝相的浅冷白，
    // 正是"我的华为"那一类系统页面的底色）。书卡用 ohos_id_color_card_bg = #FFFFFF，
    // 与底色拉开 13/255 色阶。
    // 官方《组件颜色与界面背景颜色相近》faqs-arkui-1119：组件背景色要与页面背景色区分，
    // 否则组件区域无法突出显示，悬浮底栏的沉浸光感/背景模糊也无处显色。
    // 用系统 token 而非硬编码色值，深浅色模式与系统主题自动跟随。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: $r('app.string.app_title'),
        screenRole: ScreenRole.ProjectList,
        screenTabIndex: 0,
        // Issue #827 评论 1 第 6 点：宽屏顶部由 PrimaryTabShell 的 WidePrimaryShell
        // 统一提供（同步 / 搜索 / 设置 常驻右上），页面再画一条 HDS 标题栏
        // 就会在内容上方留出一条整宽空白。
        showTitleBar: this.adaptiveState.sizeClass !== WidthClass.Expanded
      }) {
        this.HomeContent()
      }
    } else {
      NavDestination() {
        this.HomeContent()
      }
      .hideTitleBar(this.adaptiveState.sizeClass === WidthClass.Expanded)
      .hideBackButton(true)
      .onBackPressed(() => handleRootBack())
      .title($r('app.string.app_title'))
      .menus(buildNavMenuItems(this.headerActions))
      .onShown((_reason: VisibilityChangeReason) => {
        hilog.info(0x0000, 'HomeScreen', 'Home NavDestination onShown')
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.ProjectList, 0)
      })
    }
  }

  // ────────────────────────────────────────
  // 宽屏布局（Issue #827 评论 2 结构图）
  // ────────────────────────────────────────

  // 作品卡宽度：Core 的 project_card_min_width_dp 是共用最小宽度，
  // 不在平台端另写一套 180。Core 契约没解析出来时退回 LayoutPolicyHelper 的兜底值。
  private get wideProjectCardWidth(): number {
    const planMetrics = this.adaptiveState.layoutPlan.metrics
    if (planMetrics.projectCardMinWidthDp > 0) {
      return planMetrics.projectCardMinWidthDp
    }
    return defaultLayoutMetrics().projectCardMinWidthDp
  }

  // 书封面卡是竖向的：高 / 宽 = 4 / 3，与手绘稿一致。
  private get wideProjectCardHeight(): number {
    return this.wideProjectCardWidth * (4 / 3)
  }

  // 宽屏首页：内容区只有作品卡流，从左上开始排，不居中、不锁 maxWidth。
  // 「+」卡是网格里的一张普通卡（不是 FAB、不是按钮），永远跟在最后一张作品后面。
  @Builder
  WideHomeLayout() {
    Column() {
      if (this.viewModel.isLoading) {
        this.LoadingView()
      } else if (this.viewModel.errorMessage) {
        this.ErrorView()
      } else {
        Scroll() {
          Flex({ wrap: FlexWrap.Wrap }) {
            ForEach(this.viewModel.projects, (project: ProjectSummary) => {
              this.WideProjectCard(project)
            }, (project: ProjectSummary) => project.id)

            this.CreateProjectCard()
          }
          .width('100%')
          .padding({
            left: this.adaptiveState.pagePadding,
            right: this.adaptiveState.pagePadding,
            top: this.adaptiveState.pagePadding,
            bottom: this.adaptiveState.pagePadding
          })
        }
        .layoutWeight(1)
        .width('100%')
        .scrollBar(BarState.Off)
        .align(Alignment.Top)
      }
    }
    .width('100%')
    .height('100%')
  }

  // 宽屏作品卡：竖向书封面 —— 作品名 / 卷章数 / 右下总字数 / 左下最后编辑时间。
  // 排版语义与 Linux_Qt 的 ProjectCard.qml 一致，右上折角是书封面的视觉身份。
  @Builder
  WideProjectCard(project: ProjectSummary) {
    Stack({ alignContent: Alignment.TopEnd }) {
      Column() {
        // 作品名
        Text(project.title)
          .fontSize(17)
          .fontWeight(FontWeight.Medium)
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .width('100%')
          .textAlign(TextAlign.Start)

        Divider()
          .width('100%')
          .color($r('sys.color.comp_background_gray'))
          .margin({ top: 12, bottom: 12 })

        // 卷 / 章数量
        Text(`${project.volumeCount}卷 ${project.chapterCount}章`)
          .fontSize(13)
          .width('100%')
          .textAlign(TextAlign.Start)

        Blank()

        // 右下：总字数
        Text(this.formatWordCount(project.totalWordCount))
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .width('100%')
          .textAlign(TextAlign.End)

        // 左下：最后编辑时间（绝对时间，与 Qt 端同格式）
        Text(this.formatProjectCardTime(project.updatedAt))
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .width('100%')
          .textAlign(TextAlign.Start)
          .margin({ top: 4 })
      }
      .width('100%')
      .height('100%')
      .padding(16)
      .alignItems(HorizontalAlign.Start)
      .justifyContent(FlexAlign.Start)

      // 右上折角：书封面的视觉身份，不承载任何点击
      Row()
        .width(18)
        .height(18)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius({ topLeft: DesignTokens.radiusSm })
    }
    .width(this.wideProjectCardWidth)
    .height(this.wideProjectCardHeight)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ right: WIDE_PROJECT_CARD_GAP, bottom: WIDE_PROJECT_CARD_GAP })
    .shadow({ radius: DesignTokens.shadowLight.radius, color: DesignTokens.shadowLight.color, offsetY: DesignTokens.shadowLight.offsetY })
    .onClick(() => this.navigateToWorkspace(project.id, project.title))
    // Issue #827 评论 3 第 3 点：宽屏卡不能丢掉重命名 / 删除入口，
    // 但草图里没有常驻三点按钮，所以走系统长按菜单，与 Qt 端整卡右键菜单同语义。
    // 普通作品卡不是星图的「长按后继续拖动拉线」场景，可以直接用 LongPress。
    .onTouch((event: TouchEvent) => {
      if (event.type === TouchType.Down) {
        this.viewModel.currentMenuProjectId = project.id
      }
    })
    .bindContextMenu(this.ProjectMoreMenu, ResponseType.LongPress)
  }

  // 「+」卡：与作品卡同宽同高，卡片中央只有一个大「+」。
  // 它是网格成员而不是工具按钮，所以跟真实作品一起排版、一起换行、一起滚动；
  // 点击只打开命名框，输入名称后才真正创建作品。
  @Builder
  CreateProjectCard() {
    Column() {
      Text('+')
        .fontSize(44)
        .fontWeight(FontWeight.Normal)
        .fontColor($r('sys.color.icon_secondary'))
    }
    .width(this.wideProjectCardWidth)
    .height(this.wideProjectCardHeight)
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .borderRadius(DesignTokens.cardRadius)
    // Issue #827 评论 3 第 5 点：普通 1px 边框。草图里它和作品卡是同一种外框，
    // 区别只在中央一个大「+」，不要自行发明虚线这一种卡片视觉。
    .border({ width: 1, color: $r('sys.color.comp_background_gray') })
    .margin({ right: WIDE_PROJECT_CARD_GAP, bottom: WIDE_PROJECT_CARD_GAP })
    .accessibilityText('新建作品')
    .onClick(() => this.requestCreateProject())
  }

  // ────────────────────────────────────────
  // 窄屏布局（保留当前的新作品卡布局）
  // ────────────────────────────────────────

  @Builder
  SinglePaneLayout() {
    Stack({ alignContent: Alignment.BottomEnd }) {
      Column() {
        // 不再自绘窄屏顶栏：页面标题已由 HdsAppDestinationApi23 的 HDS 标题栏承载，
        // 页内再画一份会出现两个"素笺"。窄屏/宽屏一律只保留顶栏那一个。
        // 内容区域
        if (this.viewModel.isLoading) {
          this.LoadingView()
        } else if (this.viewModel.errorMessage) {
          this.ErrorView()
        } else {
          this.ProjectListContent()
        }
      }
      .width('100%')
      .height('100%')

      // ── FAB 层：新建作品浮动按钮（由 PrimaryAction(CreateProject) 是否存在决定） ──
      // Issue #778：FAB 改用 Stack BottomEnd 对齐 + margin 定位，不再用绝对坐标。
      // 配色/材质统一到 PrimaryFab，与工作区页、星图页同源。
      // Issue #827 评论 2：FAB 只在非宽屏出现。宽屏的「+」卡是网格成员，
      // 两边不能同时留着两套新建入口。
      if (!this.viewModel.isLoading && !this.viewModel.errorMessage && this.hasCreateProjectPrimaryAction()) {
        PrimaryFab({
          onTap: () => {
            this.requestCreateProject()
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

  // ────────────────────────────────────────
  // 共享组件
  // ────────────────────────────────────────

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
      Text(this.viewModel.errorMessage)
        .fontSize(16)
        .margin({ bottom: 16 })

      Button('重试')
        .onClick(() => this.handleRetry())
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 作品列表内容（窄屏） ──
  @Builder
  ProjectListContent() {
    Scroll() {
      Column() {
        if (this.viewModel.projects.length === 0) {
          this.EmptyState()
        } else {
          ForEach(this.viewModel.projects, (project: ProjectSummary) => {
            this.ProjectCard(project)
          }, (project: ProjectSummary) => project.id)
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

  // ── 空状态 ──
  // Issue #827 评论 1 第 3 点：空状态也是一个新建入口，走同一个 requestCreateProject()，
  // 不在这里直接拿默认名建作品。文案不再写死「右下角按钮」，因为宽屏根本没有 FAB，
  // 宽屏的空状态就是那张「+」卡。
  @Builder
  EmptyState() {
    Column() {
      Text('暂无作品')
        .fontSize(18)
        .margin({ bottom: 8 })

      Text('点击「+」创建第一个作品')
        .fontSize(14)
    }
    .width('100%')
    .height(200)
    .justifyContent(FlexAlign.Center)
  }

  // ── 作品卡片（窄屏）：左上名/左下字数/右下日期/右上更多菜单 ──
  @Builder
  ProjectCard(project: ProjectSummary) {
    Row() {
      // 左侧：作品名 + 总字数
      Column() {
        Text(project.title)
          .fontSize(17)
          .fontWeight(FontWeight.Medium)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Text(this.formatWordCount(project.totalWordCount))
          .fontSize(13)
          .margin({ top: 6 })
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      // 右侧：更多菜单 + 最后编辑日期
      Column() {
        // 右上：更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
        Row() {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(20)
            .height(20)
            .fillColor($r('sys.color.icon_primary'))
            .onClick(() => {
              this.viewModel.currentMenuProjectId = project.id
            })
            .bindMenu(this.ProjectMoreMenu)
        }
        .width('100%')
        .justifyContent(FlexAlign.End)

        // 右下：最后编辑日期
        Text(this.formatDate(project.updatedAt))
          .fontSize(13)
          .margin({ top: 6 })
      }
      .width(80)
      .alignItems(HorizontalAlign.End)
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 16, bottom: 16 })
    .borderRadius(DesignTokens.radiusMd)
    // 卡片用系统卡片面色（浅色模式实测 #FFFFFF），与页面底色 background_secondary 拉开色阶。
    // 官方 faqs-arkui-1119：组件背景色要与页面背景色区分，否则组件区域无法突出显示。
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ bottom: 10 })
    .onClick(() => this.navigateToWorkspace(project.id, project.title))
  }

  // ── 作品更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuProjectId 传递上下文
  @Builder
  ProjectMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Project), (slot: ActionSlot) => {
        MenuItem({ content: this.getActionLabel(slot.role) })
          .onClick(() => this.handleProjectContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ────────────────────────────────────────
  // 对话框（Issue #767：默认路径不读 themeConfig.colors.*，用系统默认样式）
  // ────────────────────────────────────────

  // ── 新建作品对话框 ──
  @Builder
  CreateDialog() {
    Column() {
      Column() {
        Text('新建作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ placeholder: '请输入作品名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.viewModel.newProjectName = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.showCreateDialog = false
              this.viewModel.newProjectName = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.onCreateProject()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('100%')
      .constraintSize({ maxWidth: DIALOG_CONTENT_MAX_WIDTH })
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .padding({ left: 24, right: 24 })
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }

  // ── 重命名对话框 ──
  @Builder
  RenameDialog() {
    Column() {
      Column() {
        Text('重命名作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ text: this.viewModel.renameInput, placeholder: '请输入新名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.viewModel.renameInput = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.cancelRename()
            })

          Blank()

          Button('确认')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.confirmRename()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('100%')
      .constraintSize({ maxWidth: DIALOG_CONTENT_MAX_WIDTH })
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .padding({ left: 24, right: 24 })
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

        Text(`确定要删除"${this.viewModel.deleteTarget?.title}"吗？此操作不可撤销。`)
          .fontSize(14)
          .margin({ bottom: 16 })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.cancelDelete()
            })

          Blank()

          Button('删除')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.viewModel.confirmDelete()
            })
        }
        .width('100%')
      }
      .width('100%')
      .constraintSize({ maxWidth: DIALOG_CONTENT_MAX_WIDTH })
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .padding({ left: 24, right: 24 })
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }
}
