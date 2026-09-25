// HomeScreen — 作品主页。
// Issue #760 评论 5822350374 问题1/3：
// - 改为 @ComponentV2 + @Local viewModel（与 WorkspaceScreen 一致）
// - 页面只派发事件，ViewModel 内部管理 async 和状态
// - 顶栏同步调用 viewModel.performSync()，不跳设置页
// - 作品菜单按 ScreenPolicy 的 Project Context actions 渲染
// - 删除 Home 内部的 BottomTabBar（全局只有 AppNavigation 一层底栏）
// - 宽屏布局恢复 #760 之前的实现（WritingStatsCard、RecentEditsSection、ProjectsSection、QuickActions）
// - 窄屏布局保留当前的新作品卡布局
// Issue #767：FAB 改原生 Button + 去 M3 颜色改系统 token

import hilog from '@ohos.hilog'
import { Routes, WorkspaceRouteParams, SettingsRouteParams, WritingRouteParams } from '../../../app/navigation/Routes'
import { ProjectSummary, WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { RecentEdit } from '../../../corebridge/dto/ResultEnvelope'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ProjectListViewModel } from '../presentation/ProjectListViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ActionSlot, ActionTarget, ActionRole } from '../../../corebridge/dto/PlatformDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, WidthClass } from '../../../corebridge/dto/PlatformDtos'

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

  // Issue #768 评论5838624438：星图快捷入口改为回调切换一级 tab。
  // 星图已是一级 tab，不再 push 进 homeStack（会制造 currentTab/screenRole 分叉）。
  private navigateToStarMap(): void {
    this.onOpenStarMap()
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

  /** 宽屏删除作品 — AlertDialog 确认后调用 ViewModel 删除（恢复 #760 之前宽屏布局） */
  private deleteProjectWithConfirm(projectId: string): void {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_project'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: {
        value: $r('app.string.delete'),
        action: () => { this.viewModel.deleteProject(projectId) }
      }
    })
  }

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
          if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
            // ── 宽屏布局：恢复 #760 之前的实现 ──
            this.TwoPaneLayout()
          } else {
            // ── 窄屏布局：保留当前的新作品卡布局 ──
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
    }
    .hideTitleBar(true)
    .onShown((_reason: VisibilityChangeReason) => {
      hilog.info(0x0000, 'HomeScreen', 'Home NavDestination onShown')
      NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.ProjectList, 0)
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
        Text($r('app.string.app_title'))
          .fontSize(28)
          .fontWeight(FontWeight.Bold)

        Blank()

        Column() {
          Text(this.viewModel.appDataRoot ? this.extractWorkspaceName(this.viewModel.appDataRoot) : '我的写作空间')
            .fontSize(15)

          if (this.viewModel.writingStats) {
            Text(`近期手写: ${this.viewModel.writingStats!.totalHumanTypedChars}`)
              .fontSize(13)
          }

          if (this.viewModel.nativeStatus) {
            Text(`Native: ${this.viewModel.nativeStatus}`)
              .fontSize(10)
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
          .margin({ bottom: 8 })

        Text(`${this.viewModel.writingStats?.totalHumanTypedChars || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)

        Text('字')
          .fontSize(14)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)

      Divider()
        .vertical(true)
        .height(40)

      Column() {
        Text('活跃时长')
          .fontSize(14)
          .margin({ bottom: 8 })

        Text(`${Math.floor((this.viewModel.writingStats?.totalActiveSeconds || 0) / 60)}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)

        Text('分钟')
          .fontSize(14)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)

      Divider()
        .vertical(true)
        .height(40)

      Column() {
        Text('净增字数')
          .fontSize(14)
          .margin({ bottom: 8 })

        Text(`${this.viewModel.writingStats?.totalNetDeltaChars || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)

        Text('字')
          .fontSize(14)
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Center)
    }
    .width('100%')
    .padding({ left: 20, right: 20, top: 20, bottom: 20 })
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

        Blank()

        Text('查看全部')
          .fontSize(14)
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
              .margin({ bottom: 6 })

            Row() {
              Text(`Project ${this.viewModel.recentEdit!.projectId}`)
                .fontSize(13)
                .margin({ right: 16 })
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(this.viewModel.recentEdit!.timestamp))
            .fontSize(13)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 14, bottom: 14 })
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

        Blank()

        Button($r('app.string.new_project'))
          .fontSize(14)
          .height(32)
          .onClick(() => {
            this.viewModel.createProject(getContext(this).resourceManager.getStringSync($r('app.string.default_new_project_title').id))
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
              .margin({ bottom: 6 })
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          // 删除按钮（红色 ×）— 恢复 #760 之前的宽屏布局
          Text('×')
            .fontSize(18)
            .width(28)
            .height(28)
            .textAlign(TextAlign.Center)
            .borderRadius(DesignTokens.radiusLg)
            .margin({ right: 12 })
            .onClick(() => {
              this.deleteProjectWithConfirm(project.id)
            })

          Image($r('sys.media.ohos_ic_public_arrow_right'))
            .width(18)
            .height(18)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 16, bottom: 16 })
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
        .margin({ bottom: 16 })
        .width('100%')

      GridRow({ columns: this.adaptiveState.gridColumns, gutter: 16 }) {
        GridCol({ span: 1 }) {
          Column() {
            Image($r('sys.media.ohos_ic_public_add'))
              .width(36)
              .height(36)
              .margin({ bottom: 10 })

            Text('新章节')
              .fontSize(14)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => {
          // 新章节 — 空操作（#760 之前宽屏布局原样恢复）
        })

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_star'))
              .width(36)
              .height(36)
              .margin({ bottom: 10 })

            Text('星图')
              .fontSize(14)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.navigateToStarMap())

        GridCol({ span: 1 }) {
          Column() {
            Image($r('sys.media.ohos_ic_public_more'))
              .width(36)
              .height(36)
              .margin({ bottom: 10 })

            Text('设置')
              .fontSize(14)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.navigateToSettings())

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_refresh'))
              .width(36)
              .height(36)
              .margin({ bottom: 10 })

            Text('同步')
              .fontSize(14)
          }
          .width('100%')
          .height(90)
          .justifyContent(FlexAlign.Center)
          .borderRadius(DesignTokens.cardRadius)
        }
        .onClick(() => this.navigateToSettings())
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

      // ── FAB 层：新建作品浮动按钮（由 PrimaryAction(CreateProject) 是否存在决定） ──
      // Issue #768：改用 ButtonType.ROUNDED_RECTANGLE，保持系统默认材质
      if (!this.viewModel.isLoading && !this.viewModel.errorMessage && this.hasCreateProjectPrimaryAction()) {
        Button({ type: ButtonType.ROUNDED_RECTANGLE }) {
          Image($r('sys.media.ohos_ic_public_add'))
            .width(24)
            .height(24)
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

  // ── 窄屏顶栏：app_title + 按 ScreenPolicy HeaderTrailing 顺序渲染按钮 ──
  @Builder
  SinglePaneTopBar() {
    Row() {
      Text($r('app.string.app_title'))
        .fontSize(24)
        .fontWeight(FontWeight.Bold)

      Blank()

      // 按 HeaderTrailing order 顺序渲染：Sync(10) → Settings(30)
      // Issue #760 评论 5825003601：Search slot 保留在 Core ScreenPolicy 中（不影响 Android/全局搜索），
      // 但 Harmony 窄屏本轮不渲染 Search 图标，避免出现无动作的可点击控件。
      ForEach(this.viewModel.headerTrailingActions, (slot: ActionSlot) => {
        if (slot.role === ActionRole.Sync) {
          Image($r('app.media.ic_refresh'))
            .width(24)
            .height(24)
            .margin({ right: 16 })
            .onClick(() => this.handleSync())
        } else if (slot.role === ActionRole.Settings) {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(24)
            .height(24)
            .onClick(() => this.navigateToSettings())
        }
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
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
  @Builder
  EmptyState() {
    Column() {
      Text('暂无作品')
        .fontSize(18)
        .margin({ bottom: 8 })

      Text('点击右下角按钮创建第一个作品')
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
