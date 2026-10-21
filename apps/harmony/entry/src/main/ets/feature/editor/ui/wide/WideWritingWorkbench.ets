// WideWritingWorkbench — 宽屏写作整页工作台（Issue #972）。
//
// 结构是真实布局流，不是旧宽屏分支的包装：
//
//   ┌──────────────────── 顶部贯通工具条 64vp ────────────────────┐
//   │ 返回 / 撤销 / 重做   字体 / 段落 / 替换   同步 / 搜索 / 设置 │
//   ├──────────┬─┬──────────────────────────────┬─┬──────────┬────┤
//   │ 章节树   │分│        编辑正文纸面          │分│ 工具面板 │工具│
//   │ 320vp    │隔│        （唯一 SujianEditor）  │隔│ 240vp    │轨  │
//   │          │条│                              │条│          │56vp│
//   │          │  │  底部：速度/进度/时间/保存    │  │          │    │
//   └──────────┴─┴──────────────────────────────┴─┴──────────┴────┘
//
// - 编辑正文由 WritingScreen 通过 @BuilderParam 传入现有 EditorBody()，
//   SujianEditor、IME、dispatcher、selection、viewport 全部只有那一份；
//   展开/收起/拖动只改布局宽度，不换编辑会话。
// - 左右面板收起后是同一工作台的两个状态：左栏留下贴边 handle，
//   右栏留下同一条分隔 handle + 最右常驻 rail。
// - 本组件不引入一级导航/底栏，外壳的 showPrimaryNavigation 由 Shell 处理。

import { ActionRole, ActionSlot } from '../../../../app/navigation/ScreenSemantics'
import { VolumeTree } from '../../../../corebridge/dto/ProjectDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { WideChapterTree } from './WideChapterTree'
import { WidePaneDivider } from './WidePaneDivider'
import { WideToolPanel } from './WideToolPanel'
import { WideToolRail } from './WideToolRail'
import { WideWritingStatus } from './WideWritingStatus'
import { WideWritingToolbar } from './WideWritingToolbar'

// 工具轨宽度按工作台宽度比例收缩，参考图约占 3.8%。
const TOOL_RAIL_WIDTH_RATIO: string = '3.8%'
const EDITOR_FOLD_STRIP_HEIGHT_VP: number = 28
// 正文纸面直接铺满分隔条之间的区域，参考图没有额外的左右留白。
const EDITOR_PAPER_MARGIN_VP: number = 0

@ComponentV2
export struct WideWritingWorkbench {
  // ── 顶部工具条 ──
  @Param headerActions: ActionSlot[] = []
  @Param fontSizePt: number = 0
  @Param lineSpacing: number = 0
  @Param onBack: () => void = () => {}
  @Param onUndo: () => void = () => {}
  @Param onRedo: () => void = () => {}
  @Param onFontSizeChange: (value: number) => void = () => {}
  @Param onLineSpacingChange: (value: number) => void = () => {}
  @Param onHeaderAction: (role: ActionRole) => void = () => {}

  // ── 左侧章节树 ──
  @Param projectTitle: string = ''
  @Param volumeTrees: VolumeTree[] = []
  @Param expandedVolumes: Set<string> = new Set<string>()
  @Param selectedChapterId: string = ''
  @Param isTreeLoading: boolean = false
  @Param projectTreeError: boolean = false
  @Param onToggleVolume: (volumeId: string) => void = () => {}
  @Param onSelectChapter: (chapterId: string, chapterTitle: string, volumeId: string) => void = () => {}
  @Param onCreateVolume: () => void = () => {}
  @Param onCreateChapter: (volumeId: string) => void = () => {}
  @Param onDeleteVolume: (volumeId: string) => void = () => {}
  @Param onDeleteChapter: (chapterId: string) => void = () => {}
  @Param onRetryTree: () => void = () => {}

