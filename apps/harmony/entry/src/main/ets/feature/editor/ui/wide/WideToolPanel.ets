// WideToolPanel — 宽屏写作工作台右侧可展开工具面板（Issue #972）。
//
// 星图页签读取当前作品真实的星图列表：数据来自 Core 的
// listStarMapsForProject(projectId)，点击条目由 WritingScreen push 现有星图详情路由。
// AI 页签没有既有能力，明确显示“未启用”，不伪造生成结果或占位数据。
// 面板收起动作仍回到 onToggleToolPane。

import { getBridge } from '../../../../app/AppContext'
import { IWriterCoreBridge } from '../../../../corebridge/IWriterCoreBridge'
import { StarMapMeta } from '../../../../corebridge/dto/StarMapDtos'
import { AnnotationObjectRefDto } from '../../../../corebridge/dto/AnnotationDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { AnnotationPanel } from '../../annotation/AnnotationPanel'

const TOOL_PANEL_HEADER_HEIGHT_VP: number = 52

@ComponentV2
export struct WideToolPanel {
  @Param toolKey: string = 'starmap'
  @Param projectId: string = ''
  @Param annotationTarget: AnnotationObjectRefDto | null = null
  @Param onAnnotationChanged: () => void = () => {}
  @Param onClose: () => void = () => {}
  @Param onOpenStarMap: (starmapId: string, starmapTitle: string) => void =
    (_starmapId: string, _starmapTitle: string): void => {}

  @Local starMaps: StarMapMeta[] = []
  @Local isLoading: boolean = false
  @Local errorMessage: string = ''

  aboutToAppear(): void {
    void this.loadStarMaps()
  }

  private toolTitle(): string {
    if (this.toolKey === 'ai') return 'AI'
    if (this.toolKey === 'tags') return '标签'
    return '星图'
  }

  private async loadStarMaps(): Promise<void> {
    if (this.projectId.length === 0) {
      this.starMaps = []
      this.errorMessage = ''
      return
    }
    this.isLoading = true
    this.errorMessage = ''
    try {
      const bridge: IWriterCoreBridge = getBridge()
      const result = await bridge.listStarMapsForProject(this.projectId)
      if (result.success && result.data) {
        this.starMaps = result.data
      } else {
        this.errorMessage = result.userMessage ?? '加载作品星图失败'
      }
    } catch (error) {
      this.errorMessage = '加载作品星图失败'
    } finally {
      this.isLoading = false
    }
  }

  private formatUpdatedAt(timestamp: number): string {
    if (!timestamp || timestamp <= 0) {
      return ''
    }
    const date: Date = new Date(timestamp)
    return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`
  }

  @Builder
  PanelHeader() {
    Row() {
      Text(this.toolTitle())
        .fontSize(15)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))

      Blank()

      if (this.toolKey === 'starmap') {
        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Image($r('app.media.ic_refresh'))
            .width(18)
            .height(18)
            .fillColor($r('sys.color.icon_secondary'))
        }
        .width(28)
        .height(28)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .margin({ right: 6 })
        .accessibilityText('刷新星图')
        .onClick(() => {
          void this.loadStarMaps()
        })
      }

      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Text('›')
          .fontSize(16)
          .fontColor($r('sys.color.icon_secondary'))
      }
      .width(28)
      .height(28)
      .borderRadius(DesignTokens.radiusPill)
      .backgroundColor($r('sys.color.comp_background_gray'))
      .onClick(() => {
        this.onClose()
      })
    }
    .width('100%')
    .height(TOOL_PANEL_HEADER_HEIGHT_VP)
    .padding({ left: 16, right: 12 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  StarMapListBody() {
    if (this.isLoading) {
      Column() {
        LoadingProgress()
          .width(24)
          .height(24)
        Text('正在加载星图...')
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ top: 8 })
      }
      .width('100%')
      .padding({ top: 28, bottom: 28 })
      .alignItems(HorizontalAlign.Center)
    } else if (this.errorMessage.length > 0) {
      Column({ space: 8 }) {
        Text(this.errorMessage)
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .textAlign(TextAlign.Center)
        Button('重试')
          .fontSize(12)
          .height(28)
          .onClick(() => {
            void this.loadStarMaps()
          })
      }
      .width('100%')
      .padding(16)
      .alignItems(HorizontalAlign.Center)
    } else if (this.starMaps.length === 0) {
      Column({ space: 6 }) {
        Text('当前作品还没有星图')
          .fontSize(13)
          .fontColor($r('sys.color.font_primary'))
        Text('在星图页创建后，会在这里显示并可直接打开。')
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
          .textAlign(TextAlign.Center)
      }
      .width('100%')
      .padding({ left: 8, right: 8, top: 24, bottom: 24 })
      .alignItems(HorizontalAlign.Center)
    } else {
      Column({ space: 8 }) {
        ForEach(this.starMaps, (starmap: StarMapMeta) => {
          Column() {
            Row() {
              Text(starmap.title)
                .fontSize(13)
                .fontWeight(FontWeight.Medium)
                .fontColor($r('sys.color.font_primary'))
                .layoutWeight(1)
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
              Text('›')
                .fontSize(14)
                .fontColor($r('sys.color.icon_secondary'))
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)

            if (starmap.description.length > 0) {
              Text(starmap.description)
                .fontSize(11)
                .fontColor($r('sys.color.font_secondary'))
                .maxLines(2)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .margin({ top: 4 })
            }

            Text(this.formatUpdatedAt(starmap.updatedAt))
              .fontSize(10)
              .fontColor($r('sys.color.font_tertiary'))
              .margin({ top: 6 })
          }
          .width('100%')
          .padding(10)
          .alignItems(HorizontalAlign.Start)
          .borderRadius(DesignTokens.radiusMd)
          .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
          .onClick(() => {
            this.onOpenStarMap(starmap.starmapId, starmap.title)
          })
        }, (starmap: StarMapMeta) => starmap.starmapId)
      }
      .width('100%')
    }
  }

  @Builder
  AiBody() {
    Column({ space: 8 }) {
      Text('AI 尚未启用')
        .fontSize(13)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
      Text('当前版本没有可用的 AI 会话能力，这里不展示模拟结果。')
        .fontSize(11)
        .fontColor($r('sys.color.font_tertiary'))
        .lineHeight(18)
    }
    .width('100%')
    .padding(16)
    .alignItems(HorizontalAlign.Start)
  }

  build() {
    Column() {
      this.PanelHeader()

      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_divider'))

      Scroll() {
        Column() {
          if (this.toolKey === 'ai') {
            this.AiBody()
          } else if (this.toolKey === 'tags') {
            if (this.annotationTarget !== null) {
              AnnotationPanel({
                projectId: this.annotationTarget !== null ? this.annotationTarget.projectId : this.projectId,
                target: this.annotationTarget,
                onChanged: (): void => {
                  this.onAnnotationChanged()
                }
              })
                .width('100%')
                .layoutWeight(1)
            } else {
              Text('选择章节后可管理当前章节或文字选区的标签。')
                .fontSize(12)
                .fontColor($r('sys.color.font_secondary'))
                .padding(16)
            }
          } else {
            this.StarMapListBody()
          }
        }
        .width('100%')
        .padding(12)
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
