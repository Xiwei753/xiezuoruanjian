// WideProjectCoverCard — 宽屏作品页的单张书封卡片。
//
// Issue #972：宽屏作品页改为竖向书封卡片网格。
// 本组件只负责一张作品卡的视觉与点击 / 长按菜单，不读取 ViewModel、不调用 Core：
// - 书封比例（宽:高 = 0.72），卡片内依次是书名、卷/章数、总字数、最近编辑时间；
// - 点击卡片通过 onOpen 把作品 id / 标题交回页面；
// - 长按弹出重命名 / 删除菜单，通过 onRename / onDelete 把当前作品交回页面。
// 页面根据回调用现有 ViewModel 命令处理，卡片本身不保存业务状态。

import { ProjectSummary } from '../../../../corebridge/dto/ProjectDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'

// 书封竖向比例（宽:高 = 0.72）：卡片宽度完全由父级网格列宽决定，不再保存固定 vp。
const COVER_ASPECT_RATIO: number = 0.72

// 书名区与折角按卡片宽度取百分比，随列宽一起缩放：
// 折角是 45° 旋转的方块，被卡片裁掉外侧一半后形成右上三角，边长约为卡片宽度的 8.9%。
const COVER_FOLD_SIZE_PERCENT: string = '12.6%'
const COVER_HORIZONTAL_PADDING_PERCENT: string = '8.9%'
const COVER_HEADER_RIGHT_PADDING_PERCENT: string = '17.8%'
const COVER_HEADER_VERTICAL_PADDING_PERCENT: string = '5.5%'
const COVER_BODY_BOTTOM_PADDING_PERCENT: string = '6.7%'

@ComponentV2
export struct WideProjectCoverCard {
  @Param project: ProjectSummary = {
    id: '',
    title: '',
    createdAt: '',
    updatedAt: '',
    totalWordCount: 0,
    volumeCount: 0,
    chapterCount: 0
  }
  /** 点击卡片：打开作品（宽屏由页面直接进入写作工作台）。 */
  @Param onOpen: (projectId: string, projectTitle: string) => void =
    (_projectId: string, _projectTitle: string): void => {}
  /** 长按菜单：重命名当前作品。 */
  @Param onRename: (project: ProjectSummary) => void = (_project: ProjectSummary): void => {}
  /** 长按菜单：删除当前作品。 */
  @Param onDelete: (project: ProjectSummary) => void = (_project: ProjectSummary): void => {}

  private formatWordCount(count: number): string {
    if (count < 10000) {
      return `${count} 字`
    }
    const wan = count / 10000.0
    return `${wan.toFixed(1)} 万字`
  }

  // 书封上的最近编辑时间用绝对时间（26/8/11 22:37），与宽屏手稿和 Qt 端一致；
  // 不复用窄屏列表的「刚刚 / N 分钟前」相对时间。
  private formatEditedAt(isoString: string): string {
    if (!isoString) {
      return ''
    }
    const date = new Date(isoString)
    if (isNaN(date.getTime())) {
      return ''
    }
    const year = String(date.getFullYear()).slice(-2)
    const month = String(date.getMonth() + 1)
    const day = String(date.getDate())
    const hour = String(date.getHours()).padStart(2, '0')
    const minute = String(date.getMinutes()).padStart(2, '0')
    return `${year}/${month}/${day} ${hour}:${minute}`
  }

  // 长按菜单：重命名 / 删除都只派发当前作品，处理入口在页面。
  @Builder
  CoverMenu() {
    Menu() {
      MenuItem({ content: '重命名' })
        .onClick(() => {
          this.onRename(this.project)
        })
      MenuItem({ content: '删除' })
        .onClick(() => {
          this.onDelete(this.project)
        })
    }
  }

  build() {
    Stack({ alignContent: Alignment.TopEnd }) {
      Column() {
        // 书封书名区：高度由文字行数与百分比内边距自然决定，正文从它下面开始。
        Row() {
          Text(this.project.title)
            .fontSize(15)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
            .textAlign(TextAlign.Start)
        }
        .width('100%')
        .padding({
          left: COVER_HORIZONTAL_PADDING_PERCENT,
          right: COVER_HEADER_RIGHT_PADDING_PERCENT,
          top: COVER_HEADER_VERTICAL_PADDING_PERCENT,
          bottom: COVER_HEADER_VERTICAL_PADDING_PERCENT
        })
        .alignItems(VerticalAlign.Center)

        // 书名区底线：与手稿一致，正文从线下面开始。
        Divider()
          .width('100%')
          .color($r('sys.color.comp_background_gray'))

        Column() {
          Text(`${this.project.volumeCount}卷 ${this.project.chapterCount}章`)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .width('100%')
            .textAlign(TextAlign.Start)

          Blank()

          // 右下：总字数
          Text(this.formatWordCount(this.project.totalWordCount))
            .fontSize(13)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .width('100%')
            .textAlign(TextAlign.End)

          // 左下：最近编辑时间。重命名 / 删除只走整卡长按菜单，与参考图书封一致，
          // 不在卡片上常驻三点按钮，避免点菜单时误触进入作品。
          Text(this.formatEditedAt(this.project.updatedAt))
            .fontSize(11)
            .fontColor($r('sys.color.font_secondary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
            .textAlign(TextAlign.Start)
            .margin({ top: 4 })
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: COVER_HORIZONTAL_PADDING_PERCENT,
          right: COVER_HORIZONTAL_PADDING_PERCENT,
          top: COVER_HEADER_VERTICAL_PADDING_PERCENT,
          bottom: COVER_BODY_BOTTOM_PADDING_PERCENT
        })
        .alignItems(HorizontalAlign.Start)
      }
      .width('100%')
      .height('100%')

      // 右上折角：按卡片宽度比例取尺寸的方块旋转 45°，被卡片裁掉外侧后形成三角折角。
      Row()
        .width(COVER_FOLD_SIZE_PERCENT)
        .aspectRatio(1)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .rotate({ angle: 45 })
        .translate({ x: '50%', y: '-50%' })
    }
    .width('100%')
    .aspectRatio(COVER_ASPECT_RATIO)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
    .shadow({
      radius: DesignTokens.shadowLight.radius,
      color: DesignTokens.shadowLight.color,
      offsetY: DesignTokens.shadowLight.offsetY
    })
    .onClick(() => {
      this.onOpen(this.project.id, this.project.title)
    })
    // 宽屏手稿没有常驻三点按钮，重命名 / 删除走系统长按菜单，
    // 与 Qt 端整卡右键菜单同语义；点击仍然是打开作品。
    .bindContextMenu(this.CoverMenu, ResponseType.LongPress)
    .accessibilityText(`${this.project.title}，${this.project.volumeCount}卷 ${this.project.chapterCount}章`)
  }
}
