import router from '@ohos.router'
import WriterCoreBridge from '../bridge/WriterCoreBridge'
import { ChapterData, SaveReceipt, LocalSettings } from '../model/CoreDtos'

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
    showWordCount: true
  }
  @State lastSavedContent: string = ''
  @State hasUnsavedChanges: boolean = false

  private bridge: WriterCoreBridge = WriterCoreBridge.getInstance()
  private autoSaveTimer: number = -1

  aboutToAppear() {
    const params = router.getParams() as WritingPageParams
    if (params) {
      this.chapterId = params.chapterId
      this.chapterTitle = params.chapterTitle
      this.loadChapter()
      this.loadSettings()
    }
  }

  aboutToDisappear() {
    this.stopAutoSave()
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
        if (this.settings.autoSave) {
          this.startAutoSave()
        }
      }
    } catch (err) {
      // Ignore settings load error
    }
  }

  startAutoSave() {
    this.stopAutoSave()
    this.autoSaveTimer = setInterval(() => {
      if (this.hasUnsavedChanges && !this.isSaving) {
        this.saveChapter()
      }
    }, this.settings.autoSaveInterval * 1000)
  }

  stopAutoSave() {
    if (this.autoSaveTimer !== -1) {
      clearInterval(this.autoSaveTimer)
      this.autoSaveTimer = -1
    }
  }

  async saveChapter() {
    if (this.isSaving) return

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
    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })
  }

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor('#333333')
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
          .fontColor('#333333')
          .margin({ left: 12 })
          .layoutWeight(1)

        if (this.saveMessage) {
          Text(this.saveMessage)
            .fontSize(12)
            .fontColor(this.saveMessage === '已保存' ? '#2ECC71' : '#E74C3C')
            .margin({ right: 8 })
        }

        if (this.settings.showWordCount) {
          Text(`${this.wordCount} 字`)
            .fontSize(12)
            .fontColor('#999999')
            .margin({ right: 8 })
        }

        Button('保存')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.hasUnsavedChanges ? '#4A90D9' : '#CCCCCC')
          .enabled(!this.isSaving && this.hasUnsavedChanges)
          .onClick(() => this.saveChapter())
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
          Text('加载中...')
            .fontSize(14)
            .fontColor('#999999')
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .fontColor('#E74C3C')
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.loadChapter())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        TextArea({ text: this.content, placeholder: '开始写作...' })
          .layoutWeight(1)
          .fontSize(this.settings.fontSize)
          .fontFamily(this.settings.fontFamily)
          .lineHeight(this.settings.lineHeight)
          .backgroundColor('#FFFFFF')
          .padding(16)
          .onChange((value: string) => this.onContentChange(value))
          .onSubmit(() => {
            if (this.settings.autoIndent) {
              // Auto-indent logic would go here
            }
          })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#F5F5F5')
  }
}
