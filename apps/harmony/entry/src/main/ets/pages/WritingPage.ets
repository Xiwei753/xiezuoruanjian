import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { ChapterData, SaveReceipt, LocalSettings } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../system/HarmonyLifecycle'
import { getBridge } from '../common/AppContext'

interface WritingPageParams {
  chapterId: string
  chapterTitle: string
}

@Entry
@Component
struct WritingPage {
  @State chapterId: string = ''
  @State chapterTitle: string = ''
  @State content: string = ''
  @State wordCount: number = 0
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State errorMessage: string = ''
  @State saveMessage: string = ''
  @State settings: LocalSettings = {
    fontSize: 16,
    lineHeight: 1.8,
    fontFamily: 'HarmonyOS Sans',
    theme: 'system',
    autoSave: true,
    autoSaveInterval: 30,
    autoIndent: true,
    showWordCount: true,
    showLineNumbers: false,
    wordWrap: true,
    spellCheck: false
  }
  @State lastSavedContent: string = ''
  @State hasUnsavedChanges: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State sessionStartTime: number = Date.now()
  @State sessionCharsAdded: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null

  aboutToAppear() {
    const params = router.getParams() as WritingPageParams
    if (params) {
      this.chapterId = params.chapterId
      this.chapterTitle = params.chapterTitle
      this.loadChapter()
      this.loadSettings()
    }

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // Setup auto-save lifecycle observer
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.saveChapter(),
      this.settings.autoSaveInterval * 1000
    )
    this.lifecycle.addObserver(this.autoSaveObserver)
  }

  aboutToDisappear() {
    // Record writing session
    const durationSeconds = Math.floor((Date.now() - this.sessionStartTime) / 1000)
    if (this.sessionCharsAdded > 0 && durationSeconds > 10) {
      this.bridge.processWritingEvent(this.sessionCharsAdded, durationSeconds)
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
  }

  async loadChapter() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.loadChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = result.data.content
        this.lastSavedContent = result.data.content
        this.wordCount = result.data.wordCount
        this.hasUnsavedChanges = false
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
          theme: result.data.theme,
          fontSize: result.data.fontSize,
          fontFamily: result.data.fontFamily,
          lineHeight: result.data.lineHeight
        } as ThemeUpdateParams)

        // Update auto-save interval
        if (this.autoSaveObserver) {
          this.lifecycle.removeObserver(this.autoSaveObserver)
          this.autoSaveObserver = new AutoSaveLifecycleObserver(
            (): Promise<void> => this.saveChapter(),
            this.settings.autoSaveInterval * 1000
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
    const oldLength = this.content.length
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // Track characters added
    const lengthDiff = text.length - oldLength
    if (lengthDiff > 0) {
      this.sessionCharsAdded += lengthDiff
    }

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

        if (this.settings.showWordCount) {
          Text(`${this.wordCount} 字`)
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 8 })
        }

        Button('保存')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.hasUnsavedChanges ? this.themeConfig.colors.primary : this.themeConfig.colors.divider)
          .fontColor(this.hasUnsavedChanges ? '#FFFFFF' : this.themeConfig.colors.textSecondary)
          .enabled(!this.isSaving && this.hasUnsavedChanges)
          .onClick(() => this.saveChapter())
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 20, bottom: 14 })

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
          TextArea({ text: this.content, placeholder: '开始写作...' })
            .width('100%')
            .constraintSize({ minHeight: '100%' })
            .fontSize(this.settings.fontSize)
            .fontFamily(this.settings.fontFamily)
            .lineHeight(this.settings.lineHeight * this.settings.fontSize)
            .backgroundColor(this.themeConfig.colors.surface)
            .padding({ left: 24, right: 24, top: 20, bottom: 20 })
            .fontColor(this.themeConfig.colors.textPrimary)
            .placeholderColor(this.themeConfig.colors.textHint)
            .onChange((value: string): void => { this.onContentChange(value) })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      }

      // Bottom toolbar
      Row() {
        if (this.settings.autoSave) {
          Text('自动保存')
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }

        Blank()

        Text(`会话: ${this.sessionCharsAdded} 字`)
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ right: 16 })

        Text(this.hasUnsavedChanges ? '未保存' : '已保存')
          .fontSize(12)
          .fontColor(this.hasUnsavedChanges ? this.themeConfig.colors.secondary : this.themeConfig.colors.success)
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 10, bottom: 10 })
      .backgroundColor(this.themeConfig.colors.background)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }
}
