import { Routes, WorkspaceRouteParams, WritingRouteParams, SettingsRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { Project, VolumeTree, ChapterMeta } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ProjectWorkspaceViewModel, VolumeCardState, ChapterRowState } from '../presentation/ProjectWorkspaceViewModel'

/** 左侧面板宽度（双栏模式） */
const LEFT_PANEL_WIDTH: number = 320

/** 分割线宽度 */
const DIVIDER_WIDTH: number = 0.5

// Issue #629 评论14 第1项：WorkspaceScreen 只保留项目/卷/章节浏览和目录操作。
// 章节正文只有 WritingScreen 一个页面级 owner。
// 删除所有 editor session/input/IME/save/wordCount/正文状态和 RightPanel() 内联写作区。
// 章节点击无论窄屏还是宽屏都进入 Routes.Writing，不要因为 isTwoPane 就换另一套编辑事务。
//
// Issue #760：业务逻辑（Core/NAPI 调用、async 加载、CRUD 操作、状态管理）移至 ProjectWorkspaceViewModel。
// WorkspaceScreen 只保留 UI 和事件派发。窄屏卷卡可展开设计：
// 收起状态：左上卷名 / 右上新增章节按钮+更多菜单 / 左下章节数 / 中下展开箭头 / 右下卷总字数
// 展开后：章节行 — 左章节名 / 右侧最后编辑日期 / 最右章节字数
@Component
export struct WorkspaceScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State projectId: string = ''
  @State projectName: string = ''
  @State project: Project | null = null
  @State volumeTrees: VolumeTree[] = []
  @State volumeCardStates: VolumeCardState[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State expandedVolumes: Set<string> = new Set()
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  // 当前更多菜单上下文 — 用于 @Builder 无参数方法访问正确的 volumeId/chapterId
  @State currentMenuVolumeId: string = ''
  @State currentMenuChapterId: string = ''

  private viewModel: ProjectWorkspaceViewModel = new ProjectWorkspaceViewModel()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    hilog.info(0x0000, 'WorkspaceScreen', 'route name=Workspace')
    const paramsArr = this.navPathStack.getParamByName(Routes.Workspace) as WorkspaceRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.projectId = params.projectId
      this.projectName = params.projectName
      this.loadProjectTree()
      // Workspace 顶层：更新接续状态为当前 project（不带 chapter），清掉旧章节导航。
      AppContinuationService.getInstance().updateCurrentState({ projectId: this.projectId } as ContinuationPayload)
    }

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

  async aboutToDisappear() {
    // 清理观察者
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ─── ViewModel 同步 ───
  // 每次 ViewModel 操作后，从 ViewModel 同步状态到 @State 变量以触发 UI 更新。

  private syncFromViewModel() {
    this.project = this.viewModel.getProject()
    this.volumeTrees = this.viewModel.getVolumeTrees()
    this.volumeCardStates = this.viewModel.getVolumeCardStates()
    this.isLoading = this.viewModel.isLoading()
    this.errorMessage = this.viewModel.getErrorMessage()
    this.expandedVolumes = this.viewModel.getExpandedVolumes()
  }

  // ─── 数据加载 ───

  async loadProjectTree() {
    await this.viewModel.loadData(this.projectId)
    this.syncFromViewModel()
  }

  // ─── UI 事件处理（只做事件派发，不写业务逻辑） ───

  handleToggleVolume(volumeId: string) {
    this.viewModel.toggleVolume(volumeId)
    this.expandedVolumes = this.viewModel.getExpandedVolumes()
  }

  async handleCreateVolume() {
    await this.viewModel.createVolume()
    this.syncFromViewModel()
  }

  async handleCreateChapter(volumeId: string) {
    await this.viewModel.createChapter(volumeId)
    this.syncFromViewModel()
  }

  handleDeleteVolume(volumeId: string) {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_volume'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: {
        value: $r('app.string.delete'),
        action: async () => {
          await this.viewModel.deleteVolume(volumeId)
          this.syncFromViewModel()
        }
      }
    })
  }

  handleDeleteChapter(chapterId: string) {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_chapter'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: {
        value: $r('app.string.delete'),
        action: async () => {
          await this.viewModel.deleteChapter(chapterId)
          this.syncFromViewModel()
        }
      }
    })
  }

  async handleRetry() {
    await this.viewModel.retry()
    this.syncFromViewModel()
  }

  // Issue #629 评论14 第1项：章节点击统一进入 Routes.Writing，不再内联编辑。
  // 窄屏和宽屏都走同一条导航路径，屏幕宽度变化只改布局，不换 TextEditSession owner。
  private navigateToWriting(chapterId: string, chapterTitle: string, volumeId: string): void {
    const writingParam: WritingRouteParams = {
      chapterId: chapterId,
      chapterTitle: chapterTitle,
      projectId: this.projectId,
      volumeId: volumeId
    }
    this.navPathStack.pushPath({
      name: Routes.Writing,
      param: writingParam
    })
  }

  // Issue #760：使用 NavigationTransactionCoordinator.getInstance().safePop() 代替直接 navPathStack.pop()
  private handleBack(): void {
    NavigationTransactionCoordinator.getInstance().safePop()
  }

  // 导航到设置页
  private navigateToSettings(): void {
    const settingsParam: SettingsRouteParams = {
      projectId: this.projectId
    }
    this.navPathStack.pushPath({
      name: Routes.Settings,
      param: settingsParam
    })
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

  // ========== 构建 UI ==========

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    // WorkspaceScreen 自带 header，hideTitleBar(true) 隐藏原生标题栏。
    NavDestination() {
      Column() {
        if (this.adaptiveState.isTwoPane) {
          // ── 双栏模式：左侧目录 + 右侧写作入口提示 ──
          // Issue #760：宽屏布局保持不变，只把数据源改为 ViewModel。
          this.TwoPaneLayout()
        } else {
          // ── 单栏模式（窄屏卷卡可展开设计） ──
          this.SinglePaneLayout()
        }
      }
      .width('100%')
      .height('100%')
      .backgroundColor(this.themeConfig.colors.background)
    }
    .hideTitleBar(true)
  }

  // ────────────────────────────────────────
  // 双栏布局（保持原样，数据源改为 ViewModel）
  // ────────────────────────────────────────

  @Builder
  TwoPaneLayout() {
    Row() {
      // 左侧面板：卷/章节树
      this.LeftPanel()

      // 分割线
      Column() {}
        .width(DIVIDER_WIDTH)
        .height('100%')
        .backgroundColor(this.themeConfig.colors.divider)

      // 右侧面板：提示选择章节（不再内联编辑）
      this.RightPanelPlaceholder()
    }
    .width('100%')
    .height('100%')
  }

  @Builder
  LeftPanel() {
    Column() {
      // 标题栏
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => this.handleBack())

        Text(this.projectName)
          .fontSize(18)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Button($r('app.string.new_volume'))
          .fontSize(12)
          .height(28)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(() => this.handleCreateVolume())
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 16, bottom: 12 })

      // 目录内容
      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(36)
            .height(36)
            .color(this.themeConfig.colors.primary)
          Text($r('app.string.loading'))
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 8 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 12 })

          Button($r('app.string.retry'))
            .fontSize(13)
            .height(32)
            .backgroundColor(this.themeConfig.colors.primary)
            .onClick(() => this.handleRetry())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.volumeTrees.length === 0) {
        Column() {
          Text($r('app.string.no_volumes_chapters'))
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ bottom: 8 })
          Text($r('app.string.click_new_volume_to_start'))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            ForEach(this.volumeTrees, (volumeTree: VolumeTree) => {
              this.VolumeSection(volumeTree)
            }, (volumeTree: VolumeTree) => volumeTree.volume.id)
          }
          .width('100%')
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width(LEFT_PANEL_WIDTH)
    .height('100%')
    .backgroundColor(this.themeConfig.colors.surface)
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      // 卷头
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        Text(volumeTree.volume.title)
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(getContext(this).resourceManager.getStringSync($r('app.string.chapter_count_format').id).replace('%s', `${volumeTree.chapters.length}`))
          .fontSize(11)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ right: 8 })

        Button('+')
          .fontSize(12)
          .width(24)
          .height(24)
          .backgroundColor(this.themeConfig.colors.secondary)
          .onClick(() => this.handleCreateChapter(volumeTree.volume.id))
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 10, bottom: 10 })
      .onClick(() => this.handleToggleVolume(volumeTree.volume.id))

      // 章节列表
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Text($r('app.string.no_chapters'))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
            .padding({ left: 28, top: 8, bottom: 8 })
        } else {
          ForEach(volumeTree.chapters, (chapter: ChapterMeta) => {
            Row() {
              Column() {
                Text(chapter.title)
                  .fontSize(14)
                  .fontColor(this.themeConfig.colors.textPrimary)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })

                Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${chapter.wordCount}`))
                  .fontSize(11)
                  .fontColor(this.themeConfig.colors.textHint)
                  .margin({ top: 2 })
              }
              .layoutWeight(1)
              .alignItems(HorizontalAlign.Start)

              Image($r('sys.media.ohos_ic_public_arrow_right'))
                .width(16)
                .height(16)
                .fillColor(this.themeConfig.colors.textHint)
                .margin({ left: 8 })
            }
            .width('100%')
            .padding({ left: 28, right: 12, top: 8, bottom: 8 })
            .onClick(() => this.navigateToWriting(chapter.id, chapter.title, volumeTree.volume.id))
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
  }

  // Issue #629 评论14 第1项：双栏右侧不再内联编辑，只显示提示。
  // 章节点击统一 navigateToWriting()，由 WritingScreen 持有编辑会话。
  @Builder
  RightPanelPlaceholder() {
    Column() {
      Column() {
        Text($r('app.string.select_chapter_to_start'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textHint)
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    }
    .layoutWeight(1)
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
    .alignItems(HorizontalAlign.Center)
  }

  // ────────────────────────────────────────
  // 单栏布局（窄屏卷卡可展开设计 — Issue #760）
  // ────────────────────────────────────────

  @Builder
  SinglePaneLayout() {
    Column() {
      // ── 顶栏：左 返回+作品名 / 右 同步+设置 ──
      this.SinglePaneHeader()

      // ── 内容区 ──
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
            .onClick(() => this.handleRetry())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            if (this.volumeCardStates.length === 0) {
              this.EmptyState()
            } else {
              ForEach(this.volumeCardStates, (cardState: VolumeCardState) => {
                this.VolumeCard(cardState)
              }, (cardState: VolumeCardState) => cardState.volumeId)
            }
          }
          .width('100%')
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
          .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
    .alignItems(HorizontalAlign.Center)
  }

  // ── 顶栏：左 返回+作品名 / 右 同步+设置 ──
  @Builder
  SinglePaneHeader() {
    Row() {
      // 左：返回 + 作品名
      Image($r('sys.media.ohos_ic_public_arrow_left'))
        .width(24)
        .height(24)
        .fillColor(this.themeConfig.colors.textPrimary)
        .onClick(() => this.handleBack())

      Text(this.projectName)
        .fontSize(20)
        .fontWeight(FontWeight.Bold)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ left: 12 })
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      // 右：同步、设置
      // 同步按钮 — 导航到设置页（同步配置在设置页内）
      Button($r('app.string.sync'))
        .fontSize(13)
        .height(28)
        .backgroundColor(this.themeConfig.colors.surfaceContainerHigh)
        .fontColor(this.themeConfig.colors.textSecondary)
        .margin({ right: 8 })
        .onClick(() => this.navigateToSettings())

      // 设置按钮
      Button($r('app.string.settings'))
        .fontSize(13)
        .height(28)
        .backgroundColor(this.themeConfig.colors.surfaceContainerHigh)
        .fontColor(this.themeConfig.colors.textSecondary)
        .onClick(() => this.navigateToSettings())
    }
    .width('100%')
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })
  }

  // ── 空状态 ──
  @Builder
  EmptyState() {
    Column() {
      Image($r('sys.media.ohos_ic_public_add'))
        .width(64)
        .height(64)
        .fillColor(this.themeConfig.colors.textHint)
        .margin({ bottom: 16 })

      Text($r('app.string.no_volumes_chapters'))
        .fontSize(16)
        .fontColor(this.themeConfig.colors.textSecondary)
        .margin({ bottom: 8 })

      Text($r('app.string.click_new_volume_to_start'))
        .fontSize(14)
        .fontColor(this.themeConfig.colors.textHint)
    }
    .width('100%')
    .padding(48)
    .justifyContent(FlexAlign.Center)
  }

  // ── 卷卡（可展开） ──
  // 收起状态：左上卷名 / 右上新增章节按钮+更多菜单 / 左下章节数 / 中下展开箭头 / 右下卷总字数
  // 展开后：同一张卷卡下方显示章节列表
  @Builder
  VolumeCard(cardState: VolumeCardState) {
    Column() {
      // ── 卷卡头部（上半部分） ──
      // 左上：卷名    右上：新增章节按钮 + 更多菜单
      Row() {
        Text(cardState.volumeTitle)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        // 新增章节按钮
        Button('+')
          .fontSize(14)
          .width(28)
          .height(28)
          .backgroundColor(this.themeConfig.colors.secondary)
          .onClick(() => this.handleCreateChapter(cardState.volumeId))

        // 更多菜单（重命名、删除）— 使用 HarmonyOS 原生 Menu 组件
        Image($r('sys.media.ohos_ic_public_more'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ left: 8 })
          .onClick(() => {
            this.currentMenuVolumeId = cardState.volumeId
          })
          .bindMenu(this.VolumeMoreMenu)
      }
      .width('100%')
      .padding({ left: 16, right: 12, top: 14, bottom: 8 })

      // ── 卷卡底部信息行 ──
      // 左下：章节数    中下：展开箭头    右下：卷总字数
      Row() {
        // 左下：章节数
        Text(getContext(this).resourceManager.getStringSync($r('app.string.chapter_count_format').id).replace('%s', `${cardState.chapterCount}`))
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textHint)

        // 中下：展开/收起箭头
        Row() {
          Image(this.expandedVolumes.has(cardState.volumeId)
            ? $r('sys.media.ohos_ic_public_arrow_down')
            : $r('sys.media.ohos_ic_public_arrow_right'))
            .width(14)
            .height(14)
            .fillColor(this.themeConfig.colors.textHint)
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)

        // 右下：卷总字数
        Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${cardState.totalWordCount}`))
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textHint)
      }
      .width('100%')
      .padding({ left: 16, right: 12, bottom: 10 })
      .onClick(() => this.handleToggleVolume(cardState.volumeId))

      // ── 章节列表（展开后显示） ──
      if (this.expandedVolumes.has(cardState.volumeId)) {
        if (cardState.chapters.length === 0) {
          Column() {
            Text($r('app.string.no_chapters'))
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textHint)
              .padding(16)
          }
          .width('100%')
          .backgroundColor(this.themeConfig.colors.surfaceContainerLow)
        } else {
          Column() {
            ForEach(cardState.chapters, (chapterState: ChapterRowState) => {
              this.ChapterRow(chapterState)
            }, (chapterState: ChapterRowState) => chapterState.chapterId)
          }
          .width('100%')
          .backgroundColor(this.themeConfig.colors.surfaceContainerLow)
        }
      }
    }
    .width('100%')
    .margin({ bottom: 12 })
    .borderRadius(DesignTokens.radiusSm)
    .clip(true)
    .backgroundColor(this.themeConfig.colors.surface)
    .shadow({ radius: 2, color: DesignTokens.shadowLight.color, offsetX: 0, offsetY: 1 })
  }

  // ── 章节行 ──
  // 左：章节名    右侧：最后编辑日期    最右：章节字数
  @Builder
  ChapterRow(chapterState: ChapterRowState) {
    Row() {
      // 左：章节名
      Text(chapterState.chapterTitle)
        .fontSize(15)
        .fontColor(this.themeConfig.colors.textPrimary)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      // 右侧：最后编辑日期
      Text(this.formatTime(chapterState.lastEditedTime))
        .fontSize(12)
        .fontColor(this.themeConfig.colors.textHint)
        .margin({ left: 8, right: 8 })

      // 最右：章节字数
      Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${chapterState.wordCount}`))
        .fontSize(12)
        .fontColor(this.themeConfig.colors.textSecondary)

      // 章节更多菜单
      Image($r('sys.media.ohos_ic_public_more'))
        .width(20)
        .height(20)
        .fillColor(this.themeConfig.colors.textHint)
        .margin({ left: 8 })
        .onClick(() => {
          this.currentMenuChapterId = chapterState.chapterId
        })
        .bindMenu(this.ChapterMoreMenu)
    }
    .width('100%')
    .padding({ left: 16, right: 12, top: 12, bottom: 12 })
    .onClick(() => this.navigateToWriting(chapterState.chapterId, chapterState.chapterTitle, chapterState.volumeId))
  }

  // ── 卷更多菜单（重命名、删除） ──
  // 使用 HarmonyOS 原生 Menu 组件，消费 Core 的 Context actions
  // bindMenu 要求 CustomBuilder 无参数，通过 @State currentMenuVolumeId 传递上下文
  @Builder
  VolumeMoreMenu() {
    Menu() {
      MenuItem('重命名')
        .onClick(() => {
          // 重命名功能暂未实现
        })
      MenuItem($r('app.string.delete'))
        .onClick(() => this.handleDeleteVolume(this.currentMenuVolumeId))
    }
  }

  // ── 章节更多菜单（重命名、删除） ──
  // 使用 HarmonyOS 原生 Menu 组件，消费 Core 的 Context actions
  // bindMenu 要求 CustomBuilder 无参数，通过 @State currentMenuChapterId 传递上下文
  @Builder
  ChapterMoreMenu() {
    Menu() {
      MenuItem('重命名')
        .onClick(() => {
          // 重命名功能暂未实现
        })
      MenuItem($r('app.string.delete'))
        .onClick(() => this.handleDeleteChapter(this.currentMenuChapterId))
    }
  }
}