  // ── 布局状态（全部由 WritingScreen 持有）──
  @Param hasChapter: boolean = false
  @Param leftPaneCollapsed: boolean = false
  @Param rightPaneCollapsed: boolean = false
  @Param chapterNavWidthVp: number = 0
  @Param chapterNavMinWidthVp: number = 0
  @Param chapterNavMaxWidthVp: number = 0
  @Param toolPaneWidthVp: number = 0
  @Param toolPaneMinWidthVp: number = 0
  @Param toolPaneMaxWidthVp: number = 0
  @Param selectedToolKey: string = 'starmap'
  @Param projectId: string = ''
  @Param onOpenStarMap: (starmapId: string, starmapTitle: string) => void =
    (_starmapId: string, _starmapTitle: string): void => {}
  @Param onChapterNavResize: (widthVp: number) => void = () => {}
  @Param onToolPaneResize: (widthVp: number) => void = () => {}
  @Param onCollapseChapterTree: () => void = () => {}
  @Param onExpandChapterTree: () => void = () => {}
  @Param onCollapseToolPane: () => void = () => {}
  @Param onExpandToolPane: () => void = () => {}
  @Param onSelectTool: (key: string) => void = () => {}
  @Param onToggleToolPane: () => void = () => {}
  @Param onRowWidthChange: (widthVp: number) => void = () => {}

  // ── 正文列底部状态 ──
  @Param charsPerMinute: number = 0
  @Param progressCurrent: number = 0
  // Core 没有「目标字数」字段；默认 0 表示未设置，状态栏不伪造分母。
  @Param progressTarget: number = 0
  @Param clockText: string = ''
  @Param saveStatus: string = ''
  @Param saveStatusColor: ResourceColor = '#0A59F7'

  /** WritingScreen 唯一的 EditorBody()：内部是唯一 SujianEditor 和编辑会话。 */
  @Builder
  emptyEditor() {}

  @BuilderParam editorContent: () => void = this.emptyEditor

  private toggleChapterTree(): void {
    if (this.leftPaneCollapsed) {
      this.onExpandChapterTree()
    } else {
      this.onCollapseChapterTree()
    }
  }

  @Builder
  ChapterTreeSlot() {
    WideChapterTree({
      projectTitle: this.projectTitle,
      volumeTrees: this.volumeTrees,
      expandedVolumes: this.expandedVolumes,
      selectedChapterId: this.selectedChapterId,
      isLoading: this.isTreeLoading,
      loadError: this.projectTreeError,
      onToggleVolume: (volumeId: string): void => {
        this.onToggleVolume(volumeId)
      },
      onSelectChapter: (chapterId: string, chapterTitle: string, volumeId: string): void => {
        this.onSelectChapter(chapterId, chapterTitle, volumeId)
      },
      onCreateVolume: (): void => {
        this.onCreateVolume()
      },
      onCreateChapter: (volumeId: string): void => {
        this.onCreateChapter(volumeId)
      },
      onDeleteVolume: (volumeId: string): void => {
        this.onDeleteVolume(volumeId)
      },
      onDeleteChapter: (chapterId: string): void => {
        this.onDeleteChapter(chapterId)
      },
      onRetry: (): void => {
        this.onRetryTree()
      }
    })
      .width(this.chapterNavWidthVp)
      .height('100%')
  }

  @Builder
  ToolRailSlot() {
    WideToolRail({
      selectedToolKey: this.selectedToolKey,
      toolPaneVisible: !this.rightPaneCollapsed,
      onSelectTool: (key: string): void => {
        this.onSelectTool(key)
      },
      onToggleToolPane: (): void => {
        this.onToggleToolPane()
      }
    })
      .width(TOOL_RAIL_WIDTH_RATIO)
      .height('100%')
  }

