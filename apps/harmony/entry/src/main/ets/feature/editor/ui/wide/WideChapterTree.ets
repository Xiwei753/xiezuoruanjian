// WideChapterTree — 宽屏写作工作台左侧卷/章树（Issue #972）。
//
// 这不是窄屏 ChapterTreePane 的包装：宽屏树按桌面手稿重新排版，
// 项目标题、卷行、章行、选中态、加载/错误/空目录都在一个连续滚动列里。
// 组件只接收 WritingScreen 已有的目录状态和回调，不持有第二份卷章数据，
// 也不发 Core 请求。

import { ChapterMeta, VolumeTree } from '../../../../corebridge/dto/ProjectDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'

const TREE_HEADER_HEIGHT_VP: number = 64
const VOLUME_ROW_HEIGHT_VP: number = 40
const CHAPTER_ROW_HEIGHT_VP: number = 36

@ComponentV2
export struct WideChapterTree {
  @Param projectTitle: string = ''
  @Param volumeTrees: VolumeTree[] = []
  @Param expandedVolumes: Set<string> = new Set<string>()
  @Param selectedChapterId: string = ''
  @Param isLoading: boolean = false
  @Param loadError: boolean = false
  @Param onToggleVolume: (volumeId: string) => void = () => {}
  @Param onSelectChapter: (chapterId: string, chapterTitle: string, volumeId: string) => void = () => {}
  @Param onCreateVolume: () => void = () => {}
  @Param onCreateChapter: (volumeId: string) => void = () => {}
  @Param onDeleteVolume: (volumeId: string) => void = () => {}
  @Param onDeleteChapter: (chapterId: string) => void = () => {}
  @Param onRetry: () => void = () => {}

  private totalChapterCount(): number {
    let total: number = 0
    for (const volumeTree of this.volumeTrees) {
      total += volumeTree.chapters.length
    }
    return total
  }

  private titleText(): string {
    return this.projectTitle.length > 0
      ? this.projectTitle
      : getContext(this).resourceManager.getStringSync($r('app.string.chapters').id)
  }

