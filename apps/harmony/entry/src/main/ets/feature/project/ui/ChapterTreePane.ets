// ChapterTreePane — 纯 UI 组件：卷/章节树展示和交互回调。
// Issue #629 评论14 第1项：把 WorkspaceScreen 里卷/章节树的纯 UI 搬进来。
// 它只接收项目树、展开状态、selectedChapterId 和回调；
// 里面不能出现 EditorSessionCoordinator、IME、Dispatcher、saveChapter。
// 由 WritingScreen 在宽屏模式下持有并展示。

import { VolumeTree, ChapterMeta } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

@Component
export struct ChapterTreePane {
  // 外部传入的项目数据
  volumeTrees: VolumeTree[] = []
  // Issue #829 手稿：章节树最上面一组是作品名，不是笼统的"章节"字样。
  projectTitle: string = ''
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
  // 目录树加载中（纯 UI 状态，由 WritingScreen 传入）
  isLoading: boolean = false
  // 目录树加载失败（纯 UI 状态，由 WritingScreen 传入；空目录与失败是两种状态）
  loadError: boolean = false
  // 重试加载目录树：仍调 WritingScreen 那一个加载入口，不在本组件里自己发请求
  onRetry: (() => void) | null = null

  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  private themeObserver: ThemeObserver | null = null

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
      // Issue #829 手稿第一节：作品名静态标题行（不再可折叠，保留新建卷按钮）。
      Row() {
        Text(this.projectTitle.length > 0
          ? this.projectTitle
          : getContext(this).resourceManager.getStringSync($r('app.string.chapters').id))
          .fontSize(14)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        Button('+')
          .fontSize(14)
          .width(28)
          .height(28)
          .borderRadius(14)
          .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
          .border({ width: 1, color: $r('sys.color.icon_secondary') })
          .onClick(() => {
            if (this.onCreateVolume) {
              this.onCreateVolume()
            }
          })
      }
      .width('100%')
      .height(36)
      .padding({ left: 12, right: 8 })
      .alignItems(VerticalAlign.Center)

      // 目录内容
      Scroll() {
        Column() {
          if (this.loadError) {
            // 加载失败：显示错误提示和重试按钮
            Column() {
              Text('加载目录失败')
                .fontSize(14)
                .fontColor($r('sys.color.font_primary'))
                .margin({ bottom: 8 })
              Button('重试')
                .fontSize(14)
                .width(80)
                .height(32)
                .borderRadius(16)
                .backgroundColor($r('sys.color.background_emphasize'))
                .fontColor($r('sys.color.font_emphasize_contrary'))
                .onClick(() => {
                  if (this.onRetry) {
                    this.onRetry()
                  }
                })
            }
            .width('100%')
            .padding(24)
            .justifyContent(FlexAlign.Center)
          } else if (this.isLoading && this.volumeTrees.length === 0) {
            // 首次加载 / 失败后重试：还没有任何目录内容可显示时才用整块加载态。
            // 建卷、建章后的目录刷新继续显示现有树，不闪一下空白再复原。
            Column() {
              LoadingProgress()
                .width(32)
                .height(32)
              Text('加载中...')
                .fontSize(14)
                .margin({ top: 8 })
                .fontColor($r('sys.color.font_secondary'))
            }
            .width('100%')
            .padding(24)
            .justifyContent(FlexAlign.Center)
          } else if (this.volumeTrees.length === 0) {
            // 空目录（加载成功但无卷章）
            Column() {
              Text($r('app.string.no_volumes_chapters'))
                .fontSize(14)
                .margin({ bottom: 8 })
              Text($r('app.string.click_new_volume_to_start'))
                .fontSize(12)
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
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      // 卷头：折叠箭头 + 标题(数量) + 新建章节「+」。
      // ArkUI 的 @Builder 不能给同一个 builder 传不同回调，所以这里保留一份
      // 视觉完全一致、只把「+」接到 onCreateChapter 的实现。
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .margin({ right: 6 })
          .onClick(() => {
            if (this.onToggleVolume) {
              this.onToggleVolume(volumeTree.volume.id)
            }
          })

        Text(volumeTree.volume.title)
          .fontSize(14)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .onClick(() => {
            if (this.onToggleVolume) {
              this.onToggleVolume(volumeTree.volume.id)
            }
          })

        Text(`${volumeTree.chapters.length}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ right: 8 })
          .onClick(() => {
            if (this.onToggleVolume) {
              this.onToggleVolume(volumeTree.volume.id)
            }
          })

        Button('+')
          .fontSize(14)
          .width(28)
          .height(28)
          .borderRadius(14)
          .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
          .border({ width: 1, color: $r('sys.color.icon_secondary') })
          .onClick(() => {
            if (this.onCreateChapter) {
              this.onCreateChapter(volumeTree.volume.id)
            }
          })
      }
      .width('100%')
      .height(36)
      .padding({ left: 20, right: 8 })
      .alignItems(VerticalAlign.Center)

      // 章节列表：手稿里章节行是单行「标题 … 字数」，不再上下两行堆叠。
      if (this.expandedVolumes.has(volumeTree.volume.id)) {
        if (volumeTree.chapters.length === 0) {
          Text($r('app.string.no_chapters'))
            .fontSize(12)
            .padding({ left: 36, top: 8, bottom: 8 })
        } else {
          ForEach(volumeTree.chapters, (chapter: ChapterMeta) => {
            Row() {
              Text(chapter.title)
                .fontSize(14)
                .fontWeight(this.selectedChapterId === chapter.id ? FontWeight.Medium : FontWeight.Normal)
                .fontColor($r('sys.color.font_primary'))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)

              // 字数是 Core 的 ChapterMeta.wordCount 真值，UI 不自己数。
              Text(`${chapter.wordCount}`)
                .fontSize(11)
                .fontColor($r('sys.color.font_secondary'))
                .margin({ left: 8 })
            }
            .width('100%')
            .height(32)
            .padding({ left: 36, right: 12 })
            .alignItems(VerticalAlign.Center)
            .backgroundColor(this.selectedChapterId === chapter.id
              ? $r('sys.color.comp_background_gray')
              : Color.Transparent)
            .borderRadius(DesignTokens.radiusPill)
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
