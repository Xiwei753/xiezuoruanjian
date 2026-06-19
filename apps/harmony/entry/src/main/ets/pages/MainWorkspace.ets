/**
 * MainWorkspace.ets — 双栏自适应主工作区
 *
 * 核心逻辑：
 * - 监听 AdaptiveContext 的 isTwoPane 状态
 * - Compact/Medium（窄屏）：Navigation Stack 模式，点击项目 → router push 到 WorkspacePage，
 *   点击章节 → router push 到 WritingPage（保持现有行为）
 * - Expanded（宽屏）：Navigation Split 模式，左侧显示卷/章节列表，右侧显示写作区
 *
 * 对标 Linux 桌面端 WritingWorkspace.qml（SplitView: 左侧 WorkspaceTree + 右侧 EditorPage）
 */
import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import {
  Project,
  ProjectTree,
  VolumeTree,
  ChapterMeta,

  LocalSettings,
  RecentEdit
} from '../model/CoreDtos'
import {
  HarmonyThemeAdapter,
  ThemeConfig,
  ThemeObserver,
  ThemeObserverImpl,
  ThemeUpdateParams
} from '../system/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../system/HarmonyLifecycle'
import { getBridge } from '../common/AppContext'
import {
  AdaptiveContext,
  AdaptiveState,
  AdaptiveObserver,
  SimpleAdaptiveObserver,

  getAdaptiveContext,
  getAdaptiveState
} from '../common/AdaptiveContext'

// ========== 常量 ==========

/** 左侧面板宽度（Expanded 双栏模式） */
const LEFT_PANEL_WIDTH: number = 320


/** 左右面板分割线宽度 */
const DIVIDER_WIDTH: number = 0.5

// ========== MainWorkspace ==========

@Entry
@Component
struct MainWorkspace {
  // ── 响应式布局状态 ──
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  // ── 项目列表状态（单栏模式使用） ──
  @State projects: Project[] = []
  @State recentEdits: RecentEdit[] = []

  // ── 左侧面板状态（双栏模式使用） ──
  @State selectedProjectId: string = ''
  @State selectedProjectName: string = ''
  @State projectTree: ProjectTree | null = null
  @State volumeTrees: VolumeTree[] = []
  @State expandedVolumes: Set<string> = new Set()
  @State isTreeLoading: boolean = false
  @State treeErrorMessage: string = ''

  // ── 右侧写作区状态 ──
  @State selectedChapterId: string = ''
  @State selectedChapterTitle: string = ''
  @State content: string = ''
  @State wordCount: number = 0
  @State isLoading: boolean = false
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
  @State lastSaveFailed: boolean = false
  @State selectedVolumeId: string = ''
  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0

  // ── 编辑器动画状态 ──
  @State typingAnimationEnabled: boolean = false
  @State smoothCursorEnabled: boolean = false
  @State cursorVisible: boolean = true
  @State lastInsertedText: string = ''
  @State showInsertAnimation: boolean = false
  @State cursorBlinkOpacity: number = 1.0
  @State insertAnimX: number = 0
  @State insertAnimY: number = 0
  @State showDeleteAnimation: boolean = false
  @State deleteAnimText: string = ''
  @State deleteAnimX: number = 0
  @State deleteAnimY: number = 0
  @State isScrolling: boolean = false
  @State cursorX: number = 0
  @State cursorY: number = 0

  // ── getCaretOffset() 防御性编码 ──
  @State caretOffsetApiAvailable: boolean = true

  // ── 删除动画"吞回"效果 ──

  @State deleteAnimTranslateX: number = 0
  @State deleteAnimTranslateY: number = 0
  @State deleteAnimScale: number = 1.0
  @State deleteAnimOpacity: number = 0.8

  // ── 滚动偏移补偿 ──
  // getCaretOffset() 可能返回的是 TextArea 内容坐标（未扣 scroll），
  // 如果 DevEco 实测发现返回的已是可视区坐标，则不需要扣减 scrollOffsetY
  @State scrollOffsetY: number = 0

