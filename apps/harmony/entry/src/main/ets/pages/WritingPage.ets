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
  @State isAutoSaving: boolean = false
  @State lastSaveFailed: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  // ── 动画相关状态 ──
  @State cursorPosition: number = 0
  @State cursorX: number = 0
  @State cursorY: number = 0
  @State cursorVisible: boolean = true
  @State typingAnimationEnabled: boolean = false
  @State smoothCursorEnabled: boolean = false
  @State lastInsertedText: string = ''
  @State showInsertAnimation: boolean = false
  @State insertAnimX: number = 0
  @State insertAnimY: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private cursorBlinkTimer: number = -1
  private insertAnimTimer: number = -1

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
      (): Promise<void> => this.saveChapter(true),
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
    if (this.insertAnimTimer !== -1) {
      clearTimeout(this.insertAnimTimer)
      this.insertAnimTimer = -1
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
        this.typingAnimationEnabled = result.data.editorTypingAnimationEnabled ?? false
        this.smoothCursorEnabled = result.data.editorSmoothCursorEnabled ?? false
        this.themeAdapter.updateFromSettings({
          theme: result.data.themeMode,
          fontSize: result.data.editorFontSize,
          lineHeight: result.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)

        // Update auto-save interval
        if (this.autoSaveObserver) {
          this.lifecycle.removeObserver(this.autoSaveObserver)
          this.autoSaveObserver = new AutoSaveLifecycleObserver(
            (): Promise<void> => this.saveChapter(true),
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
        this.saveMessage = isAutoSave ? '已自动保存' : '已保存'
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
        return true
      } else {
        this.lastSaveFailed = true
        this.saveMessage = result.userMessage || '保存失败'
        return false
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = '保存时发生错误'
      return false
    } finally {
      this.isSaving = false
      this.isAutoSaving = false
    }
  }

  private getSaveStatusText(): string {
    if (this.lastSaveFailed) return '保存失败'
    if (this.isSaving && this.isAutoSaving) return '自动保存中'
    if (this.isSaving) return '保存中'
    if (this.hasUnsavedChanges) return '未保存'
    return '已保存'
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  onContentChange(text: string, cursorPos?: number) {
    const oldContent = this.content
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // Update word count
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })

    // ── 动画触发 ──
    if (cursorPos !== undefined) {
      this.cursorPosition = cursorPos
    }

    // 打字动画：检测新插入的文本
    if (this.typingAnimationEnabled && text.length > oldContent.length) {
      const insertedLen = text.length - oldContent.length
      // 估算插入位置（取光标位置前 insertedLen 个字符）
      const insertPos = cursorPos !== undefined ? cursorPos - insertedLen : text.length - insertedLen
      this.lastInsertedText = text.substring(insertPos, insertPos + insertedLen)
      this.showInsertAnimation = true

      // 估算动画位置：基于字符数和行高
      const charWidth = this.settings.editorFontSize * 0.6
      const lineHeight = this.settings.editorLineSpacingMultiplier * this.settings.editorFontSize
      const pagePadding = this.adaptiveState.pagePadding
      const approxLine = Math.floor((insertPos * charWidth) / (360 - pagePadding * 2))
      this.insertAnimX = pagePadding + (insertPos * charWidth) % (360 - pagePadding * 2)
      this.insertAnimY = 20 + approxLine * lineHeight

      // 清除之前的定时器
      if (this.insertAnimTimer !== -1) {
        clearTimeout(this.insertAnimTimer)
      }
      this.insertAnimTimer = setTimeout(() => {
        this.showInsertAnimation = false
        this.insertAnimTimer = -1
      }, 150) as number
    }

    // 平滑光标：更新光标位置估算
    if (this.smoothCursorEnabled && cursorPos !== undefined) {
      const charWidth = this.settings.editorFontSize * 0.6
      const lineHeight = this.settings.editorLineSpacingMultiplier * this.settings.editorFontSize
      const pagePadding = this.adaptiveState.pagePadding
      const contentWidth = 360 - pagePadding * 2
      const approxLine = Math.floor((cursorPos * charWidth) / contentWidth)
      animateTo({ duration: 80, curve: Curve.EaseOut }, () => {
        this.cursorX = pagePadding + (cursorPos * charWidth) % contentWidth
        this.cursorY = 20 + approxLine * lineHeight
      })
      // 输入时光标可见
      this.cursorVisible = true
    }
  }

  async clearContent() {
    AlertDialog.show({
      title: '确认清空',
      message: '确认清空本章内容？此操作会保存为空，但可通过同步/历史恢复。',
      primaryButton: {
        value: '取消',
        action: () => {}
      },
      secondaryButton: {
        value: '确认清空',
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
        this.saveMessage = '已清空'
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.lastSaveFailed = true
        this.saveMessage = '清空失败'
      }
    } catch (err) {
      this.lastSaveFailed = true
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
          Stack() {
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

            // ── 平滑光标 overlay ──
            if (this.smoothCursorEnabled && this.cursorVisible) {
              Column()
                .width(2)
                .height(this.settings.editorFontSize * 1.2)
                .backgroundColor(this.themeConfig.colors.primary)
                .position({ x: this.cursorX, y: this.cursorY })
                .opacity(0.8)
                .animation({ duration: 80, curve: Curve.EaseOut })
            }

            // ── 打字动画 overlay ──
            if (this.typingAnimationEnabled && this.showInsertAnimation) {
              Text(this.lastInsertedText)
                .fontSize(this.settings.editorFontSize)
                .fontColor(this.themeConfig.colors.textPrimary)
                .opacity(0.6)
                .position({ x: this.insertAnimX, y: this.insertAnimY })
                .scale({ x: 1.1, y: 1.1 })
                .animation({ duration: 150, curve: Curve.EaseOut })
            }
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
