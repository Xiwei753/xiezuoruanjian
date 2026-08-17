// ChapterTreePane — 纯 UI 组件：卷/章节树展示和交互回调。
// Issue #629 评论14 第1项：把 WorkspaceScreen 里卷/章节树的纯 UI 搬进来。
// 它只接收项目树、展开状态、selectedChapterId 和回调；
// 里面不能出现 EditorSessionCoordinator、IME、Dispatcher、saveChapter。
// 由 WritingScreen 在宽屏模式下持有并展示。

import { VolumeTree, ChapterMeta } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

@Component
export struct ChapterTreePane {
  // 外部传入的项目数据
  volumeTrees: VolumeTree[] = []
  // 当前展开的卷 ID 集合（由外部控制）
  expandedVolumes: Set<string> = new Set()
  // 当前选中的章节 ID（用于高亮）
  selectedChapterId: string = ''
  // 展开/折叠卷
  onToggleVolume: ((volumeId: string) => void) | null = null
  // 选择章节（宽屏左侧点章节 → 安全 replace 到新 Writing route）
  onSelectChapter: ((chapterId: string, chapterTitle: string, volumeId: string) => void) | null = null
  // 创建卷
  onCreateVolume: (() => void) | null = null
  // 创建章节
  onCreateChapter: ((volumeId: string) => void) | null = null
  // 删除卷
  onDeleteVolume: ((volumeId: string) => void) | null = null
  // 删除章节
  onDeleteChapter: ((chapterId: string) => void) | null = null

  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  private themeObserver: ReturnType<typeof ThemeObserverImpl> | null = null

  aboutToAppear() {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    HarmonyThemeAdapter.getInstance().addObserver(this.themeObserver)
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      HarmonyThemeAdapter.getInstance().removeObserver(this.themeObserver)
    }
  }

  build() {
    Column() {
      // 标题栏
      Row() {
        Text($r('app.string.chapters'))
          .fontSize(18)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Button('+')
          .fontSize(14)
          .width(28)
          .height(28)
          .backgroundColor(this.themeConfig.colors.primary)
          .fontColor(this.themeConfig.colors.onPrimary)
          .onClick(() => {
            if (this.onCreateVolume) {
              this.onCreateVolume()
            }
          })
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

      // 目录内容
      Scroll() {
        Column() {
          if (this.volumeTrees.length === 0) {
            Column() {
              Text($r('app.string.no_volumes_chapters'))
                .fontSize(14)
                .fontColor(this.themeConfig.colors.textSecondary)
                .margin({ bottom: 8 })
              Text($r('app.string.click_new_volume_to_start'))
                .fontSize(12)
                .fontColor(this.themeConfig.colors.textHint)
            }
            .width('100%')
            .padding(24)
            .justifyContent(FlexAlign.Center)
          } else {
            ForEach(this.volumeTrees, (volumeTree: VolumeTree) => {
              this.VolumeSection(volumeTree)
            }, (volumeTree: VolumeTree) => volumeTree.volume.id)
          }
        }
        .width('100%')
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.surface)
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      // 卷头
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .fillColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        Text(volumeTree.volume.title)
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(`${volumeTree.chapters.length}`)
          .fontSize(11)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ right: 8 })

        Button('+')
          .fontSize(12)
          .width(24)
          .height(24)
          .backgroundColor(this.themeConfig.colors.secondary)
          .fontColor(this.themeConfig.colors.onPrimary)
          .onClick(() => {
            if (this.onCreateChapter) {
              this.onCreateChapter(volumeTree.volume.id)
            }
          })
      }
      .width('100%')
      .padding({ left: 16, right: 12, top: 10, bottom: 10 })
      .onClick(() => {
        if (this.onToggleVolume) {
          this.onToggleVolume(volumeTree.volume.id)
        }
      })

      // 章节列表
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Text($r('app.string.no_chapters'))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
            .padding({ left: 28, top: 8, bottom: 8 })
        } else {
          ForEach(volumeTree.chapters, (chapter: ChapterMeta) => {
            Row() {
              Column() {
                Text(chapter.title)
                  .fontSize(14)
                  .fontColor(this.selectedChapterId === chapter.id
                    ? this.themeConfig.colors.primary
                    : this.themeConfig.colors.textPrimary)
                  .fontWeight(this.selectedChapterId === chapter.id ? FontWeight.Medium : FontWeight.Normal)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })

                Text(`${chapter.wordCount}`)
                  .fontSize(11)
                  .fontColor(this.themeConfig.colors.textHint)
                  .margin({ top: 2 })
              }
              .layoutWeight(1)
              .alignItems(HorizontalAlign.Start)

              Image($r('sys.media.ohos_ic_public_arrow_right'))
                .width(16)
                .height(16)
                .fillColor(this.themeConfig.colors.textHint)
                .margin({ left: 8 })
            }
            .width('100%')
            .padding({ left: 28, right: 12, top: 8, bottom: 8 })
            .backgroundColor(this.selectedChapterId === chapter.id
              ? this.themeConfig.colors.primaryLight
              : 'transparent')
            .onClick(() => {
              if (this.onSelectChapter) {
                this.onSelectChapter(chapter.id, chapter.title, volumeTree.volume.id)
              }
            })
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
  }
}
