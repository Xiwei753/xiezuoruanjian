import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { Project, RecentEdit, WritingStats, AppSummary, ThemePaletteRecordDto, BuiltinThemeDto } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ThemeMode } from '../system/HarmonyThemeAdapter'
import { DesignTokens } from '../system/DesignTokens'
import { getBridge, getWorkspacePath, getAppContext } from '../common/AppContext'
import { NativeWriterCoreBridge } from '../bridge/NativeWriterCoreBridge'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../common/AdaptiveContext'
import { WidthClass, ShellMode } from '../model/CoreDtos'

@Entry
@Component
struct Index {
  @State workspacePath: string = ''
  @State projects: Project[] = []
  @State recentEdits: RecentEdit[] = []
  @State writingStats: WritingStats | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State nativeStatus: string = ''
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    this.initAndLoad()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
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
          // Fail-fast：native 初始化失败直接报错，不降级到 mock
          const errCode = nativeBridge.getLastInitErrorCode()
          const rustError = await nativeBridge.getLastError()
          let errDesc = `Unknown error(${errCode})`
          if (errCode === -1) { errDesc = 'Path is empty' }
          else if (errCode === -2) { errDesc = 'Invalid path encoding' }
          else if (errCode === -3) { errDesc = 'Internal state error' }
          else if (errCode === -4) { errDesc = 'Workspace dir creation failed' }
          else if (errCode === -999) { errDesc = 'NAPI exception (SO load failed?)' }
          this.errorMessage = `Native core init failed: ${errDesc}\nPath: ${workspacePath}\nDetail: ${rustError}`
          this.isLoading = false
          return
        }
        const statusResult = await nativeBridge.getNativeLoadStatus()
        this.nativeStatus = statusResult.success ? (statusResult.data || 'unknown') : 'error'
      }

      await this.loadWorkspace()
      this.initializeTheme()
    } catch (err) {
      this.errorMessage = `Init error: ${err}`
      this.isLoading = false
    }
  }

  async loadWorkspace() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      // 新 Core API 边界：平台自己提供数据根目录，Core 不再 openWorkspace。
      // appDataRoot 由平台管理，projects/recentEdits 通过 getAppState 查询。
      const workspacePath = getWorkspacePath()
      this.workspacePath = workspacePath
      const appStateResult = await this.bridge.getAppState()
      const statsResult = await this.bridge.getWritingStats()

      if (appStateResult.success && appStateResult.data) {
        this.projects = appStateResult.data.projects
        this.recentEdits = appStateResult.data.recentEdits
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_workspace').id)
      }

      if (statsResult.success && statsResult.data) {
        this.writingStats = statsResult.data
      }
    } catch (err) {
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_workspace_detail').id)
    } finally {
      this.isLoading = false
    }
  }

  async initializeTheme() {
    try {
      const paletteResult = await this.bridge.listPaletteRecords()
      if (paletteResult.success && paletteResult.data) {
        this.themeAdapter.setPaletteRecords(paletteResult.data)
      }
    } catch (_e) { }

    try {
      const builtinResult = await this.bridge.listBuiltinThemes()
      if (builtinResult.success && builtinResult.data) {
        this.themeAdapter.setBuiltinThemes(builtinResult.data)
      }
    } catch (_e) { }

    try {
      const settingsResult = await this.bridge.getLocalSettings()
      if (settingsResult.success && settingsResult.data) {
        const appearanceMode = settingsResult.data.appearanceMode || settingsResult.data.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          settingsResult.data.colorSource || 'built_in',
          settingsResult.data.selectedPaletteId || '',
          settingsResult.data.selectedBuiltinThemeId || ''
        )
      }
    } catch (_e) { }
  }

  build() {
    Column() {
      // Header — 受内容最大宽度约束，居中显示
      Row() {
        Text($r('app.string.app_title'))
          .fontSize(28)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Column() {
           Text(this.workspacePath ? this.extractWorkspaceName(this.workspacePath) : $r('app.string.my_writing_space'))
             .fontSize(15)
             .fontColor(this.themeConfig.colors.textSecondary)

          if (this.writingStats) {
             Text(getContext(this).resourceManager.getStringSync($r('app.string.today_chars_format').id).replace('%s', `${this.writingStats.todayChars}`))
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
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 16 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
            .color(this.themeConfig.colors.primary)
          Text($r('app.string.loading'))
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

          Button($r('app.string.retry'))
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
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  WritingStatsCard() {
    Row() {
      Column() {
        Text($r('app.string.today_writing'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.writingStats?.todayChars || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.primary)

        Text($r('app.string.unit_char'))
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
        Text($r('app.string.today_duration'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${Math.floor((this.writingStats?.todayDuration || 0) / 60)}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.secondary)

        Text($r('app.string.unit_minute'))
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
        Text($r('app.string.total_words'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ bottom: 8 })

        Text(`${this.writingStats?.totalWords || 0}`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.success)

        Text($r('app.string.unit_char'))
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

  @Builder
  RecentEditsSection() {
    Column() {
      Row() {
        Text($r('app.string.recent_edits'))
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Text($r('app.string.view_all'))
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
             Text(`Chapter ${edit.chapterId}`)
               .fontSize(16)
               .fontColor(this.themeConfig.colors.textPrimary)
               .margin({ bottom: 6 })

             Row() {
               Text(`Project ${edit.projectId}`)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(edit.timestamp))
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 14, bottom: 14 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(DesignTokens.radiusMd)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WritingPage',
            params: {
              chapterId: edit.chapterId,
              chapterTitle: `Chapter ${edit.chapterId}`
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
        Text($r('app.string.my_projects'))
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Button($r('app.string.new_project'))
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(async () => {
            const result = await this.bridge.createProject(getContext(this).resourceManager.getStringSync($r('app.string.default_new_project_title').id))
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
            Text(project.title)
              .fontSize(17)
              .fontWeight(FontWeight.Medium)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          // 删除按钮
          Text('×')
            .fontSize(18)
            .fontColor(this.themeConfig.colors.onError)
            .width(28)
            .height(28)
            .textAlign(TextAlign.Center)
            .borderRadius(DesignTokens.radiusLg)
            .backgroundColor(this.themeConfig.colors.error)
            .margin({ right: 12 })
            .onClick(() => {
              this.deleteProject(project.id)
            })

          Image($r('sys.media.ohos_ic_public_arrow_right'))
            .width(18)
            .height(18)
            .fillColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 16, bottom: 16 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(DesignTokens.radiusMd)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WorkspacePage',
            params: {
              projectId: project.id,
              projectName: project.title
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
      Text($r('app.string.quick_actions'))
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

            Text($r('app.string.new_chapter'))
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
        })

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_star'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.secondary)
              .margin({ bottom: 10 })

            Text($r('app.string.starmap'))
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
          router.pushUrl({
            url: 'pages/StarMapPage',
            params: {
              starmapId: 'starmap-001',
               starmapTitle: 'Character Relations'
            }
          })
        })

        GridCol({ span: 1 }) {
          Column() {
            Image($r('sys.media.ohos_ic_public_more'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.textSecondary)
              .margin({ bottom: 10 })

            Text($r('app.string.settings'))
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
          router.pushUrl({ url: 'pages/SettingsPage', params: { projectId: this.projects.length > 0 ? this.projects[0].id : '' } })
        })

        GridCol({ span: 1 }) {
          Column() {
            Image($r('app.media.ic_refresh'))
              .width(36)
              .height(36)
              .fillColor(this.themeConfig.colors.success)
              .margin({ bottom: 10 })

            Text($r('app.string.sync'))
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
          router.pushUrl({ url: 'pages/SettingsPage', params: { projectId: this.projects.length > 0 ? this.projects[0].id : '' } })
        })
      }
      .width('100%')
    }
    .width('100%')
  }

  async deleteProject(projectId: string) {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_project'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: {
        value: $r('app.string.delete'),
        action: () => {
          this.doDeleteProject(projectId)
        }
      }
    })
  }

  async doDeleteProject(projectId: string) {
    try {
      const result = await this.bridge.deleteProject(projectId)
      if (result.success) {
        this.loadWorkspace()
      }
    } catch (err) {
      console.error('删除作品失败:', err)
    }
  }

  formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)

    if (minutes < 1) {
      return getContext(this).resourceManager.getStringSync($r('app.string.just_now').id)
    } else if (minutes < 60) {
      return getContext(this).resourceManager.getStringSync($r('app.string.minutes_ago_format').id).replace('%s', `${minutes}`)
    } else if (hours < 24) {
      return getContext(this).resourceManager.getStringSync($r('app.string.hours_ago_format').id).replace('%s', `${hours}`)
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  extractWorkspaceName(path: string): string {
    if (!path) {
      return getContext(this).resourceManager.getStringSync($r('app.string.my_writing_space').id)
    }
    const segments = path.replace(/\\/g, '/').split('/')
    const last = segments[segments.length - 1]
    return last || getContext(this).resourceManager.getStringSync($r('app.string.my_writing_space').id)
  }
}
