// StarMapProofReplayPanel.ets — Issue #1036 星图创作记录只读回放（HarmonyOS）。
//
// 只消费 Core 的历史帧 JSON：节点文字、位置、事件来源和分支；不修改真实星图。
// 子图/连线细节沿用 Core 返回的 graph，平台端不重算操作顺序。

import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'

export interface StarMapProofNode {
  id: string
  title: string
  position: { x: number; y: number }
}

export interface StarMapProofEvent {
  actor: string
  entrypoint: string
  operation: string
  objectType: string
  objectId: string
}

export interface StarMapProofFrame {
  starmapId: string
  deviceId: string
  position: number
  totalEvents: number
  event: StarMapProofEvent | null
  state: { graph: { nodes: StarMapProofNode[] } }
  historyComplete: boolean
  valid: boolean
  errors: string[]
  branches: { deviceId: string; eventCount: number }[]
}

@Component
export struct StarMapProofReplayPanel {
  @Prop bridge: IWriterCoreBridge | null = null
  @Prop starmapId: string = ''
  onClose: () => void = () => {}

  @State private frame: StarMapProofFrame | null = null
  @State private position: number = 0
  @State private totalEvents: number = 0
  @State private deviceId: string = ''
  @State private playing: boolean = false
  @State private errorMessage: string = ''
  private timer: number = -1

  aboutToAppear() {
    void this.requestFrame(0)
  }

  aboutToDisappear() {
    this.stopPlayback()
  }

  private async requestFrame(target: number): Promise<void> {
    const bridge = this.bridge
    if (bridge === null || this.starmapId.length === 0) {
      return
    }
    const request = JSON.stringify({
      starmapId: this.starmapId,
      deviceId: this.deviceId,
      targetPosition: Math.max(0, Math.floor(target)),
      includeTrajectory: true
    })
    const envelope = await bridge.replayStarMapProof(request)
    if (!envelope.success || envelope.data === undefined || envelope.data === null) {
      this.errorMessage = envelope.userMessage ?? '无法读取星图创作记录'
      return
    }
    const frame = envelope.data as StarMapProofFrame
    if (this.deviceId.length === 0) {
      this.deviceId = frame.deviceId ?? ''
    }
    this.frame = frame
    this.position = frame.position
    this.totalEvents = frame.totalEvents
    this.errorMessage = ''
  }

  private stopPlayback(): void {
    this.playing = false
    if (this.timer >= 0) {
      clearInterval(this.timer)
      this.timer = -1
    }
  }

  private togglePlayback(): void {
    if (this.playing) {
      this.stopPlayback()
      return
    }
    if (this.totalEvents <= 0) {
      return
    }
    if (this.position >= this.totalEvents) {
      void this.requestFrame(0)
    }
    this.playing = true
    this.timer = setInterval(() => {
      if (!this.playing) {
        return
      }
      const next = Math.min(this.totalEvents, this.position + 1)
      void this.requestFrame(next)
      if (next >= this.totalEvents) {
        this.stopPlayback()
      }
    }, 200)
  }

  private sourceText(): string {
    const event = this.frame?.event
    if (event === null || event === undefined) {
      return '起始状态'
    }
    return `${event.actor} · ${event.entrypoint} · ${event.operation} ${event.objectType}`
  }

  build() {
    Column({ space: 8 }) {
      Row() {
        Text('星图创作记录')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
        Blank()
        Button('关闭')
          .fontSize(12)
          .onClick(() => {
            this.stopPlayback()
            this.onClose()
          })
      }
      .width('100%')

      Text(`位置 ${this.position} / ${this.totalEvents} · 设备 ${this.deviceId.length > 0 ? this.deviceId : '—'}`)
        .fontSize(11)
        .fontColor($r('sys.color.font_secondary'))
        .width('100%')

      Text(this.sourceText())
        .fontSize(11)
        .fontColor($r('sys.color.font_tertiary'))
        .width('100%')
        .maxLines(2)

      if (this.frame !== null && !this.frame.historyComplete) {
        Text('此前历史不完整（从真实基线开始）')
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
          .width('100%')
      }
      if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(11)
          .fontColor($r('sys.color.warning'))
          .width('100%')
      }

      Row({ space: 8 }) {
        Button(this.playing ? '暂停' : '播放')
          .fontSize(12)
          .enabled(this.totalEvents > 0)
          .onClick(() => this.togglePlayback())
        Button('上一步')
          .fontSize(12)
          .enabled(this.position > 0)
          .onClick(() => {
            this.stopPlayback()
            void this.requestFrame(Math.max(0, this.position - 1))
          })
        Button('下一步')
          .fontSize(12)
          .enabled(this.position < this.totalEvents)
          .onClick(() => {
            this.stopPlayback()
            void this.requestFrame(Math.min(this.totalEvents, this.position + 1))
          })
      }
      .width('100%')

      List({ space: 6 }) {
        ForEach(this.frame?.state.graph.nodes ?? [], (node: StarMapProofNode) => {
          ListItem() {
            Row() {
              Text(node.title.length > 0 ? node.title : node.id)
                .fontSize(12)
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
              Blank()
              Text(`(${Math.round(node.position.x)}, ${Math.round(node.position.y)})`)
                .fontSize(10)
                .fontColor($r('sys.color.font_tertiary'))
            }
            .width('100%')
            .padding({ top: 4, bottom: 4 })
          }
        }, (node: StarMapProofNode) => node.id)
      }
      .layoutWeight(1)
      .width('100%')
    }
    .width('90%')
    .height('70%')
    .padding(16)
    .backgroundColor($r('sys.color.background_primary'))
    .borderRadius(12)
  }
}
