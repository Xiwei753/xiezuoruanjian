// WideStarMapHome — 宽屏星图首页（Issue #972）。
//
// 只画内容区，不提供外壳：
// - 左侧一级导航、右上同步 / 搜索 / 设置由外部 WideAppShell 统一提供；
// - 本组件不含窄屏 FAB、CompactPrimaryActionLayer、底部导航；
// - 内容从左上开始按星图卡片网格排列，最后一张同尺寸「新建星图」卡是创建入口；
// - 空列表显示「暂无星图」+「创建第一个星图」，不出现右下角 FAB。
//
// 数据、创建、打开、重命名、删除与重试全部由 StarMapListScreen 传入，
// 组件自己不访问 bridge，也不复制 Core 业务状态。

import { StarMapMeta } from '../../../../corebridge/dto/StarMapDtos'
import { CompactPageState, CompactPageStateKind } from '../../../../ui/components/CompactPageState'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'

// 宽屏内容区与卡片度量：全部按父容器宽度取百分比，卡片宽度由等分列决定。
// 1244vp 窗口下内容区约 1056vp，4 列卡片约 247x163vp，与作品页同一套边距口径。
const WIDE_STARMAP_COLUMNS: string = '1fr 1fr 1fr 1fr'
const WIDE_CARD_ASPECT_RATIO: number = 1.52
const WIDE_PAGE_PADDING_PERCENT: string = '2%'
const WIDE_CONTENT_TOP_GAP_PERCENT: string = '0.7%'
const WIDE_CARD_PADDING_PERCENT: string = '6%'
const WIDE_HEADER_TOP_PADDING_PERCENT: string = '2%'
const WIDE_HEADER_BOTTOM_PADDING_PERCENT: string = '1.3%'
const WIDE_MORE_BUTTON_SIZE_VP: number = 32

@ComponentV2
export struct WideStarMapHome {
  @Param starMaps: StarMapMeta[] = []
  @Param isLoading: boolean = true
  @Param errorMessage: string = ''
  /** 动作表里存在 CreateStarMap 时才给创建入口，与窄屏 FAB 共用同一份语义。 */
  @Param canCreate: boolean = false

  /** 打开星图：参数与 StarMapListScreen.navigateToStarMap 一致。 */
  @Param onOpenStarMap: (starmapId: string, starmapTitle: string) => void =
    (_starmapId: string, _starmapTitle: string): void => {}
  /** 打开新建星图命名框：行为与现有 requestCreateStarMap 一致。 */
  @Param onCreate: () => void = (): void => {}
  /** 打开重命名框：行为与现有 startRename 一致。 */
  @Param onRename: (starmap: StarMapMeta) => void = (_starmap: StarMapMeta): void => {}
  /** 打开删除确认框：行为与现有 startDelete 一致。 */
  @Param onDelete: (starmap: StarMapMeta) => void = (_starmap: StarMapMeta): void => {}
  /** 重新加载列表：行为与现有 loadStarMaps 一致。 */
  @Param onRetry: () => void = (): void => {}

  // 更多菜单上下文：与窄屏 StarMapListScreen.currentMenuStarMapId 同语义。
  @Local currentMenuStarMapId: string = ''

  // 网格实测宽度只用来换算百分比间距，卡片本身仍由等分列和宽高比决定。
  @Local gridWidthVp: number = 0

  private cardGapVp(): number {
    return this.gridWidthVp * 0.016
  }

  private onGridAreaChange(area: Area): void {
    const width: number = Number(area.width)
    if (width > 0 && width !== this.gridWidthVp) {
      this.gridWidthVp = width
    }
  }

  // ─── 格式化工具（与窄屏列表保持同一时间口径） ───