  build() {
    Column() {
      WideWritingToolbar({
        headerActions: this.headerActions,
        fontSizePt: this.fontSizePt,
        lineSpacing: this.lineSpacing,
        onBack: (): void => {
          this.onBack()
        },
        onUndo: (): void => {
          this.onUndo()
        },
        onRedo: (): void => {
          this.onRedo()
        },
        onFontSizeChange: (value: number): void => {
          this.onFontSizeChange(value)
        },
        onLineSpacingChange: (value: number): void => {
          this.onLineSpacingChange(value)
        },
        onHeaderAction: (role: ActionRole): void => {
          this.onHeaderAction(role)
        }
      })
        .width('100%')

      Row() {
        // 左：章节树 / 收起后的贴边 handle
        if (this.leftPaneCollapsed) {
          WidePaneDivider({
            side: 'left',
            collapsed: true,
            minWidthVp: this.chapterNavMinWidthVp,
            maxWidthVp: this.chapterNavMaxWidthVp,
            onExpand: (): void => {
              this.onExpandChapterTree()
            }
          })
        } else {
          this.ChapterTreeSlot()

          WidePaneDivider({
            side: 'left',
            collapsed: false,
            widthVp: this.chapterNavWidthVp,
            minWidthVp: this.chapterNavMinWidthVp,
            maxWidthVp: this.chapterNavMaxWidthVp,
            onResize: (widthVp: number): void => {
              this.onChapterNavResize(widthVp)
            },
            onCollapse: (): void => {
              this.onCollapseChapterTree()
            }
          })
        }

        // 中：正文列（纸张面 + 底部状态）。editorContent 是页面唯一的 EditorBody。
        Column() {
          // 正文列顶部折叠带：与手稿一样在正文顶部居中留一个展开/收起章节树的入口。
          Row() {
            Button({ type: ButtonType.Normal, stateEffect: true }) {
              Text(this.leftPaneCollapsed ? '⌃' : '⌄')
                .fontSize(14)
                .fontColor($r('sys.color.icon_secondary'))
            }
            .width(32)
            .height(24)
            .borderRadius(DesignTokens.radiusPill)
            .backgroundColor(Color.Transparent)
            .onClick(() => {
              this.toggleChapterTree()
            })
          }
          .width('100%')
          .height(EDITOR_FOLD_STRIP_HEIGHT_VP)
          .justifyContent(FlexAlign.Center)
          .alignItems(VerticalAlign.Center)

          // 纸张感正文面：折叠/拖拽只改变这里的可用宽度，不重建编辑会话。
          Column() {
            if (this.hasChapter) {
              this.editorContent()
            } else {
              Column() {
                Text('从左侧选择或新建章节')
                  .fontSize(16)
                  .fontColor($r('sys.color.font_secondary'))
              }
              .width('100%')
              .height('100%')
              .justifyContent(FlexAlign.Center)
              .alignItems(HorizontalAlign.Center)
            }
          }
          .width('100%')
          .layoutWeight(1)
          .margin({
            left: EDITOR_PAPER_MARGIN_VP,
            right: EDITOR_PAPER_MARGIN_VP,
            bottom: EDITOR_PAPER_MARGIN_VP
          })
          .borderRadius(DesignTokens.radiusLg)
          .clip(true)
          .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
          .shadow({
            radius: DesignTokens.shadowLight.radius,
            color: DesignTokens.shadowLight.color,
            offsetY: DesignTokens.shadowLight.offsetY
          })

          WideWritingStatus({
            charsPerMinute: this.charsPerMinute,
            progressCurrent: this.progressCurrent,
            progressTarget: this.progressTarget,
            clockText: this.clockText,
            saveStatus: this.saveStatus,
            saveStatusColor: this.saveStatusColor
          })
            .width('100%')
        }
        .layoutWeight(1)
        .height('100%')
        .backgroundColor($r('sys.color.background_secondary'))

        // 右：工具面板 / 收起后的 handle，之后永远保留最右工具轨
        if (this.rightPaneCollapsed) {
          WidePaneDivider({
            side: 'right',
            collapsed: true,
            minWidthVp: this.toolPaneMinWidthVp,
            maxWidthVp: this.toolPaneMaxWidthVp,
            onExpand: (): void => {
              this.onExpandToolPane()
            }
          })
        } else {
          WidePaneDivider({
            side: 'right',
            collapsed: false,
            widthVp: this.toolPaneWidthVp,
            minWidthVp: this.toolPaneMinWidthVp,
            maxWidthVp: this.toolPaneMaxWidthVp,
            onResize: (widthVp: number): void => {
              this.onToolPaneResize(widthVp)
            },
            onCollapse: (): void => {
              this.onCollapseToolPane()
            }
          })

          WideToolPanel({
            toolKey: this.selectedToolKey,
            projectId: this.projectId,
            onClose: (): void => {
              this.onCollapseToolPane()
            },
            onOpenStarMap: (starmapId: string, starmapTitle: string): void => {
              this.onOpenStarMap(starmapId, starmapTitle)
            }
          })
            .width(this.toolPaneWidthVp)
            .height('100%')
        }

        this.ToolRailSlot()
      }
      .width('100%')
      .layoutWeight(1)
      .onAreaChange((_oldArea: Area, newArea: Area): void => {
        this.onRowWidthChange(Number(newArea.width))
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
