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

interface ChangedRange {
  start: number
  oldEnd: number
  newEnd: number
  insertedText: string
  deletedText: string
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
  @State isScrolling: boolean = false
  @State showDeleteAnimation: boolean = false
  @State deleteAnimText: string = ''
  @State deleteAnimX: number = 0
  @State deleteAnimY: number = 0

  @State deleteAnimTranslateX: number = 0
  @State deleteAnimTranslateY: number = 0
  @State deleteAnimScale: number = 1.0
  @State deleteAnimOpacity: number = 0.8
  @State caretOffsetApiAvailable: boolean = true
  @State scrollOffsetY: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private cursorBlinkTimer: number = -1
  private insertAnimTimer: number = -1
  private deleteAnimTimer: number = -1
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
    if (this.insertAnimTimer !== -1) {
      clearTimeout(this.insertAnimTimer)
      this.insertAnimTimer = -1
    }
    if (this.deleteAnimTimer !== -1) {
      clearTimeout(this.deleteAnimTimer)
      this.deleteAnimTimer = -1
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
        this.errorMessage = result.userMessage || getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter').id)
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
        this.typingAnimationEnabled = result.data.editorTypingAnimationEnabled ?? false
        this.smoothCursorEnabled = result.data.editorSmoothCursorEnabled ?? false
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
        this.saveMessage = result.userMessage || getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
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

  private findChangedRange(oldText: string, newText: string): ChangedRange {
    const oldLen = oldText.length
    const newLen = newText.length
    const minLen = Math.min(oldLen, newLen)

    // 找公共前缀
    let start = 0
    while (start < minLen && oldText.charAt(start) === newText.charAt(start)) {
      start++
    }

    // 找公共后缀
    let oldEnd = oldLen
    let newEnd = newLen
    while (oldEnd > start && newEnd > start && oldText.charAt(oldEnd - 1) === newText.charAt(newEnd - 1)) {
      oldEnd--
      newEnd--
    }

    const deletedText = oldText.substring(start, oldEnd)
    const insertedText = newText.substring(start, newEnd)

    const result: ChangedRange = { start: start, oldEnd: oldEnd, newEnd: newEnd, insertedText: insertedText, deletedText: deletedText }
    return result
  }

  private getSaveStatusText(): string {
    if (this.lastSaveFailed) return getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
    if (this.isSaving && this.isAutoSaving) return getContext(this).resourceManager.getStringSync($r('app.string.auto_saving').id)
    if (this.isSaving) return getContext(this).resourceManager.getStringSync($r('app.string.saving').id)
    if (this.hasUnsavedChanges) return getContext(this).resourceManager.getStringSync($r('app.string.unsaved').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  onContentChange(text: string) {
    const oldContent = this.content
    const oldLength = oldContent.length
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // 更新字数
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })

    // ── 动画触发 ──
    const newLength = text.length
    const changed = this.findChangedRange(oldContent, text)

    // 打字动画：检测新插入的文本
    if (this.typingAnimationEnabled && changed.insertedText.length > 0 && changed.insertedText.length <= 3 && !this.isScrolling) {
      if (!this.caretOffsetApiAvailable) {
        // API 不可用时优雅降级：不播放动画
      } else {
        // 用 getCaretOffset 获取真实光标坐标（防御性编码：try-catch + null 检查）
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            const insertedText = changed.insertedText
            // 包含换行不播放
            if (!insertedText.includes('\n') && !insertedText.includes('\r')) {
              this.lastInsertedText = insertedText
              // getCaretOffset() 可能返回 TextArea 内容坐标（未扣 scroll），
              // 如果 DevEco 实测发现返回的已是可视区坐标，则删除 `- this.scrollOffsetY` 即可
              this.insertAnimX = caretOffset.x
              this.insertAnimY = caretOffset.y - this.scrollOffsetY
              this.showInsertAnimation = true

              if (this.insertAnimTimer !== -1) {
                clearTimeout(this.insertAnimTimer)
              }
              this.insertAnimTimer = setTimeout(() => {
                this.showInsertAnimation = false
                this.insertAnimTimer = -1
              }, this.settings.editorTypingAnimationDurationMs || 150) as number
            }
          }
        } catch (e) {
          this.caretOffsetApiAvailable = false
        }
      }
    }