  private formatTimestamp(timestamp: number): string {
    if (!timestamp || timestamp <= 0) {
      return ''
    }
    const date = new Date(timestamp)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes}分钟前`
    } else if (hours < 24) {
      return `${hours}小时前`
    } else if (days < 7) {
      return `${days}天前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  build() {
    Column() {
      this.PageHeader()

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
      } else if (this.starMaps.length === 0) {
        this.EmptyState()
      } else {
        this.StarMapGrid()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  /** 轻量页头：宽屏壳层没有标题栏，这里只给页面身份和一句说明。 */
  @Builder
  PageHeader() {
    Row() {
      Column({ space: 4 }) {
        Text('星图')
          .fontSize(24)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))

        Text('用节点与连线梳理故事的人物、事件和线索')
          .fontSize(13)
          .fontColor($r('sys.color.font_secondary'))
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      if (!this.isLoading && this.errorMessage.length === 0 && this.starMaps.length > 0) {
        Text(`${this.starMaps.length} 张星图`)
          .fontSize(13)
          .fontColor($r('sys.color.font_tertiary'))
      }
    }
    .width('100%')
    .padding({
      left: WIDE_PAGE_PADDING_PERCENT,
      right: WIDE_PAGE_PADDING_PERCENT,
      top: WIDE_HEADER_TOP_PADDING_PERCENT,
      bottom: WIDE_HEADER_BOTTOM_PADDING_PERCENT
    })
    .alignItems(VerticalAlign.Center)
  }

  /**
   * 星图卡片网格：从内容区左上开始按 4 等分列排列，卡片按列宽和 1.52 比例确定宽高。
   * 「新建星图」是网格里的最后一张同尺寸卡，不是悬浮按钮。
   */
  @Builder
  StarMapGrid() {
    Grid() {
      ForEach(this.starMaps, (starmap: StarMapMeta) => {
        GridItem() {
          this.StarMapCard(starmap)
        }
      }, (starmap: StarMapMeta) => starmap.starmapId)

      if (this.canCreate) {
        GridItem() {
          this.CreateStarMapCard()
        }
      }
    }
    .columnsTemplate(WIDE_STARMAP_COLUMNS)
    .columnsGap(this.cardGapVp())
    .rowsGap(this.cardGapVp())
    .width('100%')
    .layoutWeight(1)
    .padding({
      left: WIDE_PAGE_PADDING_PERCENT,
      right: WIDE_PAGE_PADDING_PERCENT,
      top: WIDE_CONTENT_TOP_GAP_PERCENT,
      bottom: WIDE_PAGE_PADDING_PERCENT
    })
    .onAreaChange((_oldArea: Area, newArea: Area): void => {
      this.onGridAreaChange(newArea)
    })
    .scrollBar(BarState.Off)
  }

  /** 宽屏星图卡：标题 / 描述 / 更新时间 / 更多菜单，整卡点击进入画布。 */
  @Builder
  StarMapCard(starmap: StarMapMeta) {
    Column() {
      Row() {
        if (starmap.accentColor.length > 0) {
          Row()
            .width(4)
            .height(22)
            .borderRadius(DesignTokens.radiusXs)
            .backgroundColor(starmap.accentColor)
            .margin({ right: 12 })
        }

        Column() {
          Text(starmap.title)
            .fontSize(18)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
            .textAlign(TextAlign.Start)

          if (starmap.description.length > 0) {
            Text(starmap.description)
              .fontSize(13)
              .fontColor($r('sys.color.font_secondary'))
              .maxLines(3)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .width('100%')
              .textAlign(TextAlign.Start)
              .margin({ top: 8 })
          }
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)

        Row() {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(20)
            .height(20)
            .fillColor($r('sys.color.icon_primary'))
            .onClick(() => {
              this.currentMenuStarMapId = starmap.starmapId
            })
            .bindMenu(this.StarMapMoreMenu)
        }
        .width(WIDE_MORE_BUTTON_SIZE_VP)
        .height(WIDE_MORE_BUTTON_SIZE_VP)
        .justifyContent(FlexAlign.Center)
        .alignItems(VerticalAlign.Center)
        .borderRadius(DesignTokens.radiusSm)
        .margin({ left: 8 })
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)

      Blank()

      Row() {
        Text(this.formatTimestamp(starmap.updatedAt))
          .fontSize(12)
          .fontColor($r('sys.color.font_tertiary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Blank()

        Text('打开')
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .aspectRatio(WIDE_CARD_ASPECT_RATIO)
    .padding(WIDE_CARD_PADDING_PERCENT)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .shadow({
      radius: DesignTokens.shadowLight.radius,
      color: DesignTokens.shadowLight.color,
      offsetY: DesignTokens.shadowLight.offsetY
    })
    .onClick(() => this.onOpenStarMap(starmap.starmapId, starmap.title))
  }

  /** 新建卡：与星图卡同宽同高，跟在最后一张星图后面。 */
  @Builder
  CreateStarMapCard() {
    Column({ space: 10 }) {
      SymbolGlyph($r('sys.symbol.plus'))
        .fontSize(30)
        .fontColor([$r('sys.color.icon_secondary')])

      Text('新建星图')
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
    }
    .width('100%')
    .aspectRatio(WIDE_CARD_ASPECT_RATIO)
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
    .accessibilityText('新建星图')
    .onClick(() => this.onCreate())
  }

  /** 宽屏空状态：明确给出「创建第一个星图」操作，不提示右下角 FAB。 */
  @Builder
  EmptyState() {
    Column({ space: 8 }) {
      Text('暂无星图')
        .fontSize(20)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))

      Text('从一个节点开始，把故事里的人物、事件和线索连成一张图。')
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
        .textAlign(TextAlign.Center)

      if (this.canCreate) {
        Button('创建第一个星图')
          .fontSize(15)
          .height(40)
          .margin({ top: 12 })
          .borderRadius(DesignTokens.inputFieldRadius)
          .onClick(() => this.onCreate())
      }
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .padding(WIDE_PAGE_PADDING_PERCENT)
  }

  /** 卡片更多菜单：重命名 + 删除，最终仍调用页面现有的两个回调。 */
  @Builder
  StarMapMoreMenu() {
    Menu() {
      MenuItem({ content: '重命名' })
        .onClick(() => {
          const target = this.starMaps.find((s: StarMapMeta) => s.starmapId === this.currentMenuStarMapId)
          if (target) {
            this.onRename(target)
          }
        })

      MenuItem({ content: '删除' })
        .onClick(() => {
          const target = this.starMaps.find((s: StarMapMeta) => s.starmapId === this.currentMenuStarMapId)
          if (target) {
            this.onDelete(target)
          }
        })
    }
  }
}
