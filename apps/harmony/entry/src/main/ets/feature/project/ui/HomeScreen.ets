// HomeScreen — 作品主页（窄屏）。
// UI 只做事件派发和数据展示，业务逻辑全部委托 ProjectListViewModel。
// 删除了近期手写/活跃时长/净增字数/最近编辑/QuickActions。
// 新增：作品卡片（名/字数/日期/更多菜单）、新建FAB、底部一级壳层（作品/星图/统计）。

import hilog from '@ohos.hilog'
import { Routes, WorkspaceRouteParams, SettingsRouteParams } from '../../../app/navigation/Routes'
import { ProjectSummary } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ProjectListViewModel } from '../presentation/ProjectListViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'

@Component
export struct HomeScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State projects: ProjectSummary[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State showCreateDialog: boolean = false
  @State newProjectName: string = ''
  @State renameTarget: ProjectSummary | null = null
  @State renameInput: string = ''
  @State deleteTarget: ProjectSummary | null = null

  private viewModel: ProjectListViewModel = new ProjectListViewModel()
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
    this.loadData()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  private async loadData() {
    await this.viewModel.loadData()
    this.projects = this.viewModel.getProjects()
    this.isLoading = this.viewModel.isLoadingData()
    this.errorMessage = this.viewModel.getErrorMessage()
  }

  private async doCreateProject() {
    if (this.newProjectName.trim().length === 0) {
      return
    }
    const ok = await this.viewModel.createProject(this.newProjectName.trim())
    if (ok) {
      this.projects = this.viewModel.getProjects()
      this.isLoading = this.viewModel.isLoadingData()
    }
    this.showCreateDialog = false
    this.newProjectName = ''
  }

  private async doDeleteProject() {
    if (this.deleteTarget) {
      const ok = await this.viewModel.deleteProject(this.deleteTarget.id)
      if (ok) {
        this.projects = this.viewModel.getProjects()
        this.isLoading = this.viewModel.isLoadingData()
      }
    }
    this.deleteTarget = null
  }

  private async doRenameProject() {
    if (this.renameTarget && this.renameInput.trim().length > 0) {
      const ok = await this.viewModel.renameProject(this.renameTarget.id, this.renameInput.trim())
      if (ok) {
        this.projects = this.viewModel.getProjects()
      }
    }
    this.renameTarget = null
    this.renameInput = ''
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

  build() {
    NavDestination() {
      Stack() {
        // ── 主内容层 ──
        Column() {
          // 顶栏
          this.TopBar()

          // 内容区域
          if (this.isLoading) {
            this.LoadingView()
          } else if (this.errorMessage) {
            this.ErrorView()
          } else {
            this.ProjectListContent()
          }

          // 底部一级壳层
          this.BottomTabBar()
        }
        .width('100%')
        .height('100%')
        .backgroundColor(this.themeConfig.colors.background)

        // ── FAB 层：新建作品浮动按钮 ──
        if (!this.isLoading && !this.errorMessage) {
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
            this.showCreateDialog = true
            this.newProjectName = ''
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
    }
    .hideTitleBar(true)
    .onShown((_reason: VisibilityChangeReason) => {
      hilog.info(0x0000, 'HomeScreen', 'Home NavDestination onShown')
    })
  }

  // ── 顶栏：素笺 + 同步/设置按钮 ──
  @Builder
  TopBar() {
    Row() {
      Text('素笺')
        .fontSize(24)
        .fontWeight(FontWeight.Bold)
        .fontColor(this.themeConfig.colors.textPrimary)

      Blank()

      // 同步按钮
      Image($r('app.media.ic_refresh'))
        .width(24)
        .height(24)
        .fillColor(this.themeConfig.colors.textSecondary)
        .margin({ right: 16 })
        .onClick(() => {
          const settingsParam: SettingsRouteParams = { projectId: this.projects.length > 0 ? this.projects[0].id : '' }
          this.navPathStack.pushPath({ name: Routes.Settings, param: settingsParam })
        })

      // 设置按钮
      Image($r('sys.media.ohos_ic_public_more'))
        .width(24)
        .height(24)
        .fillColor(this.themeConfig.colors.textSecondary)
        .onClick(() => {
          const settingsParam: SettingsRouteParams = { projectId: this.projects.length > 0 ? this.projects[0].id : '' }
          this.navPathStack.pushPath({ name: Routes.Settings, param: settingsParam })
        })
    }
    .width('100%')
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })
  }

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
      Text(this.errorMessage)
        .fontSize(16)
        .fontColor(this.themeConfig.colors.error)
        .margin({ bottom: 16 })

      Button('重试')
        .onClick(() => {
          this.loadData()
        })
        .backgroundColor(this.themeConfig.colors.primary)
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 作品列表内容 ──
  @Builder
  ProjectListContent() {
    Scroll() {
      Column() {
        if (this.projects.length === 0) {
          this.EmptyState()
        } else {
          ForEach(this.projects, (project: ProjectSummary) => {
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

  // ── 作品卡片：左上名/左下字数/右下日期/右上更多菜单 ──
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
        // 右上：更多菜单（重命名、删除）
        Row() {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(20)
            .height(20)
            .fillColor(this.themeConfig.colors.textHint)
            .bindMenu([
              {
                value: '重命名',
                action: () => {
                  this.renameTarget = project
                  this.renameInput = project.title
                }
              },
              {
                value: '删除',
                action: () => {
                  this.deleteTarget = project
                }
              }
            ])
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
    .onClick(() => {
      const workspaceParam: WorkspaceRouteParams = { projectId: project.id, projectName: project.title }
      this.navPathStack.pushPath({ name: Routes.Workspace, param: workspaceParam })
    })
  }

  // ── 底部一级壳层：作品/星图/统计 ──
  @Builder
  BottomTabBar() {
    Row() {
      // 作品 tab（当前页）
      Column() {
        Image($r('app.media.ic_star'))
          .width(22)
          .height(22)
          .fillColor(this.themeConfig.colors.primary)
        Text('作品')
          .fontSize(12)
          .fontColor(this.themeConfig.colors.primary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        // 已在当前页，不做导航
      })

      // 星图 tab
      Column() {
        Image($r('app.media.ic_star'))
          .width(22)
          .height(22)
          .fillColor(this.themeConfig.colors.textSecondary)
        Text('星图')
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.navPathStack.pushPath({ name: Routes.StarMap })
      })

      // 统计 tab
      Column() {
        Image($r('sys.media.ohos_ic_public_more'))
          .width(22)
          .height(22)
          .fillColor(this.themeConfig.colors.textSecondary)
        Text('统计')
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.navPathStack.pushPath({ name: Routes.Stats })
      })
    }
    .width('100%')
    .height(56)
    .backgroundColor(this.themeConfig.colors.surface)
    .shadow({ radius: 4, color: '#1A000000', offsetY: -2 })
  }

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
            this.newProjectName = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surfaceVariant)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.showCreateDialog = false
              this.newProjectName = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .fontColor(this.themeConfig.colors.onPrimary)
            .onClick(() => {
              this.doCreateProject()
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

        TextInput({ text: this.renameInput, placeholder: '请输入新名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .caretColor(this.themeConfig.colors.primary)
          .onChange((value: string) => {
            this.renameInput = value
          })

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surfaceVariant)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.renameTarget = null
              this.renameInput = ''
            })

          Blank()

          Button('确认')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .fontColor(this.themeConfig.colors.onPrimary)
            .onClick(() => {
              this.doRenameProject()
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

        Text(`确定要删除"${this.deleteTarget?.title}"吗？此操作不可撤销。`)
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
              this.deleteTarget = null
            })

          Blank()

          Button('删除')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.error)
            .fontColor(this.themeConfig.colors.onError)
            .onClick(() => {
              this.doDeleteProject()
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
