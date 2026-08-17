import { Routes, WorkspaceRouteParams } from '../../../app/navigation/Routes'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ProjectTree, VolumeTree, ChapterMeta, Project } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ThemeUpdateParams } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'

/** 左侧面板宽度（双栏模式） */
const LEFT_PANEL_WIDTH: number = 320

/** 分割线宽度 */
const DIVIDER_WIDTH: number = 0.5

// Issue #629 评论14 第1项：WorkspaceScreen 只保留项目/卷/章节浏览和目录操作。
// 章节正文只有 WritingScreen 一个页面级 owner。
// 删除所有 editor session/input/IME/save/wordCount/正文状态和 RightPanel() 内联写作区。
// 章节点击无论窄屏还是宽屏都进入 Routes.Writing，不要因为 isTwoPane 就换另一套编辑事务。
@Component
export struct WorkspaceScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State projectId: string = ''
  @State projectName: string = ''
  @State project: Project | null = null
  @State volumeTrees: VolumeTree[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State expandedVolumes: Set<string> = new Set()
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
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

  async loadProjectTree() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
        this.project = result.data.project
        this.volumeTrees = result.data.volumes
        // Expand first volume by default
        if (this.volumeTrees.length > 0) {
          this.expandedVolumes.add(this.volumeTrees[0].volume.id)
        }
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_project').id)
      }
    } catch (err) {
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_project_detail').id)
    } finally {
      this.isLoading = false
    }
  }

  // ── 目录操作 ──

  toggleVolume(volumeId: string) {
    if (this.expandedVolumes.has(volumeId)) {
      this.expandedVolumes.delete(volumeId)
    } else {
      this.expandedVolumes.add(volumeId)
    }
  }

  async createVolume() {
    try {
      const result = await this.bridge.createVolume(this.projectId, getContext(this).resourceManager.getStringSync($r('app.string.default_new_volume_title').id))
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create volume:', err)
    }
  }

  async createChapter(volumeId: string) {
    try {
      const result = await this.bridge.createChapter(volumeId, getContext(this).resourceManager.getStringSync($r('app.string.default_new_chapter_title').id))
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create chapter:', err)
    }
  }

  async deleteVolume(volumeId: string) {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_volume'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: { value: $r('app.string.delete'), action: () => this.doDeleteVolume(volumeId) }
    })
  }

  async doDeleteVolume(volumeId: string) {
    try {
      const result = await this.bridge.deleteVolume(this.projectId, volumeId)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete volume:', err)
    }
  }

  async deleteChapter(chapterId: string) {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_chapter'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: { value: $r('app.string.delete'), action: () => this.doDeleteChapter(chapterId) }
    })
  }

  async doDeleteChapter(chapterId: string) {
    try {
      const result = await this.bridge.deleteChapter(chapterId)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete chapter:', err)
    }
  }

  // Issue #629 评论14 第1项：章节点击统一进入 Routes.Writing，不再内联编辑。
  // 窄屏和宽屏都走同一条导航路径，屏幕宽度变化只改布局，不换 TextEditSession owner。
  private navigateToWriting(chapterId: string, chapterTitle: string, volumeId: string): void {
    this.navPathStack.pushPath({
      name: Routes.Writing,
      param: {
        chapterId: chapterId,
        chapterTitle: chapterTitle,
        projectId: this.projectId,
        volumeId: volumeId
      }
    })
  }

  // ========== 构建 UI ==========

  build() {
    Column() {
      if (this.adaptiveState.isTwoPane) {
        // ── 双栏模式：左侧目录 + 右侧写作入口提示 ──
        this.TwoPaneLayout()
      } else {
        // ── 单栏模式 ──
        this.SinglePaneLayout()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  // ────────────────────────────────────────
  // 双栏布局
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
          .onClick(() => this.navPathStack.pop())

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
          .onClick(() => this.createVolume())
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
            .onClick(() => this.loadProjectTree())
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
           .onClick(() => this.createChapter(volumeTree.volume.id))
       }
       .width('100%')
       .padding({ left: 12, right: 12, top: 10, bottom: 10 })
       .onClick(() => this.toggleVolume(volumeTree.volume.id))

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
  // 单栏布局
  // ────────────────────────────────────────

  @Builder
  SinglePaneLayout() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => this.navPathStack.pop())

        Text(this.projectName)
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        // Project stats
        if (this.project) {
           Text(getContext(this).resourceManager.getStringSync($r('app.string.volume_count_format').id).replace('%s', `${this.volumeTrees.length}`))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 12 })
        }

         Button($r('app.string.new_volume'))
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(() => this.createVolume())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })

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
            .onClick(() => this.loadProjectTree())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            if (this.volumeTrees.length === 0) {
              this.EmptyState()
            } else {
              ForEach(this.volumeTrees, (volumeTree: VolumeTree) => {
                this.SinglePaneVolumeSection(volumeTree)
              }, (volumeTree: VolumeTree) => volumeTree.volume.id)
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

  @Builder
  SinglePaneVolumeSection(volumeTree: VolumeTree) {
    Column() {
      // Volume header
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        Text(volumeTree.volume.title)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

         Text(getContext(this).resourceManager.getStringSync($r('app.string.chapter_count_format').id).replace('%s', `${volumeTree.chapters.length}`))
           .fontSize(12)
           .fontColor(this.themeConfig.colors.textHint)
           .margin({ right: 8 })

         Button('+')
           .fontSize(14)
           .width(28)
           .height(28)
           .backgroundColor(this.themeConfig.colors.secondary)
           .onClick(() => this.createChapter(volumeTree.volume.id))

         Button('×')
           .fontSize(14)
           .width(28)
           .height(28)
           .backgroundColor(this.themeConfig.colors.error)
           .margin({ left: 4 })
           .onClick(() => this.deleteVolume(volumeTree.volume.id))
       }
       .width('100%')
       .padding(12)
       .backgroundColor(this.themeConfig.colors.surface)
       .borderRadius({ topLeft: DesignTokens.radiusSm, topRight: DesignTokens.radiusSm })
       .onClick(() => this.toggleVolume(volumeTree.volume.id))

       // Chapters list
       if (this.expandedVolumes.has(volumeTree.volume.id)) {
         if (volumeTree.chapters.length === 0) {
           Column() {
             Text($r('app.string.no_chapters'))
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textHint)
              .padding(16)
          }
          .width('100%')
          .backgroundColor(this.themeConfig.colors.surface)
        } else {
          ForEach(volumeTree.chapters, (chapter: ChapterMeta) => {
            Row() {
              Column() {
                Text(chapter.title)
                  .fontSize(15)
                  .fontColor(this.themeConfig.colors.textPrimary)
                  .margin({ bottom: 4 })

                Row() {
                  Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${chapter.wordCount}`))
                    .fontSize(12)
                    .fontColor(this.themeConfig.colors.textSecondary)
                    .margin({ right: 12 })

                  Text(this.formatTime(chapter.updatedAt))
                    .fontSize(12)
                    .fontColor(this.themeConfig.colors.textHint)
                }
              }
              .layoutWeight(1)
              .alignItems(HorizontalAlign.Start)

              Button('×')
                .fontSize(14)
                .width(24)
                .height(24)
                .backgroundColor('transparent')
                .fontColor(this.themeConfig.colors.textHint)
                .onClick(() => this.deleteChapter(chapter.id))

              Image($r('sys.media.ohos_ic_public_arrow_right'))
                .width(16)
                .height(16)
                .fillColor(this.themeConfig.colors.textHint)
                .margin({ left: 8 })
            }
            .width('100%')
            .padding({ left: 16, right: 16, top: 12, bottom: 12 })
            .backgroundColor(this.themeConfig.colors.surface)
            .onClick(() => this.navigateToWriting(chapter.id, chapter.title, volumeTree.volume.id))
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
    .margin({ bottom: 12 })
    .borderRadius(DesignTokens.radiusSm)
    .clip(true)
    .shadow({ radius: 2, color: DesignTokens.shadowLight.color, offsetX: 0, offsetY: 1 })
  }

  formatTime(isoString: string): string {
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
}
