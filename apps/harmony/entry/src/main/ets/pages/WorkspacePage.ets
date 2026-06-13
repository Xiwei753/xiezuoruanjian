import router from '@ohos.router'
import WriterCoreBridge from '../bridge/WriterCoreBridge'
import { ProjectTree, VolumeTree, Chapter } from '../model/CoreDtos'

interface WorkspacePageParams {
  projectId: string
  projectName: string
}

@Entry
@Component
struct WorkspacePage {
  @State projectId: string = ''
  @State projectName: string = ''
  @State volumeTrees: VolumeTree[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State expandedVolumes: Set<string> = new Set()

  private bridge: WriterCoreBridge = WriterCoreBridge.getInstance()

  aboutToAppear() {
    const params = router.getParams() as WorkspacePageParams
    if (params) {
      this.projectId = params.projectId
      this.projectName = params.projectName
      this.loadProjectTree()
    }
  }

  async loadProjectTree() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
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

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor('#333333')
          .onClick(() => router.back())

        Text(this.projectName)
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor('#333333')
          .margin({ left: 12 })

        Blank()

        Button('+ 新卷')
          .fontSize(14)
          .height(32)
          .backgroundColor('#4A90D9')
          .onClick(async () => {
            const result = await this.bridge.createVolume(this.projectId, '新卷')
            if (result.success) {
              this.loadProjectTree()
            }
          })
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
            .onClick(() => this.loadProjectTree())
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
          .padding({ left: 16, right: 16, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#F5F5F5')
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
          .fillColor('#666666')
          .margin({ right: 8 })

        Text(volumeTree.volume.name)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor('#333333')

        Blank()

        Text(`${volumeTree.chapters.length} 章`)
          .fontSize(12)
          .fontColor('#999999')

        Button('+ 新章')
          .fontSize(12)
          .height(28)
          .backgroundColor('#7B68EE')
          .margin({ left: 8 })
          .onClick(async () => {
            const result = await this.bridge.createChapter(volumeTree.volume.id, '新章节')
            if (result.success) {
              this.loadProjectTree()
            }
          })
      }
      .width('100%')
      .padding(12)
      .backgroundColor('#FFFFFF')
      .borderRadius({ topLeft: 8, topRight: 8 })
      .onClick(() => this.toggleVolume(volumeTree.volume.id))

      // Chapters list
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        ForEach(volumeTree.chapters, (chapter: Chapter) => {
          Row() {
            Column() {
              Text(chapter.name)
                .fontSize(15)
                .fontColor('#333333')
                .margin({ bottom: 4 })

              Row() {
                Text(`${chapter.wordCount} 字`)
                  .fontSize(12)
                  .fontColor('#999999')
                  .margin({ right: 12 })

                Text(this.formatTime(chapter.updatedAt))
                  .fontSize(12)
                  .fontColor('#999999')
              }
            }
            .layoutWeight(1)
            .alignItems(HorizontalAlign.Start)

            Image($r('sys.media.ohos_ic_public_arrow_right'))
              .width(16)
              .height(16)
              .fillColor('#CCCCCC')
          }
          .width('100%')
          .padding({ left: 16, right: 16, top: 12, bottom: 12 })
          .backgroundColor('#FFFFFF')
          .borderRadius(0)
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
    .width('100%')
    .margin({ bottom: 12 })
    .borderRadius(8)
    .clip(true)
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
