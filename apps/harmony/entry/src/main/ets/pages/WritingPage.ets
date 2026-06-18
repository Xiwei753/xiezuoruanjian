import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { ChapterContent, ChapterSaveReceipt, LocalSettings } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../system/HarmonyLifecycle'
import { getBridge } from '../common/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../common/AdaptiveContext'
import { WidthClass, ShellMode } from '../model/CoreDtos'

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
    editorTypingAnimationDurationMs: 0,
    editorSmoothCursorDurationMs: 0,
    aiEnabled: false,
    desktopSidebarWidth: 0,
    desktopEditorWidth: 0
  }
  @State lastSavedContent: string = ''
  @State hasUnsavedChanges: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

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
      (): Promise<void> => this.saveChapter(),
      this.settings.autoSaveDelayMs
    )
    this.lifecycle.addObserver(this.autoSaveObserver)
  }

  aboutToDisappear() {
    // 上报写作编辑事件（让 Core 统一计算统计）
    if (this.sessionOldText !== this.content && this.chapterId) {
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

    // Save if unsaved changes
    if (this.hasUnsavedChanges) {
      this.saveChapter()
    }

    // Cleanup
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
        this.errorMessage = result.userMessage || '加载章节失败'
      }
    } catch (err) {
      this.errorMessage = '加载章节时发生错误'
    } finally {
      this.isLoading = false
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

        // Update auto-save interval
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
      // Ignore settings load error
    }
  }

  async saveChapter() {
    if (this.isSaving || !this.hasUnsavedChanges) return

    this.isSaving = true
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
        this.wordCount = result.data.wordCount
        this.saveMessage = '已保存'
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.saveMessage = result.userMessage || '保存失败'
      }
    } catch (err) {
      this.saveMessage = '保存时发生错误'
    } finally {
      this.isSaving = false
    }
  }

  onContentChange(text: string) {
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // Update word count
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })
  }

  async clearContent() {
    // TODO: Show confirmation dialog
    this.isSaving = true
    try {
      const result = await this.bridge.clearChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = ''
        this.lastSavedContent = ''
        this.hasUnsavedChanges = false
        this.wordCount = 0
        this.saveMessage = '已清空'
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      }
    } catch (err) {
      this.saveMessage = '清空失败'
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
          .onClick(() => {
            if (this.hasUnsavedChanges) {
              this.saveChapter().then(() => router.back())
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
            .fontColor(this.saveMessage === '已保存' ? this.themeConfig.colors.success : this.themeConfig.colors.error)
            .margin({ right: 8 })
        }

        Text(`${this.wordCount} 字`)
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        Button('保存')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.hasUnsavedChanges ? this.themeConfig.colors.primary : this.themeConfig.colors.divider)
          .fontColor(this.hasUnsavedChanges ? '#FFFFFF' : this.themeConfig.colors.textSecondary)
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
            .onClick(() => this.loadChapter())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            TextArea({ text: this.content, placeholder: '开始写作...' })
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

      // Bottom toolbar
      Row() {
        if (this.settings.autoSaveEnabled) {
          Text('自动保存')
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }

        Blank()

        Text(this.hasUnsavedChanges ? '未保存' : '已保存')
          .fontSize(12)
          .fontColor(this.hasUnsavedChanges ? this.themeConfig.colors.secondary : this.themeConfig.colors.success)
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