  @Builder
  StateBlock(title: string, description: string, showRetry: boolean) {
    Column() {
      Text(title)
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .textAlign(TextAlign.Center)

      Text(description)
        .fontSize(12)
        .fontColor($r('sys.color.font_secondary'))
        .textAlign(TextAlign.Center)
        .margin({ top: 6 })

      if (showRetry) {
        Button($r('app.string.retry'), { type: ButtonType.Normal })
          .height(32)
          .padding({ left: 16, right: 16 })
          .fontSize(13)
          .fontColor($r('sys.color.font_emphasize_contrary'))
          .borderRadius(DesignTokens.radiusPill)
          .backgroundColor($r('sys.color.background_emphasize'))
          .margin({ top: 12 })
          .onClick(() => {
            this.onRetry()
          })
      }
    }
    .width('100%')
    .padding({ left: 20, right: 20, top: 32, bottom: 32 })
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .fillColor($r('sys.color.icon_secondary'))
          .margin({ right: 8 })
          .onClick(() => {
            this.onToggleVolume(volumeTree.volume.id)
          })

        Text(volumeTree.volume.title)
          .fontSize(14)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .onClick(() => {
            this.onToggleVolume(volumeTree.volume.id)
          })

        Text(`${volumeTree.chapters.length}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ right: 6 })
          .onClick(() => {
            this.onToggleVolume(volumeTree.volume.id)
          })

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(16)
            .fontColor([$r('sys.color.icon_primary')])
        }
        .width(28)
        .height(28)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor(Color.Transparent)
        .onClick(() => {
          this.onCreateChapter(volumeTree.volume.id)
        })

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(16)
            .height(16)
            .fillColor($r('sys.color.icon_primary'))
        }
        .width(28)
        .height(28)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor(Color.Transparent)
        .bindMenu([
          {
            value: '删除卷',
            action: (): void => {
              this.onDeleteVolume(volumeTree.volume.id)
            }
          }
        ])
      }
      .width('100%')
      .height(VOLUME_ROW_HEIGHT_VP)
      .padding({ left: 12, right: 8 })
      .alignItems(VerticalAlign.Center)

      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Text($r('app.string.no_chapters'))
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .width('100%')
            .padding({ left: 36, top: 8, bottom: 8 })
        } else {
          ForEach(volumeTree.chapters, (chapter: ChapterMeta) => {
            this.ChapterRow(chapter, volumeTree.volume.id)
          }, (chapter: ChapterMeta) => chapter.id)
        }
      }
    }
    .width('100%')
  }

  @Builder
  ChapterRow(chapter: ChapterMeta, volumeId: string) {
    Row() {
      Row()
        .width(3)
        .height(18)
        .borderRadius(2)
        .backgroundColor(this.selectedChapterId === chapter.id
          ? $r('sys.color.ohos_id_color_emphasize')
          : Color.Transparent)
        .margin({ right: 8 })

      Text(chapter.title)
        .fontSize(14)
        .fontWeight(this.selectedChapterId === chapter.id ? FontWeight.Medium : FontWeight.Normal)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)

      // 字数是 Core ChapterMeta 真值，UI 不自己数。
      Text(`${chapter.wordCount}`)
        .fontSize(11)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ left: 8 })

      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Image($r('sys.media.ohos_ic_public_more'))
          .width(16)
          .height(16)
          .fillColor($r('sys.color.icon_primary'))
      }
      .width(28)
      .height(28)
      .borderRadius(DesignTokens.radiusPill)
      .backgroundColor(Color.Transparent)
      .bindMenu([
        {
          value: '删除章节',
          action: (): void => {
            this.onDeleteChapter(chapter.id)
          }
        }
      ])
    }
    .width('100%')
    .height(CHAPTER_ROW_HEIGHT_VP)
    .padding({ left: 14, right: 6 })
    .alignItems(VerticalAlign.Center)
    .backgroundColor(this.selectedChapterId === chapter.id
      ? $r('sys.color.comp_background_gray')
      : Color.Transparent)
    .borderRadius(DesignTokens.radiusSm)
    .onClick(() => {
      this.onSelectChapter(chapter.id, chapter.title, volumeId)
    })
  }

  build() {
    Column() {
      Row() {
        Column() {
          Text(this.titleText())
            .fontSize(15)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.font_primary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          Text(`${this.totalChapterCount()} 章`)
            .fontSize(11)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ top: 3 })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(20)
            .fontColor([$r('sys.color.icon_primary')])
        }
        .width(32)
        .height(32)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .onClick(() => {
          this.onCreateVolume()
        })
      }
      .width('100%')
      .height(TREE_HEADER_HEIGHT_VP)
      .padding({ left: 16, right: 12 })
      .alignItems(VerticalAlign.Center)

      Row()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_divider'))

      Scroll() {
        Column() {
          if (this.loadError) {
            this.StateBlock('加载目录失败', '目录数据没有返回，可重试读取。', true)
          } else if (this.isLoading && this.volumeTrees.length === 0) {
            Column() {
              LoadingProgress()
                .width(32)
                .height(32)
              Text('加载中…')
                .fontSize(13)
                .fontColor($r('sys.color.font_secondary'))
                .margin({ top: 8 })
            }
            .width('100%')
            .padding(32)
            .justifyContent(FlexAlign.Center)
          } else if (this.volumeTrees.length === 0) {
            this.StateBlock(
              getContext(this).resourceManager.getStringSync($r('app.string.no_volumes_chapters').id),
              getContext(this).resourceManager.getStringSync($r('app.string.click_new_volume_to_start').id),
              false)
          } else {
            ForEach(this.volumeTrees, (volumeTree: VolumeTree) => {
              this.VolumeSection(volumeTree)
            }, (volumeTree: VolumeTree) => volumeTree.volume.id)
          }
        }
        .width('100%')
        .padding({ top: 8, bottom: 16 })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
