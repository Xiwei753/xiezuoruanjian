import router from '@ohos.router'
import WriterCoreBridge from '../bridge/WriterCoreBridge'
import { Workspace, Project, RecentEdit } from '../model/CoreDtos'

@Entry
@Component
struct Index {
  @State workspaceName: string = '我的写作空间'
  @State projects: Project[] = []
  @State recentEdits: RecentEdit[] = []
  @State isLoading: boolean = true
  @State errorMessage: string = ''

  private bridge: WriterCoreBridge = WriterCoreBridge.getInstance()

  aboutToAppear() {
    this.loadWorkspace()
  }

  async loadWorkspace() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.openWorkspace('/mock/workspace')
      if (result.success && result.data) {
        this.workspaceName = result.data.workspace.name
        this.projects = result.data.projects
        this.recentEdits = result.data.recentEdits
      } else {
        this.errorMessage = result.userMessage || '加载工作区失败'
      }
    } catch (err) {
      this.errorMessage = '加载工作区时发生错误'
    } finally {
      this.isLoading = false
    }
  }

  build() {
    Column() {
      // Header
      Row() {
        Text('写作软件')
          .fontSize(24)
          .fontWeight(FontWeight.Bold)
          .fontColor('#333333')

        Blank()

        Text(this.workspaceName)
          .fontSize(14)
          .fontColor('#666666')
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
            .onClick(() => this.loadWorkspace())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            // Recent edits section
            if (this.recentEdits.length > 0) {
              this.RecentEditsSection()
            }

            // Projects section
            this.ProjectsSection()

            // StarMap entry
            this.StarMapEntry()
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
  RecentEditsSection() {
    Column() {
      Text('最近编辑')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor('#333333')
        .margin({ bottom: 12 })

      ForEach(this.recentEdits, (edit: RecentEdit) => {
        Row() {
          Column() {
            Text(edit.chapterTitle)
              .fontSize(16)
              .fontColor('#333333')
              .margin({ bottom: 4 })

            Text(edit.projectTitle)
              .fontSize(12)
              .fontColor('#999999')
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Text(this.formatTime(edit.editedAt))
            .fontSize(12)
            .fontColor('#999999')
        }
        .width('100%')
        .padding(12)
        .backgroundColor('#FFFFFF')
        .borderRadius(8)
        .margin({ bottom: 8 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WritingPage',
            params: {
              chapterId: edit.chapterId,
              chapterTitle: edit.chapterTitle
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
          .fontColor('#333333')

        Blank()

        Button('+ 新建作品')
          .fontSize(14)
          .height(32)
          .backgroundColor('#4A90D9')
          .onClick(async () => {
            const result = await this.bridge.createProject('新作品')
            if (result.success) {
              this.loadWorkspace()
            }
          })
      }
      .width('100%')
      .margin({ bottom: 12 })

      ForEach(this.projects, (project: Project) => {
        Row() {
          Column() {
            Text(project.name)
              .fontSize(16)
              .fontWeight(FontWeight.Medium)
              .fontColor('#333333')
              .margin({ bottom: 4 })

            Row() {
              Text(`${project.volumeCount} 卷`)
                .fontSize(12)
                .fontColor('#666666')
                .margin({ right: 12 })

              Text(`${project.chapterCount} 章`)
                .fontSize(12)
                .fontColor('#666666')
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
        .padding(16)
        .backgroundColor('#FFFFFF')
        .borderRadius(8)
        .margin({ bottom: 8 })
        .onClick(() => {
          router.pushUrl({
            url: 'pages/WorkspacePage',
            params: {
              projectId: project.id,
              projectName: project.name
            }
          })
        })
      }, (project: Project) => project.id)
    }
    .width('100%')
    .margin({ bottom: 24 })
  }

  @Builder
  StarMapEntry() {
    Column() {
      Text('星图')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor('#333333')
        .margin({ bottom: 12 })
        .width('100%')

      Row() {
        Column() {
          Text('人物关系图')
            .fontSize(16)
            .fontColor('#333333')
            .margin({ bottom: 4 })

          Text('8 个节点 · 12 条连线')
            .fontSize(12)
            .fontColor('#999999')
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)

        Button('查看')
          .fontSize(14)
          .height(32)
          .backgroundColor('#7B68EE')
          .onClick(() => {
            router.pushUrl({
              url: 'pages/StarMapPage',
              params: {
                starmapId: 'starmap-001',
                starmapTitle: '人物关系图'
              }
            })
          })
      }
      .width('100%')
      .padding(16)
      .backgroundColor('#FFFFFF')
      .borderRadius(8)
    }
    .width('100%')
  }

  formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes} 分钟前`
    } else if (hours < 24) {
      return `${hours} 小时前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }
}
