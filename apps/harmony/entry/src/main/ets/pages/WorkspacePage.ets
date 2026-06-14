import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { ProjectTree, VolumeTree, Chapter, Project } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver } from '../system/HarmonyThemeAdapter'
import { getBridge } from '../common/AppContext'

interface WorkspacePageParams {
  projectId: string
  projectName: string
}

@Entry
@Component
struct WorkspacePage {
  @State projectId: string = ''
  @State projectName: string = ''
  @State project: Project | null = null
  @State volumeTrees: VolumeTree[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State expandedVolumes: Set<string> = new Set()
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver = {
    onThemeChanged: (theme: ThemeConfig) => {
      this.themeConfig = theme
    }
  }

  aboutToAppear() {
    const params = router.getParams() as WorkspacePageParams
    if (params) {
      this.projectId = params.projectId
      this.projectName = params.projectName
      this.loadProjectTree()
    }

    this.themeAdapter.addObserver(this.themeObserver)
  }

  aboutToDisappear() {
    this.themeAdapter.removeObserver(this.themeObserver)
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
        this.errorMessage = result.userMessage || '加载项目失败'
      }
    } catch (err) {
      this.errorMessage = '加载项目时发生错误'
    } finally {
      this.isLoading = false
    }
  }

  toggleVolume(volumeId: string) {
    if (this.expandedVolumes.has(volumeId)) {
      this.expandedVolumes.delete(volumeId)
    } else {
      this.expandedVolumes.add(volumeId)
    }
  }

  async createVolume() {
    try {
      const result = await this.bridge.createVolume(this.projectId, '新卷')
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create volume:', err)
    }
  }

  async createChapter(volumeId: string) {
    try {
      const result = await this.bridge.createChapter(volumeId, '新章节')
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create chapter:', err)
    }
  }

  async deleteVolume(volumeId: string) {
    // TODO: Show confirmation dialog
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
    // TODO: Show confirmation dialog
    try {
      const result = await this.bridge.deleteChapter(chapterId)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete chapter:', err)
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
          .onClick(() => router.back())

        Text(this.projectName)
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        // Project stats
        if (this.project) {
          Text(`${this.project.volumeCount} 卷 · ${this.project.chapterCount} 章`)
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 12 })
        }

        Button('+ 新卷')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .onClick(() => this.createVolume())
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

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
          .padding({ left: 16, right: 16, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  EmptyState() {
    Column() {
      Image($r('sys.media.ohos_ic_public_add'))
        .width(64)
        .height(64)
        .fillColor(this.themeConfig.colors.textHint)
        .margin({ bottom: 16 })

      Text('还没有卷和章节')
        .fontSize(16)
        .fontColor(this.themeConfig.colors.textSecondary)
        .margin({ bottom: 8 })

      Text('点击上方"新卷"开始创作')
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

        Text(volumeTree.volume.name)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(`${volumeTree.chapters.length} 章`)
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
      .borderRadius({ topLeft: 8, topRight: 8 })
      .onClick(() => this.toggleVolume(volumeTree.volume.id))

      // Chapters list
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Column() {
            Text('暂无章节')
              .fontSize(14)
              .fontColor(this.themeConfig.colors.textHint)
              .padding(16)
          }
          .width('100%')
          .backgroundColor(this.themeConfig.colors.surface)
        } else {
          ForEach(volumeTree.chapters, (chapter: Chapter) => {
            Row() {
              Column() {
                Text(chapter.name)
                  .fontSize(15)
                  .fontColor(this.themeConfig.colors.textPrimary)
                  .margin({ bottom: 4 })

                Row() {
                  Text(`${chapter.wordCount} 字`)
                    .fontSize(12)
                    .fontColor(this.themeConfig.colors.textSecondary)
                    .margin({ right: 12 })

                  Text(this.formatTime(chapter.updatedAt))
                    .fontSize(12)
                    .fontColor(this.themeConfig.colors.textHint)

                  if (chapter.status) {
                    Text(chapter.status === 'draft' ? '草稿' : chapter.status === 'published' ? '已发布' : '已归档')
                      .fontSize(11)
                      .fontColor(this.themeConfig.colors.primary)
                      .padding({ left: 6, right: 6, top: 2, bottom: 2 })
                      .backgroundColor(this.themeConfig.colors.background)
                      .borderRadius(4)
                      .margin({ left: 8 })
                  }
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
              router.pushUrl({
                url: 'pages/WritingPage',
                params: {
                  chapterId: chapter.id,
                  chapterTitle: chapter.name
                }
              })
            })
          }, (chapter: Chapter) => chapter.id)
        }
      }
    }
    .width('100%')
    .margin({ bottom: 12 })
    .borderRadius(8)
    .clip(true)
    .shadow({ radius: 2, color: '#1A000000', offsetX: 0, offsetY: 1 })
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
