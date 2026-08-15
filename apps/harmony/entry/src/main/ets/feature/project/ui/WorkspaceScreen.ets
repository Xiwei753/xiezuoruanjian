import { Routes, WorkspaceRouteParams } from '../../../app/navigation/Routes'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ProjectTree, VolumeTree, ChapterMeta, Project, ChapterContent, ChapterSaveReceipt } from '../../../corebridge/dto/ProjectDtos'
import { LocalSettings } from '../../../corebridge/dto/SettingsDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ThemeUpdateParams } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { WidthClass, ShellMode } from '../../../corebridge/dto/PlatformDtos'

/** 左侧面板宽度（双栏模式） */
const LEFT_PANEL_WIDTH: number = 320

/** 分割线宽度 */
const DIVIDER_WIDTH: number = 0.5

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

  // ── 右侧写作区状态（双栏模式） ──
  @State selectedChapterId: string = ''
  @State selectedChapterTitle: string = ''
  @State selectedVolumeId: string = ''
  @State content: string = ''
  @State wordCount: number = 0
  @State isEditorLoading: boolean = false
  @State isSaving: boolean = false
  @State editorErrorMessage: string = ''
  @State saveMessage: string = ''
  @State settings: LocalSettings = {
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    themeMode: 'system',
    autoSaveEnabled: true,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    windowWidth: 0,
    windowHeight: 0,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 0,
    editorSmoothCursorDurationMs: 0,
    aiEnabled: false,
    linuxQtSidebarWidth: 0,
    linuxQtEditorWidth: 0
  }
  @State lastSavedContent: string = ''
  @State hasUnsavedChanges: boolean = false
  @State lastSaveFailed: boolean = false

  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null

  aboutToAppear() {
    const paramsArr = this.navPathStack.getParamByName(Routes.Workspace) as WorkspaceRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.projectId = params.projectId
      this.projectName = params.projectName
      this.loadProjectTree()
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

    // 自动保存观察者（双栏模式下使用）
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.saveChapter(),
      this.settings.autoSaveDelayMs
    )
    this.lifecycle.addObserver(this.autoSaveObserver)
  }

  aboutToDisappear() {
    // 双栏模式：保存未保存的变更（saveChapter 成功后会统一上报统计）
    if (this.hasUnsavedChanges && this.adaptiveState.isTwoPane) {
      this.saveChapter()
    } else if (this.adaptiveState.isTwoPane && this.selectedChapterId && this.sessionOldText !== this.content) {
      // 没有未保存变更但有统计差异时，兜底上报一次
      const deviceId = this.settings.statsDeviceId || 'unknown'
      const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
      this.bridge.processWritingEvent(
        deviceId,
        'harmony',
        this.projectId,
        this.selectedVolumeId,
        this.selectedChapterId,
        this.sessionOldText,
        this.content,
        durationSeconds,
        this.sessionId
      )
    }

    // 清理观察者
    if (this.autoSaveObserver) {
      this.lifecycle.removeObserver(this.autoSaveObserver)
    }
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

  // ── 双栏模式：章节编辑 ──

  async loadChapter() {
    if (!this.selectedChapterId) {
      return
    }

    this.isEditorLoading = true
    this.editorErrorMessage = ''

    try {
      const result = await this.bridge.loadChapter(this.selectedChapterId)
      if (result.success && result.data) {
        this.content = result.data.content
        this.lastSavedContent = result.data.content
        this.wordCount = result.data.meta.wordCount
        this.hasUnsavedChanges = false
        // 记录编辑前文本，用于写作统计
        this.sessionOldText = result.data.content
        this.sessionId = `hm-${this.selectedChapterId}-${Date.now()}`
        this.sessionStartTime = Date.now()

      } else {
        this.editorErrorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter').id)
      }
    } catch (err) {
      this.editorErrorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter_detail').id)
    } finally {
      this.isEditorLoading = false
    }
  }

  async loadSettings() {
    try {
      const result = await this.bridge.getLocalSettings()
      if (result.success && result.data) {
        this.settings = result.data
        this.themeAdapter.updateFromSettings({
          theme: result.data.themeMode,
          fontSize: result.data.editorFontSize,
          lineHeight: result.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)

        // 更新自动保存间隔
        if (this.autoSaveObserver) {
          this.lifecycle.removeObserver(this.autoSaveObserver)
          this.autoSaveObserver = new AutoSaveLifecycleObserver(
            (): Promise<void> => this.saveChapter(),
            this.settings.autoSaveDelayMs
          )
          this.lifecycle.addObserver(this.autoSaveObserver)
        }
      }
    } catch (err) {
      // 忽略设置加载错误
    }
  }

  async saveChapter() {
    if (this.isSaving || !this.hasUnsavedChanges || !this.selectedChapterId) {
      return
    }

    this.isSaving = true
    this.saveMessage = ''

    try {
      const result = await this.bridge.saveChapter(this.selectedChapterId, this.content)
      if (result.success && result.data) {
        // ── 保存成功后上报编辑事件 ──
        if (this.sessionOldText !== this.content) {
          const deviceId = this.settings.statsDeviceId || 'unknown'
          const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
          this.bridge.processWritingEvent(
            deviceId,
            'harmony',
            this.projectId,
            this.selectedVolumeId,
            this.selectedChapterId,
            this.sessionOldText,
            this.content,
            durationSeconds,
            this.sessionId
          )
          // 重置 session，开始新的统计区间
          this.sessionOldText = this.content
          this.sessionId = `hm-${this.selectedChapterId}-${Date.now()}`
          this.sessionStartTime = Date.now()
        }

        this.lastSavedContent = this.content
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        this.wordCount = result.data.wordCount
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.lastSaveFailed = true
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_save').id)
    } finally {
      this.isSaving = false
    }
  }

  onContentChange(text: string) {
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // 更新字数统计
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  selectChapter(chapterId: string, chapterTitle: string, volumeId: string) {
    // 如果有未保存的变更，先保存（saveChapter 成功后会统一上报统计）
    if (this.hasUnsavedChanges && this.selectedChapterId) {
      this.saveChapter().then(() => {
        if (this.lastSaveFailed) {
          // 保存失败，不切换章节
          return
        }
        this.doSelectChapter(chapterId, chapterTitle, volumeId)
      })
      return
    }
    this.doSelectChapter(chapterId, chapterTitle, volumeId)
  }

  doSelectChapter(chapterId: string, chapterTitle: string, volumeId: string) {
    this.selectedChapterId = chapterId
    this.selectedChapterTitle = chapterTitle
    this.selectedVolumeId = volumeId
    this.loadChapter()
    this.loadSettings()
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
        // 如果删除的是当前打开的章节，清空右侧
        if (this.selectedChapterId === chapterId) {
          this.selectedChapterId = ''
          this.selectedChapterTitle = ''
          this.selectedVolumeId = ''
          this.content = ''
          this.lastSavedContent = ''
          this.sessionOldText = ''
          this.hasUnsavedChanges = false
          this.wordCount = 0
        }
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete chapter:', err)
    }
  }

  // ========== 构建 UI ==========

  build() {
    Column() {
      if (this.adaptiveState.isTwoPane) {
        // ── 双栏模式：左侧目录 + 右侧编辑区 ──
        this.TwoPaneLayout()
      } else {
        // ── 单栏模式：保持原有行为 ──
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

      // 右侧面板：写作区
      this.RightPanel()
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
              this.VolumeSectionForTwoPane(volumeTree)
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
  VolumeSectionForTwoPane(volumeTree: VolumeTree) {
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
                  .fontColor(this.selectedChapterId === chapter.id
                    ? this.themeConfig.colors.primary
                    : this.themeConfig.colors.textPrimary)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })

                   Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${chapter.wordCount}`))
                  .fontSize(11)
                  .fontColor(this.themeConfig.colors.textHint)
                  .margin({ top: 2 })
              }
              .layoutWeight(1)
              .alignItems(HorizontalAlign.Start)
            }
            .width('100%')
            .padding({ left: 28, right: 12, top: 8, bottom: 8 })
            .backgroundColor(this.selectedChapterId === chapter.id
              ? this.themeConfig.colors.divider
              : 'transparent')
            .onClick(() => this.selectChapter(chapter.id, chapter.title, volumeTree.volume.id))
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
  }

  @Builder
  RightPanel() {
    Column() {
      if (!this.selectedChapterId) {
        // 未选择章节：显示占位
        Column() {
           Text($r('app.string.select_chapter_to_start'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        // 写作区头部
        Row() {
          Text(this.selectedChapterTitle)
            .fontSize(18)
            .fontWeight(FontWeight.Medium)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          if (this.saveMessage) {
            Text(this.saveMessage)
              .fontSize(12)
              .fontColor(this.getSaveStatusColor())
              .margin({ right: 8 })
          }

           Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${this.wordCount}`))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 8 })

          Button($r('app.string.save'))
            .fontSize(14)
            .height(32)
            .backgroundColor(this.hasUnsavedChanges ? this.themeConfig.colors.primary : this.themeConfig.colors.divider)
            .fontColor(this.hasUnsavedChanges ? this.themeConfig.colors.onPrimary : this.themeConfig.colors.textSecondary)
            .enabled(!this.isSaving && this.hasUnsavedChanges)
            .onClick(() => this.saveChapter())
        }
        .width('100%')
        .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 14 })

        // 编辑区
        if (this.isEditorLoading) {
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
          } else if (this.editorErrorMessage) {
            Column() {
              Text(this.editorErrorMessage)
                .fontSize(16)
                .fontColor(this.themeConfig.colors.error)
                .margin({ bottom: 16 })

              Button($r('app.string.retry'))
              .onClick(() => this.loadChapter())
              .backgroundColor(this.themeConfig.colors.primary)
          }
          .width('100%')
          .layoutWeight(1)
          .justifyContent(FlexAlign.Center)
        } else {
          Scroll() {
            Column() {
               TextArea({ text: this.content, placeholder: getContext(this).resourceManager.getStringSync($r('app.string.start_writing_placeholder').id) })
                .width('100%')
                .constraintSize({ minHeight: '100%' })
                .fontSize(this.settings.editorFontSize)
                .lineHeight(this.settings.editorLineSpacingMultiplier * this.settings.editorFontSize)
                .backgroundColor(this.themeConfig.colors.surface)
                .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 20 })
                .fontColor(this.themeConfig.colors.textPrimary)
                .placeholderColor(this.themeConfig.colors.textHint)
                .onChange((value: string): void => { this.onContentChange(value) })
            }
            .width('100%')
            .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
          }
          .layoutWeight(1)
          .scrollBar(BarState.Off)
          .edgeEffect(EdgeEffect.Spring)
        }

        // 底部工具栏
        Row() {
          if (this.settings.autoSaveEnabled) {
             Text($r('app.string.auto_save'))
               .fontSize(12)
               .fontColor(this.themeConfig.colors.textHint)
           }

           Blank()

           if (this.isSaving) {
             Text($r('app.string.saving'))
               .fontSize(12)
               .fontColor(this.themeConfig.colors.textSecondary)
           } else if (this.hasUnsavedChanges) {
             Text($r('app.string.unsaved'))
               .fontSize(12)
               .fontColor(this.themeConfig.colors.secondary)
           } else if (this.lastSaveFailed) {
             Text($r('app.string.save_failed'))
               .fontSize(12)
               .fontColor(this.themeConfig.colors.error)
           } else {
             Text($r('app.string.saved'))
              .fontSize(12)
              .fontColor(this.themeConfig.colors.success)
          }
        }
        .width('100%')
        .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 10, bottom: 10 })
        .backgroundColor(this.themeConfig.colors.background)
      }
    }
    .layoutWeight(1)
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
    .alignItems(HorizontalAlign.Center)
  }

  // ────────────────────────────────────────
  // 单栏布局：保持原有行为
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
                this.VolumeSection(volumeTree)
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
  VolumeSection(volumeTree: VolumeTree) {
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
            .onClick(() => {
              this.navPathStack.pushPath({ name: Routes.Writing, param: { chapterId: chapter.id,
                  chapterTitle: chapter.title,
                  projectId: this.projectId,
                  volumeId: volumeTree.volume.id } })
            })
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