    // 删除动画：检测被删除的文本 — 吞回效果
    if (this.typingAnimationEnabled && changed.deletedText.length > 0 && changed.deletedText.length <= 3 && !this.isScrolling) {
      if (!this.caretOffsetApiAvailable) {
        // API 不可用时优雅降级：不播放动画
      } else {
        // 只取一次当前光标坐标（删除后的位置），作为动画终点
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            const deletedText = changed.deletedText
            if (!deletedText.includes('\n') && !deletedText.includes('\r')) {
              this.deleteAnimText = deletedText
              // 终点：当前光标位置
              this.deleteAnimX = caretOffset.x
              this.deleteAnimY = caretOffset.y - this.scrollOffsetY
              // 起点偏移：从光标右侧开始，用稳定退化方案计算
              this.deleteAnimTranslateX = deletedText.length * this.settings.editorFontSize * 0.6
              this.deleteAnimTranslateY = 0
              this.deleteAnimScale = 1.0
              this.deleteAnimOpacity = 0.8
              this.showDeleteAnimation = true

              if (this.deleteAnimTimer !== -1) {
                clearTimeout(this.deleteAnimTimer)
              }
              this.deleteAnimTimer = setTimeout(() => {
                this.showDeleteAnimation = false
                this.deleteAnimTimer = -1
              }, this.settings.editorTypingAnimationDurationMs || 150) as number

              // 吞回动画：从右侧飞回光标位置 + 缩小 + 淡出
              animateTo({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseIn }, () => {
                this.deleteAnimTranslateX = 0
                this.deleteAnimTranslateY = 0
                this.deleteAnimScale = 0.5
                this.deleteAnimOpacity = 0
              })
            }
          }
        } catch (e) {
          this.caretOffsetApiAvailable = false
        }
      }
    }

    // 平滑光标：用 getCaretOffset 获取真实坐标（防御性编码：try-catch + null 检查）
    if (this.smoothCursorEnabled && !this.isScrolling) {
      if (!this.caretOffsetApiAvailable) {
        // API 不可用时优雅降级：不播放动画
      } else {
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            animateTo({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut }, () => {
              this.cursorX = caretOffset.x
              // getCaretOffset() 可能返回 TextArea 内容坐标（未扣 scroll），
              // 如果 DevEco 实测发现返回的已是可视区坐标，则删除 `- this.scrollOffsetY` 即可
              this.cursorY = caretOffset.y - this.scrollOffsetY
            })
            this.cursorVisible = true
          }
        } catch (e) {
          this.caretOffsetApiAvailable = false
        }
      }
    }
  }

  async clearContent() {
    AlertDialog.show({
      title: $r('app.string.confirm_clear') as string,
      message: $r('app.string.confirm_clear_message') as string,
      primaryButton: {
        value: $r('app.string.cancel') as string,
        action: () => {}
      },
      secondaryButton: {
        value: $r('app.string.confirm_clear_action') as string,
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
          Stack() {
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

            // ── 平滑光标 overlay ──
            if (this.smoothCursorEnabled && this.cursorVisible) {
              Column()
                .width(2)
                .height(this.settings.editorFontSize * 1.2)
                .backgroundColor(this.themeConfig.colors.primary)
                .position({ x: this.cursorX, y: this.cursorY })
                .opacity(0.8)
                .animation({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut })
            }

            // ── 打字动画 overlay ──
            if (this.typingAnimationEnabled && this.showInsertAnimation) {
              Text(this.lastInsertedText)
                .fontSize(this.settings.editorFontSize)
                .fontColor(this.themeConfig.colors.textPrimary)
                .opacity(0.6)
                .position({ x: this.insertAnimX, y: this.insertAnimY })
                .scale({ x: 1.1, y: 1.1 })
                .animation({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseOut })
            }

            // ── 删除动画 overlay — 吞回效果：从旧位置飞向新光标位置 ──
            if (this.typingAnimationEnabled && this.showDeleteAnimation) {
              Text(this.deleteAnimText)
                .fontSize(this.settings.editorFontSize)
                .fontColor(this.themeConfig.colors.textPrimary)
                .opacity(this.deleteAnimOpacity)
                .position({ x: this.deleteAnimX, y: this.deleteAnimY })
                .scale({ x: this.deleteAnimScale, y: this.deleteAnimScale })
                .translate({ x: this.deleteAnimTranslateX, y: this.deleteAnimTranslateY })
                .animation({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseIn })
            }
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
