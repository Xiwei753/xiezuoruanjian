import { Routes, WorkspaceRouteParams, WritingRouteParams, SettingsRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { VolumeTree, ChapterMeta } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ProjectWorkspaceViewModel, VolumeCardState, ChapterRowState } from '../presentation/ProjectWorkspaceViewModel'
import { ActionSlot, ActionTarget, ActionRole } from '../../../corebridge/dto/PlatformDtos'

/** 左侧面板宽度（双栏模式） */
const LEFT_PANEL_WIDTH: number = 320

/** 分割线宽度 */
const DIVIDER_WIDTH: number = 0.5

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
  // navPathStack 通过 NavigationTransactionCoordinator 获取，不使用 @Consume（V1 装饰器，V2 中不可用）
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: ProjectWorkspaceViewModel = new ProjectWorkspaceViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    hilog.info(0x0000, 'WorkspaceScreen', 'route name=Workspace')
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (navPathStack) {
      const paramsArr = navPathStack.getParamByName(Routes.Workspace) as WorkspaceRouteParams[]
      const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
      if (params) {
        this.viewModel.projectId = params.projectId
        this.viewModel.projectName = params.projectName
        this.viewModel.loadData(params.projectId)
        // Workspace 顶层：更新接续状态为当前 project（不带 chapter），清掉旧章节导航。
        AppContinuationService.getInstance().updateCurrentState({ projectId: params.projectId } as ContinuationPayload)
      }
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

  aboutToDisappear() {
    // 清理观察者
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ─── UI 事件处理（只做事件派发，不写业务逻辑） ───

  handleToggleVolume(volumeId: string) {
    this.viewModel.toggleVolume(volumeId)
  }

  handleCreateVolume() {
    this.viewModel.createVolume()
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
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const writingParam: WritingRouteParams = {
      chapterId: chapterId,
      chapterTitle: chapterTitle,
      projectId: this.viewModel.projectId,
      volumeId: volumeId
    }
    navPathStack.pushPath({
      name: Routes.Writing,
      param: writingParam
    })
  }

  // Issue #760：使用 NavigationTransactionCoordinator.getInstance().safePop() 代替直接 navPathStack.pop()
  private handleBack(): void {
    this.navCoordinator.safePop()
  }

  // 导航到设置页
  private navigateToSettings(): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    const settingsParam: SettingsRouteParams = {
      projectId: this.viewModel.projectId
    }
    navPathStack.pushPath({
      name: Routes.Settings,
      param: settingsParam
    })
  }

  // 执行同步（通过 ViewModel，而不是跳设置页）
  private handleSync(): void {
    this.viewModel.performSync()
  }

  // ─── Context Action 处理 ───

  /** 处理卷的 Context action（按 action slot 渲染的菜单项点击） */
  private handleVolumeContextAction(slot: ActionSlot): void {
    const volumeId = this.viewModel.currentMenuVolumeId
    if (slot.role === ActionRole.Rename) {
      this.viewModel.startRenameVolume(volumeId)
    } else if (slot.role === ActionRole.Delete) {
      this.viewModel.startDeleteVolume(volumeId, slot.requiresConfirmation)
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
      this.viewModel.startRenameChapter(chapterId)
    } else if (slot.role === ActionRole.Delete) {
      this.viewModel.startDeleteChapter(chapterId, slot.requiresConfirmation)
    } else if (slot.role === ActionRole.MoveEarlier) {
      this.viewModel.moveChapterEarlier(chapterId)
    } else if (slot.role === ActionRole.MoveLater) {
      this.viewModel.moveChapterLater(chapterId)
    }
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

  // ========== 构建 UI ==========

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    // WorkspaceScreen 自带 header，hideTitleBar(true) 隐藏原生标题栏。
    NavDestination() {
      Stack() {
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

        // ── 对话框层 ──
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

        Text(this.viewModel.projectName)
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
      if (this.viewModel.isLoadingState) {
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
      } else if (this.viewModel.errorMessageState) {
        Column() {
          Text(this.viewModel.errorMessageState)
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
      } else if (this.viewModel.volumeTrees.length === 0) {
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
            ForEach(this.viewModel.volumeTrees, (volumeTree: VolumeTree) => {
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
        Image(this.viewModel.isVolumeExpanded(volumeTree.volume.id)
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
      if (this.viewModel.isVolumeExpanded(volumeTree.volume.id)) {
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
      if (this.viewModel.isLoadingState) {
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
      } else if (this.viewModel.errorMessageState) {
        Column() {
          Text(this.viewModel.errorMessageState)
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
            if (this.viewModel.volumeCardStates.length === 0) {
              this.EmptyState()
            } else {
              ForEach(this.viewModel.volumeCardStates, (cardState: VolumeCardState) => {
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

  // ── 顶栏：左 返回+作品名 / 右 按 ScreenPolicy HeaderTrailing 渲染 Sync→Search→Settings ──
  // Issue #760 评论 5823355851 问题3：顶栏搜索入口丢失，改为按 headerTrailingActions 顺序渲染
  @Builder
  SinglePaneHeader() {
    Row() {
      // 左：返回 + 作品名
      Image($r('sys.media.ohos_ic_public_arrow_left'))
        .width(24)
        .height(24)
        .fillColor(this.themeConfig.colors.textPrimary)
        .onClick(() => this.handleBack())

      Text(this.viewModel.projectName)
        .fontSize(20)
        .fontWeight(FontWeight.Bold)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ left: 12 })
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      // 右：按 ScreenPolicy HeaderTrailing 顺序渲染（Sync → Search → Settings）
      ForEach(this.viewModel.headerTrailingActions, (slot: ActionSlot) => {
        if (slot.role === ActionRole.Sync) {
          // 同步按钮 — 执行真正的同步动作（通过 ViewModel）
          Button($r('app.string.sync'))
            .fontSize(13)
            .height(28)
            .backgroundColor(this.viewModel.isSyncing ? this.themeConfig.colors.primary : this.themeConfig.colors.surfaceContainerHigh)
            .fontColor(this.viewModel.isSyncing ? this.themeConfig.colors.onPrimary : this.themeConfig.colors.textSecondary)
            .margin({ right: 8 })
            .enabled(!this.viewModel.isSyncing)
            .onClick(() => this.handleSync())
        } else if (slot.role === ActionRole.Search) {
          // 搜索按钮 — 搜索功能暂未实现，保留入口图标
          Image($r('sys.media.ohos_ic_public_search'))
            .width(24)
            .height(24)
            .fillColor(this.themeConfig.colors.textSecondary)
            .margin({ left: 8, right: 8 })
            .onClick(() => {
              // 搜索功能即将上线，暂保留入口
            })
        } else if (slot.role === ActionRole.Settings) {
          // 设置按钮
          Button($r('app.string.settings'))
            .fontSize(13)
            .height(28)
            .backgroundColor(this.themeConfig.colors.surfaceContainerHigh)
            .fontColor(this.themeConfig.colors.textSecondary)
            .onClick(() => this.navigateToSettings())
        }
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
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

        // 更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
        Image($r('sys.media.ohos_ic_public_more'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ left: 8 })
          .onClick(() => {
            this.viewModel.currentMenuVolumeId = cardState.volumeId
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
          Image(this.viewModel.isVolumeExpanded(cardState.volumeId)
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
      if (this.viewModel.isVolumeExpanded(cardState.volumeId)) {
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

      // 章节更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
      Image($r('sys.media.ohos_ic_public_more'))
        .width(20)
        .height(20)
        .fillColor(this.themeConfig.colors.textHint)
        .margin({ left: 8 })
        .onClick(() => {
          this.viewModel.currentMenuChapterId = chapterState.chapterId
        })
        .bindMenu(this.ChapterMoreMenu)
    }
    .width('100%')
    .padding({ left: 16, right: 12, top: 12, bottom: 12 })
    .onClick(() => this.navigateToWriting(chapterState.chapterId, chapterState.chapterTitle, chapterState.volumeId))
  }

  // ── 卷更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuVolumeId 传递上下文
  @Builder
  VolumeMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Volume), (slot: ActionSlot) => {
        MenuItem(this.getActionLabel(slot.role))
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
        MenuItem(this.getActionLabel(slot.role))
          .onClick(() => this.handleChapterContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ── 重命名对话框 ──
  // 类似 HomeScreen 的 RenameDialog，调用 ViewModel 的 renameVolume/renameChapter
  @Builder
  RenameDialog() {
    Column() {
      Column() {
        Text(this.viewModel.renameTarget?.type === 'volume' ? '重命名卷' : '重命名章节')
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
  // 仅当 ActionSlot.requiresConfirmation === true 时显示
  @Builder
  DeleteConfirmDialog() {
    Column() {
      Column() {
        Text('确认删除')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 8 })

        Text(`确定要删除"${this.viewModel.deleteTarget?.name}"吗？此操作不可撤销。`)
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
