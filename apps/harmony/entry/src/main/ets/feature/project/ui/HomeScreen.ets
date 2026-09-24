// HomeScreen — 作品主页。
// Issue #760 评论 5822350374 问题1/3：
// - 改为 @ComponentV2 + @Local viewModel（与 WorkspaceScreen 一致）
// - 页面只派发事件，ViewModel 内部管理 async 和状态
// - 顶栏同步调用 viewModel.performSync()，不跳设置页
// - 作品菜单按 ScreenPolicy 的 Project Context actions 渲染
// - 删除 Home 内部的 BottomTabBar（全局只有 AppNavigation 一层底栏）
// - 宽屏布局恢复 #760 之前的实现（WritingStatsCard、RecentEditsSection、ProjectsSection、QuickActions）
// - 窄屏布局保留当前的新作品卡布局

import hilog from '@ohos.hilog'
import { Routes, WorkspaceRouteParams, SettingsRouteParams, WritingRouteParams, StarMapRouteParams } from '../../../app/navigation/Routes'
import { ProjectSummary, WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { RecentEdit } from '../../../corebridge/dto/ResultEnvelope'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ProjectListViewModel } from '../presentation/ProjectListViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ActionSlot, ActionTarget, ActionRole } from '../../../corebridge/dto/PlatformDtos'

@ComponentV2
export struct HomeScreen {
  // navPathStack 通过 NavigationTransactionCoordinator 获取，不使用 @Consume（V1 装饰器，V2 中不可用）
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: ProjectListViewModel = new ProjectListViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
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

  private handleSync(): void {
    this.viewModel.performSync()
  }

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