  // ── 通用状态 ──
  @State isInitialLoading: boolean = true
  @State initErrorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  // ── 私有引用 ──
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private adaptiveContext: AdaptiveContext = getAdaptiveContext()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private insertAnimTimer: number = -1
  private deleteAnimTimer: number = -1
  private textAreaController: TextAreaController = new TextAreaController()

  // ========== 生命周期 ==========

  aboutToAppear() {
    // 主题观察者
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 响应式断点观察者
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    this.adaptiveContext.addObserver(this.adaptiveObserver!)

    // 自动保存观察者
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.saveChapter(),
      this.settings.autoSaveDelayMs
    )
    this.lifecycle.addObserver(this.autoSaveObserver)

    // 加载初始数据
    this.loadInitialData()
  }

  aboutToDisappear() {
    // 记录写作编辑事件（让 Core 统一计算统计）
    if (this.selectedChapterId && this.sessionOldText !== this.content) {
      const deviceId = this.settings.statsDeviceId || 'unknown'
      const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
      this.bridge.processWritingEvent(
        deviceId,
        'harmony',
        this.selectedProjectId,
        this.selectedVolumeId,
        this.selectedChapterId,
        this.sessionOldText,
        this.content,
        durationSeconds,
        this.sessionId
      )
    }

    // 保存未保存的变更
    if (this.hasUnsavedChanges) {
      this.saveChapter()
    }

    // 清理观察者
    if (this.autoSaveObserver) {
      this.lifecycle.removeObserver(this.autoSaveObserver)
    }
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      this.adaptiveContext.removeObserver(this.adaptiveObserver)
    }

    // 清理动画定时器
    if (this.insertAnimTimer !== -1) {
      clearTimeout(this.insertAnimTimer)
      this.insertAnimTimer = -1
    }
    if (this.deleteAnimTimer !== -1) {
      clearTimeout(this.deleteAnimTimer)
      this.deleteAnimTimer = -1
    }
  }

  // ========== 数据加载 ==========

  async loadInitialData() {
    this.isInitialLoading = true
    this.initErrorMessage = ''

    try {
      const workspaceResult = await this.bridge.getWorkspaceState()
      if (workspaceResult.success && workspaceResult.data) {
        this.projects = workspaceResult.data.projects
        this.recentEdits = workspaceResult.data.recentEdits
      } else {
        this.initErrorMessage = workspaceResult.userMessage || '加载工作区失败'
      }
    } catch (err) {
      this.initErrorMessage = '加载工作区时发生错误'
    } finally {
      this.isInitialLoading = false
    }
  }

  async loadProjectTree(projectId: string) {
    if (this.selectedProjectId === projectId && this.projectTree !== null) {
      return // 已加载，不重复请求
    }

    this.isTreeLoading = true
    this.treeErrorMessage = ''
    this.selectedProjectId = projectId

    try {
      const result = await this.bridge.getProjectTree(projectId)
      if (result.success && result.data) {
        this.projectTree = result.data
        this.volumeTrees = result.data.volumes
        // 默认展开第一卷
        if (this.volumeTrees.length > 0 && !this.expandedVolumes.has(this.volumeTrees[0].volume.id)) {
          this.expandedVolumes.add(this.volumeTrees[0].volume.id)
        }
      } else {
        this.treeErrorMessage = result.userMessage || '加载项目失败'
      }
    } catch (err) {
      this.treeErrorMessage = '加载项目时发生错误'
    } finally {
      this.isTreeLoading = false
    }
  }

  async loadChapter() {
    if (!this.selectedChapterId) {
      return
    }

    this.isLoading = true
    this.errorMessage = ''

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

        // 读取编辑器动画开关
        this.typingAnimationEnabled = result.data.editorTypingAnimationEnabled ?? false
        this.smoothCursorEnabled = result.data.editorSmoothCursorEnabled ?? false

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

  // ========== 章节操作 ==========

  async saveChapter(): Promise<boolean> {
    if (this.isSaving || !this.hasUnsavedChanges || !this.selectedChapterId) {
      return true
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
            this.selectedProjectId,
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
        this.saveMessage = '已保存'
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
    }
  }

  onContentChange(text: string) {
    const oldContent = this.content
    const oldLength = oldContent.length
    const newLength = text.length
    const changed = this.findChangedRange(oldContent, text)

    // 插入动画（防御性编码 + 滚动偏移补偿）
    if (this.typingAnimationEnabled && changed.insertedText.length > 0 && changed.insertedText.length <= 3 && !this.isScrolling) {
      if (this.caretOffsetApiAvailable) {
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            const insertedText = changed.insertedText
            if (!insertedText.includes('\n') && !insertedText.includes('\r')) {
              this.lastInsertedText = insertedText
              this.insertAnimX = caretOffset.x
              // 滚动偏移补偿：getCaretOffset() 返回的 y 可能是内容坐标，需扣减 scrollOffsetY
              this.insertAnimY = caretOffset.y - this.scrollOffsetY
              this.showInsertAnimation = true
              if (this.insertAnimTimer !== -1) {
                clearTimeout(this.insertAnimTimer)
              }
              this.insertAnimTimer = setTimeout(() => {
                this.showInsertAnimation = false
                this.lastInsertedText = ''
                this.insertAnimTimer = -1
              }, this.settings.editorTypingAnimationDurationMs || 150) as number
            }
          }
        } catch (err) {
          // getCaretOffset() API 不可用或异常，优雅降级：不播放动画
          this.caretOffsetApiAvailable = false
        }
      }
    }

    // 删除动画（防御性编码 + 吞回效果 + 滚动偏移补偿）
    // 只取一次当前光标坐标（删除后位置），起点用稳定退化方案计算
    if (this.typingAnimationEnabled && changed.deletedText.length > 0 && changed.deletedText.length <= 3 && !this.isScrolling) {
      if (this.caretOffsetApiAvailable) {
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            const deletedText = changed.deletedText
            if (!deletedText.includes('\n') && !deletedText.includes('\r')) {
              this.deleteAnimText = deletedText
              // 终点：当前光标位置
              this.deleteAnimX = caretOffset.x
              this.deleteAnimY = caretOffset.y - this.scrollOffsetY
              // 起点：从光标右侧偏移 deletedText.length * fontSize * 0.6 处飞回
              this.deleteAnimTranslateX = deletedText.length * this.settings.editorFontSize * 0.6
              this.deleteAnimTranslateY = 0
              this.deleteAnimScale = 1.0
              this.deleteAnimOpacity = 0.8
              this.showDeleteAnimation = true
              if (this.deleteAnimTimer !== -1) {
                clearTimeout(this.deleteAnimTimer)
              }
              // 触发吞回动画：translate 从偏移归零（从右侧飞回光标位置）
              animateTo({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseIn }, () => {
                this.deleteAnimTranslateX = 0
                this.deleteAnimTranslateY = 0
                this.deleteAnimScale = 0.5
                this.deleteAnimOpacity = 0
              })
              this.deleteAnimTimer = setTimeout(() => {
                this.showDeleteAnimation = false
                this.deleteAnimTimer = -1
              }, this.settings.editorTypingAnimationDurationMs || 150) as number
            }
          }
        } catch (err) {
          this.caretOffsetApiAvailable = false
        }
      }
    }

    // 平滑光标（防御性编码 + 滚动偏移补偿）
    if (this.smoothCursorEnabled && !this.isScrolling) {
      if (this.caretOffsetApiAvailable) {
        try {
          const caretOffset = this.textAreaController.getCaretOffset()
          if (caretOffset && caretOffset.x >= 0 && caretOffset.y >= 0) {
            animateTo({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut }, () => {
              this.cursorX = caretOffset.x
              // 滚动偏移补偿
              this.cursorY = caretOffset.y - this.scrollOffsetY
            })
            this.cursorVisible = true
          }
        } catch (err) {
          // getCaretOffset() API 不可用或异常，优雅降级：不播放动画
          this.caretOffsetApiAvailable = false
        }
      }
    }

    this.content = text
    this.hasUnsavedChanges = text !== this.lastSavedContent

    // 更新字数统计
    this.bridge.calculateWordCount(text).then(count => {
      this.wordCount = count
    })
  }

  // ========== 卷/章节操作 ==========

  toggleVolume(volumeId: string) {
    if (this.expandedVolumes.has(volumeId)) {
      this.expandedVolumes.delete(volumeId)
    } else {
      this.expandedVolumes.add(volumeId)
    }
  }

  async createVolume() {
    try {
      const result = await this.bridge.createVolume(this.selectedProjectId, '新卷')
      if (result.success) {
        this.loadProjectTree(this.selectedProjectId)
      }
    } catch (err) {
      console.error('创建卷失败:', err)
    }
  }

  async createChapter(volumeId: string) {
    try {
      const result = await this.bridge.createChapter(volumeId, '新章节')
      if (result.success) {
        this.loadProjectTree(this.selectedProjectId)
      }
    } catch (err) {
      console.error('创建章节失败:', err)
    }
  }

  async deleteVolume(volumeId: string) {
    AlertDialog.show({
      title: '确认删除',
      message: '确认删除此卷及其所有章节？此操作不可撤销。',
      primaryButton: { value: '取消', action: () => {} },
      secondaryButton: { value: '删除', action: () => this.doDeleteVolume(volumeId) }
    })
  }

  async doDeleteVolume(volumeId: string) {
    try {
      const result = await this.bridge.deleteVolume(this.selectedProjectId, volumeId)
      if (result.success) {
        this.loadProjectTree(this.selectedProjectId)
      }
    } catch (err) {
      console.error('删除卷失败:', err)
    }
  }

  async deleteChapter(chapterId: string) {
    AlertDialog.show({
      title: '确认删除',
      message: '确认删除此章节？此操作不可撤销。',
      primaryButton: { value: '取消', action: () => {} },
      secondaryButton: { value: '删除', action: () => this.doDeleteChapter(chapterId) }
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
          this.sessionId = ''
          this.hasUnsavedChanges = false
          this.wordCount = 0
        }
        this.loadProjectTree(this.selectedProjectId)
      }
    } catch (err) {
      console.error('删除章节失败:', err)
    }
  }

  async deleteProject(projectId: string) {
    AlertDialog.show({
      title: '确认删除',
      message: '确认删除此作品及其所有内容？此操作不可撤销。',
      primaryButton: { value: '取消', action: () => {} },
      secondaryButton: { value: '删除', action: () => this.doDeleteProject(projectId) }
    })
  }

  async doDeleteProject(projectId: string) {
    try {
      const result = await this.bridge.deleteProject(projectId)
      if (result.success) {
        // 如果删除的是当前选中的项目，清空右侧
        if (this.selectedProjectId === projectId) {
          this.selectedProjectId = ''
          this.selectedProjectName = ''
          this.projectTree = null
          this.volumeTrees = []
        }
        this.loadInitialData()
      }
    } catch (err) {
      console.error('删除作品失败:', err)
    }
  }

  // ========== 选中章节 ==========

  selectChapter(chapterId: string, chapterName: string, volumeId: string) {
    // 如果有未保存的变更，先上报并保存
    if (this.hasUnsavedChanges && this.selectedChapterId) {
      const deviceId = this.settings.statsDeviceId || 'unknown'
      const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
      this.bridge.processWritingEvent(
        deviceId,
        'harmony',
        this.selectedProjectId,
        this.selectedVolumeId,
        this.selectedChapterId,
        this.sessionOldText,
        this.content,
        durationSeconds,
        this.sessionId
      )
      this.saveChapter()
    }

    this.selectedChapterId = chapterId
    this.selectedChapterTitle = chapterName
    this.selectedVolumeId = volumeId
    this.sessionStartTime = Date.now()
    this.loadChapter()
    this.loadSettings()
  }

  // ========== 构建 UI ==========

  build() {
    Column() {
      if (this.adaptiveState.isTwoPane) {
        // ── Expanded 双栏模式 ──
        this.TwoPaneLayout()
      } else {
        // ── Compact/Medium 单栏模式 ──
        this.SinglePaneLayout()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  // ────────────────────────────────────────
  // 双栏布局：左侧项目树 + 右侧写作区
  // ────────────────────────────────────────

  @Builder
  TwoPaneLayout() {
    Row() {
      // 左侧面板：项目树 / 卷/章节列表
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
      // 左侧面板标题栏
      Row() {
        Text(this.selectedProjectName || '选择作品')
          .fontSize(18)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (this.selectedProjectId) {
          Button('+ 新卷')
            .fontSize(12)
            .height(28)
            .backgroundColor(this.themeConfig.colors.primary)
            .onClick(() => this.createVolume())
        }
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

      // 面板内容
      if (!this.selectedProjectId) {
        // 未选择项目：显示项目列表
        this.ProjectListForLeftPanel()
      } else if (this.isTreeLoading) {
        Column() {
          LoadingProgress()
            .width(36)
            .height(36)
            .color(this.themeConfig.colors.primary)
          Text('加载中...')
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 8 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.treeErrorMessage) {
        Column() {
          Text(this.treeErrorMessage)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 12 })

          Button('重试')
            .fontSize(13)
            .height(32)
            .backgroundColor(this.themeConfig.colors.primary)
            .onClick(() => this.loadProjectTree(this.selectedProjectId))
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        // 显示卷/章节树
        this.VolumeChapterTree()
      }
    }
    .width(LEFT_PANEL_WIDTH)
    .height('100%')
    .backgroundColor(this.themeConfig.colors.surface)
  }

  @Builder
  ProjectListForLeftPanel() {
    if (this.isInitialLoading) {
      Column() {
        LoadingProgress()
          .width(36)
          .height(36)
          .color(this.themeConfig.colors.primary)
        Text('加载中...')
          .fontSize(13)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ top: 8 })
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    } else if (this.initErrorMessage) {
      Column() {
        Text(this.initErrorMessage)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.error)
          .margin({ bottom: 12 })

        Button('重试')
          .fontSize(13)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(() => this.loadInitialData())
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    } else {
      Scroll() {
        Column() {
          if (this.projects.length === 0) {
            Column() {
              Text('还没有作品')
                .fontSize(15)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ bottom: 8 })

              Text('点击下方按钮创建')
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textHint)
            }
            .width('100%')
            .padding(32)
            .justifyContent(FlexAlign.Center)
          } else {
            ForEach(this.projects, (project: Project) => {
              Row() {
                Column() {
                  Text(project.title)
                    .fontSize(15)
                    .fontColor(this.themeConfig.colors.textPrimary)
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                }
                .layoutWeight(1)
                .alignItems(HorizontalAlign.Start)

                Button('×')
                  .fontSize(12)
                  .width(24)
                  .height(24)
                  .padding(0)
                  .backgroundColor('transparent')
                  .fontColor(this.themeConfig.colors.textHint)
                  .margin({ right: 4 })
                  .onClick(() => this.deleteProject(project.id))

                Image($r('sys.media.ohos_ic_public_arrow_right'))
                  .width(14)
                  .height(14)
                  .fillColor(this.themeConfig.colors.textHint)
              }
              .width('100%')
              .padding({ left: 16, right: 16, top: 12, bottom: 12 })
              .borderRadius(8)
              .backgroundColor(this.selectedProjectId === project.id
                ? this.themeConfig.colors.background
                : 'transparent')
              .onClick(() => {
                this.selectedProjectName = project.title
                this.loadProjectTree(project.id)
              })
            }, (project: Project) => project.id)
          }

          // 新建作品按钮
          Button('+ 新建作品')
            .fontSize(14)
            .height(36)
            .width('100%')
            .backgroundColor(this.themeConfig.colors.primary)
            .margin({ top: 12 })
            .onClick(async () => {
              const result = await this.bridge.createProject('新作品')
              if (result.success) {
                this.loadInitialData()
              }
            })
        }
        .width('100%')
        .padding({ left: 12, right: 12, top: 8, bottom: 16 })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
    }
  }

  @Builder
  VolumeChapterTree() {
    Scroll() {
      Column() {
        // 返回项目列表
        Row() {
          Image($r('sys.media.ohos_ic_public_arrow_left'))
            .width(16)
            .height(16)
            .fillColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 8 })

          Text('返回作品列表')
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
        }
        .width('100%')
        .padding({ left: 16, top: 8, bottom: 8 })
        .onClick(() => {
          // 保存当前章节
          if (this.hasUnsavedChanges) {
            this.saveChapter()
          }
          this.selectedProjectId = ''
          this.selectedProjectName = ''
          this.projectTree = null
          this.volumeTrees = []
        })

        if (this.volumeTrees.length === 0) {
          Column() {
            Text('还没有卷和章节')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textSecondary)
              .margin({ bottom: 8 })

            Text('点击上方"新卷"开始创作')
              .fontSize(13)
              .fontColor(this.themeConfig.colors.textHint)
          }
          .width('100%')
          .padding(32)
          .justifyContent(FlexAlign.Center)
        } else {
          ForEach(this.volumeTrees, (volumeTree: VolumeTree) => {
            this.VolumeSection(volumeTree)
          }, (volumeTree: VolumeTree) => volumeTree.volume.id)
        }
      }
      .width('100%')
      .padding({ left: 8, right: 8, bottom: 16 })
    }
    .layoutWeight(1)
    .scrollBar(BarState.Off)
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      // 卷标题行
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(14)
          .height(14)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 6 })

        Text(volumeTree.volume.title)
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Text(`${volumeTree.chapters.length} 章`)
          .fontSize(11)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ right: 6 })

        Button('+')
          .fontSize(12)
          .width(24)
          .height(24)
          .padding(0)
          .backgroundColor(this.themeConfig.colors.secondary)
          .onClick(() => this.createChapter(volumeTree.volume.id))

        Button('×')
          .fontSize(12)
          .width(24)
          .height(24)
          .padding(0)
          .backgroundColor(this.themeConfig.colors.error)
          .margin({ left: 4 })
          .onClick(() => this.deleteVolume(volumeTree.volume.id))
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 10, bottom: 10 })
      .borderRadius({ topLeft: 6, topRight: 6 })
      .backgroundColor(this.themeConfig.colors.background)
      .onClick(() => this.toggleVolume(volumeTree.volume.id))

      // 章节列表
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Text('暂无章节')
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
            .padding({ left: 32, top: 8, bottom: 8 })
            .width('100%')
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

                Row() {
                  Text(`${chapter.wordCount} 字`)
                    .fontSize(11)
                    .fontColor(this.themeConfig.colors.textSecondary)
                    .margin({ right: 8 })

                  Text(this.formatTime(chapter.updatedAt))
                    .fontSize(11)
                    .fontColor(this.themeConfig.colors.textHint)
                }
                .margin({ top: 2 })
              }
              .layoutWeight(1)
              .alignItems(HorizontalAlign.Start)

              Button('×')
                .fontSize(11)
                .width(20)
                .height(20)
                .padding(0)
                .backgroundColor('transparent')
                .fontColor(this.themeConfig.colors.textHint)
                .onClick(() => this.deleteChapter(chapter.id))
            }
            .width('100%')
            .padding({ left: 28, right: 12, top: 8, bottom: 8 })
            .backgroundColor(this.selectedChapterId === chapter.id
              ? this.themeConfig.colors.background
              : 'transparent')
            .borderRadius(4)
            .onClick(() => this.selectChapter(chapter.id, chapter.title, volumeTree.volume.id))
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
    .margin({ bottom: 8 })
    .borderRadius(6)
    .clip(true)
  }

  // ────────────────────────────────────────
  // 右侧面板：写作区
  // ────────────────────────────────────────

  @Builder
  RightPanel() {
    Column() {
      if (!this.selectedChapterId) {
        // 未选择章节：空状态
        this.RightPanelEmptyState()
      } else {
        // 写作区
        this.EditorArea()
      }
    }
    .layoutWeight(1)
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  RightPanelEmptyState() {
    Column() {
      if (this.selectedProjectId) {
        Text('从左侧选择章节开始写作')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textHint)
      } else {
        Text('从左侧选择作品')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textHint)

        Text('然后选择章节开始写作')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ top: 8 })
      }
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  EditorArea() {
    Column() {
      // 编辑器标题栏
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
      .padding({ left: 24, right: 24, top: 16, bottom: 12 })

      // 编辑器内容区
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
        // 写作区：内容居中，最大宽度约束
        Scroll() {
          Column() {
            Stack() {
              TextArea({ text: this.content, placeholder: '开始写作...', controller: this.textAreaController })
                .width('100%')
                .constraintSize({ minHeight: '100%' })
                .fontSize(this.settings.editorFontSize)
                .lineHeight(this.settings.editorLineSpacingMultiplier * this.settings.editorFontSize)
                .backgroundColor(this.themeConfig.colors.surface)
                .padding({ left: 24, right: 24, top: 20, bottom: 20 })
                .fontColor(this.themeConfig.colors.textPrimary)
                .placeholderColor(this.themeConfig.colors.textHint)
                .onChange((value: string): void => { this.onContentChange(value) })

              // 打字动画 overlay：新字符淡入效果（真实光标坐标定位）
              if (this.typingAnimationEnabled && this.showInsertAnimation) {
                Text(this.lastInsertedText)
                  .fontSize(this.settings.editorFontSize)
                  .fontColor(this.themeConfig.colors.primary)
                  .opacity(0.6)
                  .position({ x: this.insertAnimX, y: this.insertAnimY })
                  .scale({ x: 1.1, y: 1.1 })
                  .animation({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseOut })
              }

              // 删除动画 overlay（吞回效果：从旧光标位置飞向新光标位置）
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

              // 自定义光标 overlay（闪烁竖线，真实光标坐标定位）
              if (this.smoothCursorEnabled) {
                Column()
                  .width(2)
                  .height(this.settings.editorFontSize * 1.2)
                  .backgroundColor(this.themeConfig.colors.primary)
                  .opacity(this.cursorBlinkOpacity)
                  .position({ x: this.cursorX, y: this.cursorY })
                  .animation({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut })
              }
            }
            .width('100%')
          }
          .width('100%')
          .constraintSize({ maxWidth: this.adaptiveState.layoutPlan?.contentMaxWidthVp || 840 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
        .align(Alignment.Top)
        .onScrollFrameBegin((offset: number) => {
          this.isScrolling = true
          return { offsetRemain: offset }
        })
        .onScroll((scrollOffset: number, scrollState: ScrollState) => {
          // 记录滚动偏移，用于动画坐标补偿
          this.scrollOffsetY = scrollOffset
        })
        .onScrollStop(() => {
          this.isScrolling = false
        })
      }

      // 底部工具栏
      Row() {
        if (this.settings.autoSaveEnabled) {
          Text('自动保存')
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }

        Blank()


        Text(this.hasUnsavedChanges ? '未保存' : '已保存')
          .fontSize(12)
          .fontColor(this.getSaveStatusColor())
      }
      .width('100%')
      .padding({ left: 24, right: 24, top: 10, bottom: 10 })
      .constraintSize({ maxWidth: this.adaptiveState.layoutPlan?.contentMaxWidthVp || 840 })
      .backgroundColor(this.themeConfig.colors.background)
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Center)
  }

  // ────────────────────────────────────────
  // 单栏布局：项目列表 + router 跳转
  // ────────────────────────────────────────

  @Builder
  SinglePaneLayout() {
    Column() {
      // 标题栏
      Row() {
        Text('写作软件')
          .fontSize(28)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        if (this.recentEdits.length > 0) {
          Text(`${this.recentEdits.length} 个最近编辑`)
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
        }
      }
      .width('100%')
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 16 })

      if (this.isInitialLoading) {
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
      } else if (this.initErrorMessage) {
        Column() {
          Text(this.initErrorMessage)
            .fontSize(16)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.loadInitialData())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            // 最近编辑
            if (this.recentEdits.length > 0) {
              this.RecentEditsSection()
            }

            // 作品列表
            this.ProjectsSection()
          }
          .width('100%')
          .padding({
            left: this.adaptiveState.pagePadding,
            right: this.adaptiveState.pagePadding,
            bottom: 32
          })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
  }

  @Builder
  RecentEditsSection() {
    Column() {
      Text('最近编辑')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 12 })
        .width('100%')

      ForEach(this.recentEdits, (edit: RecentEdit) => {
        Row() {
          Column() {
            Text(`章节 ${edit.chapterId}`)
              .fontSize(16)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 4 })

            Row() {
              Text(`作品 ${edit.projectId}`)
                .fontSize(13)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ right: 16 })
            }
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(edit.timestamp))
            .fontSize(13)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 14, bottom: 14 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(12)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WritingPage',
            params: {
              chapterId: edit.chapterId,
              chapterTitle: `章节 ${edit.chapterId}`,
              projectId: edit.projectId,
              volumeId: edit.volumeId
            }
          })
        })
      }, (edit: RecentEdit) => edit.chapterId)
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  @Builder
  ProjectsSection() {
    Column() {
      Row() {
        Text('我的作品')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Button('+ 新建作品')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(async () => {
            const result = await this.bridge.createProject('新作品')
            if (result.success) {
              this.loadInitialData()
            }
          })
      }
      .width('100%')
      .margin({ bottom: 12 })

      ForEach(this.projects, (project: Project) => {
        Row() {
          Column() {
            Text(project.title)
              .fontSize(17)
              .fontWeight(FontWeight.Medium)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 6 })
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Button('×')
            .fontSize(14)
            .width(28)
            .height(28)
            .padding(0)
            .backgroundColor('transparent')
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ right: 8 })
            .onClick(() => this.deleteProject(project.id))

          Image($r('sys.media.ohos_ic_public_arrow_right'))
            .width(18)
            .height(18)
            .fillColor(this.themeConfig.colors.textHint)
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 16, bottom: 16 })
        .backgroundColor(this.themeConfig.colors.surface)
        .borderRadius(12)
        .margin({ bottom: 10 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WorkspacePage',
            params: {
              projectId: project.id,
              projectName: project.title
            }
          })
        })
      }, (project: Project) => project.id)
    }
    .width('100%')
  }

  // ========== 工具方法 ==========

  private findChangedRange(oldText: string, newText: string): { start: number; oldEnd: number; newEnd: number; insertedText: string; deletedText: string } {
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

    return { start, oldEnd, newEnd, insertedText, deletedText }
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes} 分钟前`
    } else if (hours < 24) {
      return `${hours} 小时前`
    } else if (days < 7) {
      return `${days} 天前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }
}