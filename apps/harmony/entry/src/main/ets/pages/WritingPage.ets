import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { ChapterContent, ChapterSaveReceipt, LocalSettings } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../system/HarmonyLifecycle'
import { getBridge } from '../common/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../common/AdaptiveContext'

interface WritingPageParams {
  chapterId: string
  chapterTitle: string
  projectId: string
  volumeId: string
}

@Entry
@Component
struct WritingPage {
  @State chapterId: string = ''
  @State chapterTitle: string = ''
  @State projectId: string = ''
  @State volumeId: string = ''
  @State content: string = ''
  @State wordCount: number = 0
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State errorMessage: string = ''
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
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    linuxQtSidebarWidth: 0,
    linuxQtEditorWidth: 0
  }
  @State lastSavedContent: string = ''
  @State hasUnsavedChanges: boolean = false
  @State isAutoSaving: boolean = false
  @State lastSaveFailed: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  // ── 编辑器动画状态 ──
  // 鸿蒙端动画暂未实现（需自研渲染层 + Core VisualTransaction 接入）。
  // 当前使用系统 TextArea，不支持 hidden range / glyph rect 动画契约。
  // 设置页开关已禁用，此处不从设置读取动画开关。
  @State typingAnimationEnabled: boolean = false
  @State smoothCursorEnabled: boolean = false
  @State cursorPosition: number = 0
  @State isScrolling: boolean = false
  @State scrollOffsetY: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private cursorBlinkTimer: number = -1
  private textAreaController: TextAreaController = new TextAreaController()

  aboutToAppear() {
    const params = router.getParams() as WritingPageParams
    if (params) {
      this.chapterId = params.chapterId
      this.chapterTitle = params.chapterTitle
      this.projectId = params.projectId || ''
      this.volumeId = params.volumeId || ''
      this.loadChapter()
      this.loadSettings()
    }

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // Setup adaptive layout observer
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    // Setup auto-save lifecycle observer
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.saveChapter(true).then(() => {}),
      this.settings.autoSaveDelayMs
    )
    this.lifecycle.addObserver(this.autoSaveObserver)

    // Setup cursor blink timer for smooth cursor animation
    this.cursorBlinkTimer = setInterval(() => {
      this.cursorVisible = !this.cursorVisible
    }, 530) as number
  }

  aboutToDisappear() {
    // 统一统计入口：saveChapter() 成功路径里已有 processWritingEvent()，
    // 这里只在「没有触发 saveChapter 但确实有统计差异」时兜底上报一次
    if (this.hasUnsavedChanges) {
      this.saveChapter()
    } else if (this.sessionOldText !== this.content && this.chapterId) {
      const deviceId = this.settings.statsDeviceId || 'unknown'
      const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
      this.bridge.processWritingEvent(
        deviceId,
        'harmony',
        this.projectId,
        this.volumeId,
        this.chapterId,
        this.sessionOldText,
        this.content,
        durationSeconds,
        this.sessionId
      )
    }

    // Cleanup
    if (this.cursorBlinkTimer !== -1) {
      clearInterval(this.cursorBlinkTimer)
      this.cursorBlinkTimer = -1
    }
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

  async loadChapter() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.loadChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = result.data.content
        this.lastSavedContent = result.data.content
        this.wordCount = result.data.meta.wordCount
        this.hasUnsavedChanges = false
        // 记录编辑前文本，用于写作统计
        this.sessionOldText = result.data.content
        this.sessionId = `hm-${this.chapterId}-${Date.now()}`
        this.sessionStartTime = Date.now()
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter').id)
      }
    } catch (err) {
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter_detail').id)
    } finally {
      this.isLoading = false
    }
  }

  async loadSettings() {
    try {
      const result = await this.bridge.getLocalSettings()
      if (result.success && result.data) {
        this.settings = result.data
        // 编辑器动画开关 — 鸿蒙端暂未实现，不从设置读取
        this.typingAnimationEnabled = false
        this.smoothCursorEnabled = false
        this.themeAdapter.updateFromSettings({
          theme: result.data.themeMode,
          fontSize: result.data.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: result.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)

        // Update auto-save interval
        if (this.autoSaveObserver) {
          this.lifecycle.removeObserver(this.autoSaveObserver)
          this.autoSaveObserver = new AutoSaveLifecycleObserver(
            (): Promise<void> => this.saveChapter(true).then(() => {}),
            this.settings.autoSaveDelayMs
          )
          this.lifecycle.addObserver(this.autoSaveObserver)
        }
      }
    } catch (err) {
      // Ignore settings load error
    }
  }

  async saveChapter(isAutoSave: boolean = false): Promise<boolean> {
    if (this.isSaving || !this.hasUnsavedChanges) return true

    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''

    try {
      const result = await this.bridge.saveChapter(this.chapterId, this.content)
      if (result.success && result.data) {
        // ── 保存成功后上报编辑事件 ──
        if (this.sessionOldText !== this.content) {
          const deviceId = this.settings.statsDeviceId || 'unknown'
          const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
          this.bridge.processWritingEvent(
            deviceId,
            'harmony',
            this.projectId,
            this.volumeId,
            this.chapterId,
            this.sessionOldText,
            this.content,
            durationSeconds,
            this.sessionId
          )
          // 重置 session，开始新的统计区间
          this.sessionOldText = this.content
          this.sessionId = `hm-${this.chapterId}-${Date.now()}`
          this.sessionStartTime = Date.now()
        }

        this.lastSavedContent = this.content
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        this.wordCount = result.data.wordCount
        this.saveMessage = isAutoSave ? getContext(this).resourceManager.getStringSync($r('app.string.auto_saved').id) : getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
        return true
      } else {
        this.lastSaveFailed = true
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
        return false
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_save').id)
      return false
    } finally {
      this.isSaving = false
      this.isAutoSaving = false
    }
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  onContentChange(text: string) {
    const oldContent = this.content
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })

    // 鸿蒙端动画暂未实现，不触发动画逻辑
  }

  async clearContent() {
    AlertDialog.show({
      title: $r('app.string.confirm_clear'),
      message: $r('app.string.confirm_clear_message'),
      primaryButton: {
        value: $r('app.string.cancel'),
        action: () => {}
      },
      secondaryButton: {
        value: $r('app.string.confirm_clear_action'),
        action: () => {
          this.doClearContent()
        }
      }
    })
  }

  private async doClearContent() {
    this.isSaving = true
    try {
      const result = await this.bridge.clearChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = ''
        this.lastSavedContent = ''
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        this.wordCount = 0
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.cleared').id)
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.lastSaveFailed = true
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
    } finally {
      this.isSaving = false
    }
  }

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(async () => {
            if (this.hasUnsavedChanges) {
              const ok = await this.saveChapter()
              if (ok) {
                router.back()
              }
              // 保存失败不返回，用户看到错误提示
            } else {
              router.back()
            }
          })

        Text(this.chapterTitle)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })
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
            .onClick(() => this.loadChapter())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          TextArea({ text: this.content, placeholder: getContext(this).resourceManager.getStringSync($r('app.string.start_writing_placeholder').id), controller: this.textAreaController })
            .width('100%')
            .constraintSize({ minHeight: '100%' })
            .fontSize(this.settings.editorFontSize)
            .lineHeight(this.settings.editorLineSpacingMultiplier * this.settings.editorFontSize)
            .backgroundColor(this.themeConfig.colors.surface)
            .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 20 })
            .fontColor(this.themeConfig.colors.textPrimary)
            .placeholderColor(this.themeConfig.colors.textHint)
            .onChange((value: string): void => { this.onContentChange(value) })
            .onTextSelectionChange((selectionStart: number, selectionEnd: number) => {
              this.cursorPosition = selectionStart
            })
          }
          .width('100%')
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
        .onScrollFrameBegin((offset: number) => {
          this.isScrolling = true
          return { offsetRemain: offset }
        })
        .onScroll((scrollOffset: number, scrollState: ScrollState) => {
          this.scrollOffsetY = scrollOffset
        })
        .onScrollStop(() => {
          this.isScrolling = false
        })
      }

      // Bottom toolbar
      Row() {
        if (this.settings.autoSaveEnabled) {
          Text($r('app.string.auto_save'))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }

        Blank()

        Text(this.getSaveStatusText())
          .fontSize(12)
          .fontColor(this.getSaveStatusColor())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 10, bottom: 10 })
      .backgroundColor(this.themeConfig.colors.background)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
    .alignItems(HorizontalAlign.Center)
  }
}
