// WideProjectHome — 宽屏作品页正文。
//
// Issue #972：宽屏作品页只做一件事——从内容区左上开始排竖向书封卡片网格，
// 网格最后一张固定是同尺寸的「+」创建卡。没有页内大标题、没有 FAB、没有统计区；
// 左侧一级导航和顶部栏由外层宽屏壳提供，本组件不画页头。
//
// 组件边界：只接收作品列表 / 加载 / 错误文案和动作回调，不持有 ViewModel、
// 不直接调用 Core。打开、创建、重命名、删除、重试全部交回 HomeScreen，
// 由 HomeScreen 接现有 ProjectListViewModel 命令。
//
// 宽屏点作品由 HomeScreen 的 navigateToWorkspace 走宽屏分支直进写作工作台，
// 不经过窄屏的卷 / 章管理页。

import { ProjectSummary } from '../../../../corebridge/dto/ProjectDtos'
import { CompactPageState, CompactPageStateKind } from '../../../../ui/components/CompactPageState'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { WideProjectCoverCard } from './WideProjectCoverCard'

// 宽屏网格：列数由主画布实测宽度和书封最小可读宽度共同决定，不再固定 7 列。
// 1244vp 平板（右栏约 1101vp）正好落回设计稿的 7 列；窄一些的宽屏会自动降到 6/5/4 列。
const MIN_PROJECT_COVER_WIDTH_VP: number = 140
const WIDE_PROJECT_PAGE_PADDING_PERCENT: string = '2%'
const WIDE_PROJECT_PAGE_PADDING_RATIO: number = 0.04
const WIDE_PROJECT_CARD_GAP_RATIO: number = 0.011

@ComponentV2
export struct WideProjectHome {
  @Param projects: ProjectSummary[] = []
  @Param isLoading: boolean = false
  @Param errorMessage: string = ''
  /** 点击作品：由页面决定打开方式（宽屏直进写作工作台）。 */
  @Param onOpenProject: (projectId: string, projectTitle: string) => void =
    (_projectId: string, _projectTitle: string): void => {}
  /** 点击网格最后的「+」卡：打开新建作品命名框。 */
  @Param onCreateProject: () => void = (): void => {}
  /** 长按作品卡菜单：重命名。 */
  @Param onRenameProject: (project: ProjectSummary) => void = (_project: ProjectSummary): void => {}
  /** 长按作品卡菜单：删除。 */
  @Param onDeleteProject: (project: ProjectSummary) => void = (_project: ProjectSummary): void => {}
  /** 加载失败重试。 */
  @Param onRetry: () => void = (): void => {}

  @Local gridWidthVp: number = 0

  /** 卡片间距按网格实测宽度缩放，不写死某一台平板的 vp。 */
  private cardGapVp(): number {
    return this.gridWidthVp * WIDE_PROJECT_CARD_GAP_RATIO
  }

  /** 列数由去掉左右 2% 页边距后的可用宽度决定；至少保留一列可读书封。 */
  private projectColumnCount(): number {
    if (this.gridWidthVp <= 0) {
      return 7
    }
    const contentWidth: number = this.gridWidthVp * (1 - WIDE_PROJECT_PAGE_PADDING_RATIO)
    const gap: number = this.cardGapVp()
    const fitCount: number = Math.floor((contentWidth + gap) / (MIN_PROJECT_COVER_WIDTH_VP + gap))
    return Math.max(1, fitCount)
  }

  private projectColumnsTemplate(): string {
    const count: number = this.projectColumnCount()
    let template: string = '1fr'
    for (let index: number = 1; index < count; index++) {
      template += ' 1fr'
    }
    return template
  }

  private onGridAreaChange(area: Area): void {
    const width: number = Number(area.width)
    if (width > 0 && width !== this.gridWidthVp) {
      this.gridWidthVp = width
    }
  }

  build() {
    Column() {
      if (this.isLoading) {
        CompactPageState({
          kind: CompactPageStateKind.Loading,
          title: '加载中...'
        })
      } else if (this.errorMessage.length > 0) {
        CompactPageState({
          kind: CompactPageStateKind.Error,
          title: this.errorMessage,
          onRetry: (): void => {
            this.onRetry()
          }
        })
      } else {
        // 作品卡 + 新建卡共用同一张等分列网格；每格留出百分比间距，
        // 卡片自身按列宽和 0.72 比例确定宽高，因此 1244vp 下约为 135x187vp。
        Grid() {
          ForEach(this.projects, (project: ProjectSummary) => {
            GridItem() {
              WideProjectCoverCard({
                project: project,
                onOpen: (projectId: string, projectTitle: string): void => {
                  this.onOpenProject(projectId, projectTitle)
                },
                onRename: (target: ProjectSummary): void => {
                  this.onRenameProject(target)
                },
                onDelete: (target: ProjectSummary): void => {
                  this.onDeleteProject(target)
                }
              })
            }
          }, (project: ProjectSummary) => project.id)

          // 网格最后一张：与作品卡同宽同高的「+」创建卡，不是悬浮按钮。
          GridItem() {
            this.CreateCoverCard()
          }
        }
        .columnsTemplate(this.projectColumnsTemplate())
        .columnsGap(this.cardGapVp())
        .rowsGap(this.cardGapVp())
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: WIDE_PROJECT_PAGE_PADDING_PERCENT,
          right: WIDE_PROJECT_PAGE_PADDING_PERCENT,
          top: WIDE_PROJECT_PAGE_PADDING_PERCENT,
          bottom: WIDE_PROJECT_PAGE_PADDING_PERCENT
        })
        .onAreaChange((_oldArea: Area, newArea: Area): void => {
          this.onGridAreaChange(newArea)
        })
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  // 「+」卡：与书封作品卡同尺寸，卡片中央只有一个加号。
  // 它是网格成员，跟真实作品一起排版、换行、滚动；点击只打开命名框，
  // 输入名称后才由 ViewModel 真正创建作品。
  @Builder
  CreateCoverCard() {
    Column() {
      Text('+')
        .fontSize(40)
        .fontWeight(FontWeight.Normal)
        .fontColor($r('sys.color.icon_secondary'))
    }
    .width('100%')
    .aspectRatio(0.72)
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .borderRadius(DesignTokens.cardRadius)
    .border({ width: 1, color: $r('sys.color.icon_secondary') })
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .shadow({
      radius: DesignTokens.shadowLight.radius,
      color: DesignTokens.shadowLight.color,
      offsetY: DesignTokens.shadowLight.offsetY
    })
    .accessibilityText('新建作品')
    .onClick(() => {
      this.onCreateProject()
    })
  }
}
