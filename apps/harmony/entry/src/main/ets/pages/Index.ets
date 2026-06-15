import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { Project, RecentEdit, WritingStats } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { getBridge, getWorkspacePath, getAppContext } from '../common/AppContext'
import { NativeWriterCoreBridge } from '../bridge/NativeWriterCoreBridge'

@Entry
@Component
struct Index {
  @State workspaceName: string = '我的写作空间'
  @State projects: Project[] = []
  @State recentEdits: RecentEdit[] = []
  @State writingStats: WritingStats | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State nativeStatus: string = ''

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null

  aboutToAppear() {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)
    this.initAndLoad()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
  }

  async initAndLoad() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const ctx = getAppContext()
      if (ctx.isNativeEnvironment()) {
        const nativeBridge = NativeWriterCoreBridge.getInstance()
        const workspacePath = getWorkspacePath()
        const initOk = await nativeBridge.initialize(workspacePath)
        if (!initOk) {
          ctx.markNativeInitFailed()
          if (ctx.canFallbackToMock()) {
            await ctx.switchToMock()
            this.bridge = getBridge()
            this.nativeStatus = 'mock-fallback'
          } else {
            const errCode = nativeBridge.getLastInitErrorCode()
            const rustError = await nativeBridge.getLastError()
            let errDesc = `未知错误(${errCode})`
            if (errCode === -1) { errDesc = '路径为空' }
            else if (errCode === -2) { errDesc = '路径编码无效' }
            else if (errCode === -3) { errDesc = '内部状态异常' }
            else if (errCode === -4) { errDesc = '工作区目录创建失败' }
            else if (errCode === -999) { errDesc = 'NAPI调用异常(SO加载失败?)' }
            this.errorMessage = `Native 核心初始化失败: ${errDesc}\n路径: ${workspacePath}\n详情: ${rustError}`
            this.isLoading = false
            return
          }
        } else {
          const statusResult = await nativeBridge.getNativeLoadStatus()
          this.nativeStatus = statusResult.success ? (statusResult.data || 'unknown') : 'error'
        }
      }

      await this.loadWorkspace()
    } catch (err) {
      this.errorMessage = `初始化时发生错误: ${err}`
      this.isLoading = false
    }
  }

  async loadWorkspace() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const workspacePath = getWorkspacePath()
      const workspaceResult = await this.bridge.openWorkspace(workspacePath)
      const statsResult = await this.bridge.getWritingStats()

      if (workspaceResult.success && workspaceResult.data) {
        this.workspaceName = workspaceResult.data.workspace.name
        this.projects = workspaceResult.data.projects
        this.recentEdits = workspaceResult.data.recentEdits
      } else {
        this.errorMessage = workspaceResult.userMessage || '加载工作区失败'
      }

      if (statsResult.success && statsResult.data) {
        this.writingStats = statsResult.data
      }
    } catch (err) {
      this.errorMessage = '加载工作区时发生错误'
    } finally {
      this.isLoading = false
    }
  }

  build() {
    Column() {
      // Header
      Row() {
        Text('写作软件')
          .fontSize(28)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Column() {
          Text(this.workspaceName)
            .fontSize(15)
            .fontColor(this.themeConfig.colors.textSecondary)

          if (this.writingStats) {
            Text(`今日: ${this.writingStats.todayChars} 字`)
              .fontSize(13)
              .fontColor(this.themeConfig.colors.textHint)
          }

          if (this.nativeStatus) {
            Text(`Native: ${this.nativeStatus}`)
              .fontSize(10)
              .fontColor(this.themeConfig.colors.textHint)
          }
        }
        .alignItems(HorizontalAlign.End)
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 20, bottom: 16 })

      if (this.isLoading) {
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
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.initAndLoad())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            // Writing stats card
            if (this.writingStats) {
              this.WritingStatsCard()
            }

            // Recent edits section
            if (this.recentEdits.length > 0) {
              this.RecentEditsSection()
            }

            // Projects section
            this.ProjectsSection()

            // Quick actions
            this.QuickActions()
          }
          .width('100%')
          .padding({ left: 20, right: 20, bottom: 32 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  WritingStatsCard() {
    Row() {
      Column() {
        Text('今日写作')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.writingStats?.todayChars || 0}`)
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
        Text('连续天数')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.writingStats?.currentStreak || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.secondary)

        Text('天')
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
        Text('总字数')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.writingStats?.totalWords || 0}`)
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
    .borderRadius(16)
    .margin({ bottom: 20 })
    .shadow({ radius: 4, color: '#1A000000', offsetY: 2 })
  }

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

      ForEach(this.recentEdits, (edit: RecentEdit) => {
        Row() {
          Column() {
            Text(edit.chapterTitle)
              .fontSize(16)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })

            Row() {
              Text(edit.projectTitle)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })

              if (edit.wordCount) {
                Text(`${edit.wordCount} 字`)
                  .fontSize(13)
                  .fontColor(this.themeConfig.colors.textHint)
              }
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(edit.editedAt))
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 14, bottom: 14 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(12)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WritingPage',
            params: {
              chapterId: edit.chapterId,
              chapterTitle: edit.chapterTitle
            }
          })
        })
      }, (edit: RecentEdit) => edit.chapterId)
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  @Builder
  ProjectsSection() {
    Column() {
      Row() {
        Text('我的作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Button('+ 新建作品')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(async () => {
            const result = await this.bridge.createProject('新作品')
            if (result.success) {
              this.loadWorkspace()
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      ForEach(this.projects, (project: Project) => {
        Row() {
          Column() {
            Text(project.name)
              .fontSize(17)
              .fontWeight(FontWeight.Medium)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })

            Row() {
              Text(`${project.volumeCount} 卷`)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })

              Text(`${project.chapterCount} 章`)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })

              if (project.totalWordCount) {
                Text(`${project.totalWordCount} 字`)
                  .fontSize(13)
                  .fontColor(this.themeConfig.colors.textHint)
              }
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Image($r('sys.media.ohos_ic_public_arrow_right'))
            .width(18)
            .height(18)
            .fillColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 16, bottom: 16 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(12)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WorkspacePage',
            params: {
              projectId: project.id,
              projectName: project.name
            }
          })
        })
      }, (project: Project) => project.id)
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  @Builder
  QuickActions() {
    Column() {
      Text('快捷操作')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })
        .width('100%')

      Grid() {
        GridItem() {
          Column() {
            Image($r('sys.media.ohos_ic_public_add'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.primary)
              .margin({ bottom: 10 })

            Text('新建章节')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(16)
        }
        .onClick(() => {
        })

        GridItem() {
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
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(16)
        }
        .onClick(() => {
          router.pushUrl({
            url: 'pages/StarMapPage',
            params: {
              starmapId: 'starmap-001',
              starmapTitle: '人物关系图'
            }
          })
        })

        GridItem() {
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
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(16)
        }
        .onClick(() => {
          router.pushUrl({ url: 'pages/SettingsPage' })
        })

        GridItem() {
          Column() {
            Image($r('app.media.ic_refresh'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.success)
              .margin({ bottom: 10 })

            Text('同步')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textPrimary)
          }
          .width('100%')
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.themeConfig.colors.surface)
          .borderRadius(16)
        }
        .onClick(() => {
          router.pushUrl({ url: 'pages/SettingsPage' })
        })
      }
      .columnsTemplate('1fr 1fr')
      .rowsTemplate('1fr 1fr')
      .columnsGap(16)
      .rowsGap(16)
      .height(220)
      .width('100%')
    }
    .width('100%')
  }

  formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes} 分钟前`
    } else if (hours < 24) {
      return `${hours} 小时前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }
}