  private navigateToSettings(): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const settingsParam: SettingsRouteParams = {
      projectId: this.viewModel.projects.length > 0 ? this.viewModel.projects[0].id : ''
    }
    navPathStack.pushPath({ name: Routes.Settings, param: settingsParam })
  }

  private navigateToStarMap(): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const starMapParam: StarMapRouteParams = { starmapId: 'starmap-001', starmapTitle: 'Character Relations' }
    navPathStack.pushPath({ name: Routes.StarMap, param: starMapParam })
  }

  private navigateToRecentEdit(): void {
    if (!this.viewModel.recentEdit) {
      return
    }
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const edit = this.viewModel.recentEdit!
    const writingParam: WritingRouteParams = {
      chapterId: edit.chapterId,
      chapterTitle: `Chapter ${edit.chapterId}`,
      projectId: edit.projectId,
      volumeId: edit.volumeId
    }
    navPathStack.pushPath({ name: Routes.Writing, param: writingParam })
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

  private formatWordCount(count: number): string {
    if (count < 10000) {
      return `${count} 字`
    }
    const wan = count / 10000.0
    return `${wan.toFixed(1)} 万字`
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

  private extractWorkspaceName(path: string): string {
    if (!path) {
      return '我的写作空间'
    }
    const segments = path.replace(/\\/g, '/').split('/')
    const last = segments[segments.length - 1]
    return last || '我的写作空间'
  }

  // ========== 构建 UI ==========

  build() {
    NavDestination() {
      Stack() {
        Column() {
          if (this.adaptiveState.isTwoPane) {
            // ── 宽屏布局：恢复 #760 之前的实现 ──
            this.TwoPaneLayout()
          } else {
            // ── 窄屏布局：保留当前的新作品卡布局 ──
            this.SinglePaneLayout()
          }
        }
        .width('100%')
        .height('100%')
        .backgroundColor(this.themeConfig.colors.background)

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
    }
    .hideTitleBar(true)
    .onShown((_reason: VisibilityChangeReason) => {
      hilog.info(0x0000, 'HomeScreen', 'Home NavDestination onShown')
    })
  }

  // ────────────────────────────────────────
  // 宽屏布局（恢复 #760 之前的实现，数据源改为 ViewModel）
  // ────────────────────────────────────────

  @Builder
  TwoPaneLayout() {
    Column() {
      // Header — app_title + workspace name + writing stats + native status
      Row() {
        Text('素笺')
          .fontSize(28)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Column() {
          Text(this.viewModel.appDataRoot ? this.extractWorkspaceName(this.viewModel.appDataRoot) : '我的写作空间')
            .fontSize(15)
            .fontColor(this.themeConfig.colors.textSecondary)

          if (this.viewModel.writingStats) {
            Text(`近期手写: ${this.viewModel.writingStats!.totalHumanTypedChars}`)
              .fontSize(13)
              .fontColor(this.themeConfig.colors.textHint)
          }

          if (this.viewModel.nativeStatus) {
            Text(`Native: ${this.viewModel.nativeStatus}`)
              .fontSize(10)
              .fontColor(this.themeConfig.colors.textHint)
          }
        }
        .alignItems(HorizontalAlign.End)
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 16 })

      // 内容区域
      if (this.viewModel.isLoading) {
        this.LoadingView()
      } else if (this.viewModel.errorMessage) {
        this.ErrorView()
      } else {
        Scroll() {
          Column() {
            // Writing stats card
            if (this.viewModel.writingStats) {
              this.WritingStatsCard()
            }

            // Recent edits section
            if (this.viewModel.recentEdit != null) {
              this.RecentEditsSection()
            }

            // Projects section
            this.ProjectsSection()

            // Quick actions
            this.QuickActions()
          }
          .width('100%')
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
          .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 32 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .align(Alignment.Top)
      }
    }
    .width('100%')
    .height('100%')
  }

  // ── 写作统计卡片（宽屏布局） ──
  @Builder
  WritingStatsCard() {
    Row() {
      Column() {
        Text('近期手写')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.viewModel.writingStats?.totalHumanTypedChars || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.primary)

        Text('字')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)

      Divider()
        .vertical(true)
        .height(40)
        .color(this.themeConfig.colors.divider)

      Column() {
        Text('活跃时长')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${Math.floor((this.viewModel.writingStats?.totalActiveSeconds || 0) / 60)}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.secondary)

        Text('分钟')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)

      Divider()
        .vertical(true)
        .height(40)
        .color(this.themeConfig.colors.divider)

      Column() {
        Text('净增字数')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.viewModel.writingStats?.totalNetDeltaChars || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.success)

        Text('字')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)
    }
    .width('100%')
    .padding({ left: 20, right: 20, top: 20, bottom: 20 })
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.cardRadius)
    .margin({ bottom: 20 })
    .shadow({ radius: DesignTokens.shadowLight.radius, color: DesignTokens.shadowLight.color, offsetY: DesignTokens.shadowLight.offsetY })
    .constraintSize({ maxWidth: 600 })
  }

  // ── 最近编辑区（宽屏布局） ──
  @Builder
  RecentEditsSection() {
    Column() {
      Row() {
        Text('最近编辑')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Text('查看全部')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.primary)
          .onClick(() => {
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      if (this.viewModel.recentEdit != null) {
        Row() {
          Column() {
            Text(`Chapter ${this.viewModel.recentEdit!.chapterId}`)
              .fontSize(16)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })

            Row() {
              Text(`Project ${this.viewModel.recentEdit!.projectId}`)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(this.viewModel.recentEdit!.timestamp))
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 14, bottom: 14 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(DesignTokens.radiusMd)
        .margin({ bottom: 10 })
        .onClick(() => this.navigateToRecentEdit())
      }
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  // ── 作品列表区（宽屏布局） ──
  @Builder
  ProjectsSection() {
    Column() {
      Row() {
        Text('我的作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Button('新建作品')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(() => {
            this.viewModel.showCreateDialog = true
            this.viewModel.newProjectName = ''
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      ForEach(this.viewModel.projects, (project: ProjectSummary) => {
        Row() {
          Column() {
            Text(project.title)
              .fontSize(17)
              .fontWeight(FontWeight.Medium)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })

            Text(this.formatWordCount(project.totalWordCount))
              .fontSize(13)
              .fontColor(this.themeConfig.colors.textSecondary)
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          // 更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
          Image($r('sys.media.ohos_ic_public_more'))
            .width(24)
            .height(24)
            .fillColor(this.themeConfig.colors.textSecondary)
            .margin({ left: 8 })
            .onClick(() => {
              this.viewModel.currentMenuProjectId = project.id
            })
            .bindMenu(this.ProjectMoreMenu)

          Image($r('sys.media.ohos_ic_public_arrow_right'))
            .width(18)
            .height(18)
            .fillColor(this.themeConfig.colors.textHint)
            .margin({ left: 8 })
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 16, bottom: 16 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(DesignTokens.radiusMd)
        .margin({ bottom: 10 })
        .onClick(() => this.navigateToWorkspace(project.id, project.title))
      }, (project: ProjectSummary) => project.id)
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  // ── 快速操作区（宽屏布局） ──
  @Builder
  QuickActions() {
    Column() {
      Text('快速操作')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })
        .width('100%')

      GridRow({ columns: this.adaptiveState.gridColumns, gutter: 16 }) {
        GridCol({ span: 1 }) {
          Column() {
            Image($r('sys.media.ohos_ic_public_add'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.primary)
              .margin({ bottom: 10 })

            Text('新建作品')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => {
          this.viewModel.showCreateDialog = true
          this.viewModel.newProjectName = ''
        })

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_star'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.secondary)
              .margin({ bottom: 10 })

            Text('星图')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.navigateToStarMap())

        GridCol({ span: 1 }) {
          Column() {
            Image($r('sys.media.ohos_ic_public_more'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.textSecondary)
              .margin({ bottom: 10 })

            Text('设置')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.navigateToSettings())

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_refresh'))
              .width(36)
              .height(36)
              .fillColor(this.viewModel.isSyncing ? this.themeConfig.colors.primary : this.themeConfig.colors.success)
              .margin({ bottom: 10 })

            Text('同步')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.handleSync())
      }
      .width('100%')
    }
    .width('100%')
  }

  // ────────────────────────────────────────
  // 窄屏布局（保留当前的新作品卡布局）
  // ────────────────────────────────────────

  @Builder
  SinglePaneLayout() {
    Stack() {
      Column() {
        // 顶栏
        this.SinglePaneTopBar()

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
      .backgroundColor(this.themeConfig.colors.background)

      // ── FAB 层：新建作品浮动按钮 ──
      if (!this.viewModel.isLoading && !this.viewModel.errorMessage) {
        Row() {
          Stack() {
            Circle({ width: 56, height: 56 })
              .fill(this.themeConfig.colors.primary)
              .shadow({ radius: 8, color: '#40000000', offsetY: 4 })
            Image($r('sys.media.ohos_ic_public_add'))
              .width(24)
              .height(24)
              .fillColor(this.themeConfig.colors.onPrimary)
          }
          .width(56)
          .height(56)
        }
        .width(56)
        .height(56)
        .position({ x: '100%', y: '100%' })
        .markAnchor({ x: '100%', y: '100%' })
        .offset({ x: -this.adaptiveState.pagePadding - 16, y: -56 - 16 })
        .onClick(() => {
          this.viewModel.showCreateDialog = true
          this.viewModel.newProjectName = ''
        })
      }
    }
    .width('100%')
    .height('100%')
  }

  // ── 窄屏顶栏：素笺 + 同步/设置按钮 ──
  @Builder
  SinglePaneTopBar() {
    Row() {
      Text('素笺')
        .fontSize(24)
        .fontWeight(FontWeight.Bold)
        .fontColor(this.themeConfig.colors.textPrimary)

      Blank()

      // 同步按钮 — 执行真正的同步动作（通过 ViewModel），不再跳设置页
      Image($r('app.media.ic_refresh'))
        .width(24)
        .height(24)
        .fillColor(this.viewModel.isSyncing ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
        .margin({ right: 16 })
        .onClick(() => this.handleSync())

      // 设置按钮
      Image($r('sys.media.ohos_ic_public_more'))
        .width(24)
        .height(24)
        .fillColor(this.themeConfig.colors.textSecondary)
        .onClick(() => this.navigateToSettings())
    }
    .width('100%')
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })
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
        .color(this.themeConfig.colors.primary)
      Text('加载中...')
        .fontSize(14)
        .fontColor(this.themeConfig.colors.textHint)
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
        .fontColor(this.themeConfig.colors.error)
        .margin({ bottom: 16 })

      Button('重试')
        .onClick(() => this.handleRetry())
        .backgroundColor(this.themeConfig.colors.primary)
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
  @Builder
  EmptyState() {
    Column() {
      Text('暂无作品')
        .fontSize(18)
        .fontColor(this.themeConfig.colors.textSecondary)
        .margin({ bottom: 8 })

      Text('点击右下角按钮创建第一个作品')
        .fontSize(14)
        .fontColor(this.themeConfig.colors.textHint)
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
          .fontColor(this.themeConfig.colors.textPrimary)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Text(this.formatWordCount(project.totalWordCount))
          .fontSize(13)
          .fontColor(this.themeConfig.colors.textSecondary)
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
            .fillColor(this.themeConfig.colors.textHint)
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
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ top: 6 })
      }
      .width(80)
      .alignItems(HorizontalAlign.End)
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 16, bottom: 16 })
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 10 })
    .onClick(() => this.navigateToWorkspace(project.id, project.title))
  }

  // ── 作品更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuProjectId 传递上下文
  @Builder
  ProjectMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Project), (slot: ActionSlot) => {
        MenuItem(this.getActionLabel(slot.role))
          .onClick(() => this.handleProjectContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ────────────────────────────────────────
  // 对话框
  // ────────────────────────────────────────

  // ── 新建作品对话框 ──
  @Builder
  CreateDialog() {
    Column() {
      Column() {
        Text('新建作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 16 })

        TextInput({ placeholder: '请输入作品名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .caretColor(this.themeConfig.colors.primary)
          .onChange((value: string) => {
            this.viewModel.newProjectName = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surfaceVariant)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.viewModel.showCreateDialog = false
              this.viewModel.newProjectName = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .fontColor(this.themeConfig.colors.onPrimary)
            .onClick(() => {
              this.viewModel.onCreateProject()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .backgroundColor(this.themeConfig.colors.surface)
      .borderRadius(DesignTokens.radiusLg)
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
        Text('重命名作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 16 })

        TextInput({ text: this.viewModel.renameInput, placeholder: '请输入新名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .caretColor(this.themeConfig.colors.primary)
          .onChange((value: string) => {
            this.viewModel.renameInput = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surfaceVariant)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.viewModel.cancelRename()
            })

          Blank()

          Button('确认')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .fontColor(this.themeConfig.colors.onPrimary)
            .onClick(() => {
              this.viewModel.confirmRename()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .backgroundColor(this.themeConfig.colors.surface)
      .borderRadius(DesignTokens.radiusLg)
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
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 8 })

        Text(`确定要删除"${this.viewModel.deleteTarget?.title}"吗？此操作不可撤销。`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 16 })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surfaceVariant)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.viewModel.cancelDelete()
            })

          Blank()

          Button('删除')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.error)
            .fontColor(this.themeConfig.colors.onError)
            .onClick(() => {
              this.viewModel.confirmDelete()
            })
        }
        .width('100%')
      }
      .width('80%')
      .padding(20)
      .backgroundColor(this.themeConfig.colors.surface)
      .borderRadius(DesignTokens.radiusLg)
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }
}
