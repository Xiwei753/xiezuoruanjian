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
  // 作品名分组是否展开（手稿的「作品名 ∨」）
  projectGroupExpanded: boolean = true
  // 章纲分组是否展开（手稿底部的「章纲 ∨」；本轮只画分组头，内容后续接）
  outlineGroupExpanded: boolean = false
  onToggleProjectGroup: (() => void) | null = null
  onToggleOutlineGroup: (() => void) | null = null
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

  // Issue #829 手稿：分组头统一形态 —— 折叠箭头 + 标题(+ 数量) + 可选「+」。
  // 「+」只有一种语义（新建卷），所以不外传回调，直接用 onCreateVolume。
  @Builder
  GroupHeader(label: string, expanded: boolean, trailing: string, showAdd: boolean, onToggle: () => void) {
    Row() {
      Image(expanded
        ? $r('sys.media.ohos_ic_public_arrow_down')
        : $r('sys.media.ohos_ic_public_arrow_right'))
        .width(16)
        .height(16)
        .margin({ right: 6 })

      Text(label)
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)

      if (trailing.length > 0) {
        Text(trailing)
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ right: 8 })
      }

      if (showAdd) {
        Button('+')
          .fontSize(14)
          .width(24)
          .height(24)
          .backgroundColor(Color.Transparent)
          .onClick(() => {
            if (this.onCreateVolume) {
              this.onCreateVolume()
            }
          })
      }
    }
    .width('100%')
    .height(36)
    .padding({ left: 12, right: 8 })
    .alignItems(VerticalAlign.Center)
    .onClick(() => {
      onToggle()
    })
  }

  build() {
    Column() {
      // Issue #829 手稿第一节：作品名分组（原本这里是一行"章节"标题 + 新建卷）。
      this.GroupHeader(
        this.projectTitle.length > 0
          ? this.projectTitle
          : getContext(this).resourceManager.getStringSync($r('app.string.chapters').id),
        this.projectGroupExpanded, '', true,
        (): void => {
          if (this.onToggleProjectGroup) {
            this.onToggleProjectGroup()
          }
        })

      // 目录内容
      if (this.projectGroupExpanded) {
        Scroll() {
          Column() {
            if (this.volumeTrees.length === 0) {
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
      } else {
        Blank().layoutWeight(1)
      }

      // Issue #829 手稿最后一节：章纲分组。本轮只固定分组头与折叠交互，
      // 展开内容等章纲功能真正立项后再接（Core 已有 chapter.note 字段）。
      this.GroupHeader(
        getContext(this).resourceManager.getStringSync($r('app.string.chapter_outline').id),
        this.outlineGroupExpanded, '', false,
        (): void => {
          if (this.onToggleOutlineGroup) {
            this.onToggleOutlineGroup()
          }
        })
    }
    .width('100%')
    .height('100%')
  }

  @Builder
  VolumeSection(volumeTree: VolumeTree) {
    Column() {
      // 卷头：和作品名 / 章纲同一个 GroupHeader 形态，只是「+」的语义换成新建章节。
      // ArkUI 的 @Builder 不能给同一个 builder 传不同回调，所以这里保留一份
      // 视觉完全一致、只把「+」接到 onCreateChapter 的实现。
      Row() {
        Image(this.expandedVolumes.has(volumeTree.volume.id)
          ? $r('sys.media.ohos_ic_public_arrow_down')
          : $r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .margin({ right: 6 })

        Text(volumeTree.volume.title)
          .fontSize(14)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        Text(`${volumeTree.chapters.length}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ right: 8 })

        Button('+')
          .fontSize(14)
          .width(24)
          .height(24)
          .backgroundColor(Color.Transparent)
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
      .onClick(() => {
        if (this.onToggleVolume) {
          this.onToggleVolume(volumeTree.volume.id)
        }
      })

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
