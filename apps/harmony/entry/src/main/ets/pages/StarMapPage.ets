import router from '@ohos.router'
import WriterCoreBridge from '../bridge/WriterCoreBridge'
import { StarMapGraph, StarMapNode, StarMapEdge } from '../model/CoreDtos'

interface StarMapPageParams {
  starmapId: string
  starmapTitle: string
}

@Entry
@Component
struct StarMapPage {
  @State starmapId: string = ''
  @State starmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State selectedNodeId: string = ''
  @State scale: number = 1
  @State offsetX: number = 0
  @State offsetY: number = 0

  private bridge: WriterCoreBridge = WriterCoreBridge.getInstance()

  aboutToAppear() {
    const params = router.getParams() as StarMapPageParams
    if (params) {
      this.starmapId = params.starmapId
      this.starmapTitle = params.starmapTitle
      this.loadStarMap()
    }
  }

  async loadStarMap() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getStarMapGraph(this.starmapId)
      if (result.success && result.data) {
        this.graph = result.data
      } else {
        this.errorMessage = result.userMessage || '加载星图失败'
      }
    } catch (err) {
      this.errorMessage = '加载星图时发生错误'
    } finally {
      this.isLoading = false
    }
  }

  getNodeColor(node: StarMapNode): string {
    return node.color || '#4A90D9'
  }

  getNodeTypeName(type: string): string {
    const typeNames: Record<string, string> = {
      'character': '角色',
      'location': '地点',
      'event': '事件',
      'theme': '主题',
      'plot': '情节',
      'custom': '自定义'
    }
    return typeNames[type] || '未知'
  }

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor('#333333')
          .onClick(() => router.back())

        Text(this.starmapTitle)
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor('#333333')
          .margin({ left: 12 })

        Blank()

        // Zoom controls
        Row() {
          Button('-')
            .fontSize(18)
            .width(36)
            .height(36)
            .backgroundColor('#FFFFFF')
            .fontColor('#333333')
            .onClick(() => {
              this.scale = Math.max(0.5, this.scale - 0.1)
            })

          Text(`${Math.round(this.scale * 100)}%`)
            .fontSize(14)
            .fontColor('#666666')
            .width(50)
            .textAlign(TextAlign.Center)

          Button('+')
            .fontSize(18)
            .width(36)
            .height(36)
            .backgroundColor('#FFFFFF')
            .fontColor('#333333')
            .onClick(() => {
              this.scale = Math.min(2, this.scale + 0.1)
            })
        }
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
            .onClick(() => this.loadStarMap())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.graph) {
        Stack() {
          // Canvas for edges
          Canvas(this.buildCanvasContext())
            .width('100%')
            .height('100%')
            .backgroundColor('#FAFAFA')
            .onReady(() => {
              // Canvas is ready
            })

          // Nodes layer
          ForEach(this.graph.nodes, (node: StarMapNode) => {
            Column() {
              Circle({ width: 48, height: 48 })
                .fill(this.getNodeColor(node))
                .stroke(this.selectedNodeId === node.id ? '#FF6B6B' : 'transparent')
                .strokeWidth(3)

              Text(node.label)
                .fontSize(12)
                .fontColor('#333333')
                .margin({ top: 4 })
                .maxLines(1)
            }
            .position({
              x: (node.x * this.scale) + this.offsetX - 24,
              y: (node.y * this.scale) + this.offsetY - 24
            })
            .onClick(() => {
              this.selectedNodeId = node.id
            })
          }, (node: StarMapNode) => node.id)

          // Node info panel
          if (this.selectedNodeId) {
            this.NodeInfoPanel()
          }

          // Legend
          this.Legend()
        }
        .layoutWeight(1)
        .width('100%')
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#F5F5F5')
  }

  @Builder
  NodeInfoPanel() {
    Column() {
      Row() {
        Text('节点信息')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor('#333333')

        Blank()

        Text('X')
          .fontSize(16)
          .fontColor('#999999')
          .onClick(() => {
            this.selectedNodeId = ''
          })
      }
      .width('100%')
      .margin({ bottom: 12 })

      if (this.graph) {
        ForEach(this.graph.nodes.filter(n => n.id === this.selectedNodeId), (node: StarMapNode) => {
          Column() {
            Text(node.label)
              .fontSize(20)
              .fontWeight(FontWeight.Bold)
              .fontColor('#333333')
              .margin({ bottom: 8 })

            Row() {
              Text('类型：')
                .fontSize(14)
                .fontColor('#666666')
              Text(this.getNodeTypeName(node.type))
                .fontSize(14)
                .fontColor('#333333')
            }
            .margin({ bottom: 4 })

            if (node.description) {
              Text(node.description)
                .fontSize(14)
                .fontColor('#666666')
                .margin({ top: 8 })
            }

            // Related edges
            if (this.graph) {
              Text('关联关系：')
                .fontSize(14)
                .fontColor('#666666')
                .margin({ top: 12, bottom: 8 })

              ForEach(this.graph.edges.filter(e =>
                e.sourceId === this.selectedNodeId || e.targetId === this.selectedNodeId
              ), (edge: StarMapEdge) => {
                Row() {
                  Circle({ width: 8, height: 8 })
                    .fill('#4A90D9')
                    .margin({ right: 8 })

                  Text(edge.label || '未命名关系')
                    .fontSize(13)
                    .fontColor('#333333')
                }
                .margin({ bottom: 4 })
              }, (edge: StarMapEdge) => edge.id)
            }
          }
        }, (node: StarMapNode) => node.id)
      }
    }
    .width(280)
    .padding(16)
    .backgroundColor('#FFFFFF')
    .borderRadius(12)
    .shadow({ radius: 8, color: '#33000000', offsetX: 0, offsetY: 4 })
    .position({ x: 16, y: 16 })
  }

  @Builder
  Legend() {
    Column() {
      Text('图例')
        .fontSize(12)
        .fontColor('#999999')
        .margin({ bottom: 8 })

      Row() {
        Circle({ width: 12, height: 12 })
          .fill('#4A90D9')
          .margin({ right: 4 })
        Text('角色')
          .fontSize(11)
          .fontColor('#666666')
          .margin({ right: 12 })

        Circle({ width: 12, height: 12 })
          .fill('#F39C12')
          .margin({ right: 4 })
        Text('地点')
          .fontSize(11)
          .fontColor('#666666')
          .margin({ right: 12 })

        Circle({ width: 12, height: 12 })
          .fill('#9B59B6')
          .margin({ right: 4 })
        Text('事件')
          .fontSize(11)
          .fontColor('#666666')
      }
    }
    .padding(12)
    .backgroundColor('#FFFFFF')
    .borderRadius(8)
    .shadow({ radius: 4, color: '#1A000000', offsetX: 0, offsetY: 2 })
    .position({ x: 16, y: 'bottom' })
  }

  buildCanvasContext(): CanvasRenderingContext2D {
    const ctx = new CanvasRenderingContext2D(new RenderingContextSettings(true))
    // In a real implementation, we would draw edges here
    // For mock purposes, we'll leave the canvas empty
    return ctx
  }
}
