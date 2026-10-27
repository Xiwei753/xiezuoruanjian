// ProofReplayPanel — 手写证明回放（Issue #1008）。
// 只渲染 Core 返回的正文、选区、来源与校验结果，不在 ArkTS 侧推演编辑规则。
// 时间轴按章节内事件位置（0..N）定位，播放间隔由事件真实时间差和倍速计算。

import { WritingWorkflowCoordinator } from '../session/WritingWorkflowCoordinator'

interface ProofDeviceBranch {
  deviceId?: string
}

interface ProofReplayFrame {
  chapterId?: string
  seq?: number
  position?: number
  totalEvents?: number
  timestampMs?: number
  nextTimestampMs?: number
  text?: string
  source?: string
  valid?: boolean
  errors?: string[]
  branches?: ProofDeviceBranch[]
}

@Component
export struct ProofReplayPanel {
  chapterId: string = ''
  coordinator: WritingWorkflowCoordinator | null = null

  @State private text: string = ''
  @State private deviceId: string = ''
  @State private branches: string[] = []
  @State private eventPosition: number = 0
  @State private totalEvents: number = 0
  @State private timestampMs: number = 0
  @State private nextTimestampMs: number = 0
  @State private seq: number = 0
  @State private source: string = ''
  @State private valid: boolean = false
  @State private errors: string = ''
  @State private playing: boolean = false
  @State private speed: number = 1
  @State private exportJson: string = ''
  private timerId: number = -1

  aboutToAppear(): void {
    void this.loadFrame(-1)
  }

  aboutToDisappear(): void {
    this.stopPlayback()
  }

  private stopPlayback(): void {
    this.playing = false
    if (this.timerId >= 0) {
      clearTimeout(this.timerId)
      this.timerId = -1
    }
  }

  private requestFrame(targetIndex: number): string {
    if (targetIndex < 0) {
      return JSON.stringify({ chapterId: this.chapterId, deviceId: this.deviceId })
    }
    return JSON.stringify({
      chapterId: this.chapterId,
      deviceId: this.deviceId,
      targetIndex: targetIndex
    })
  }

  private formatTime(ms: number): string {
    if (ms <= 0) {
      return '事件时间未知'
    }
    return new Date(ms).toLocaleString()
  }

  private async loadFrame(targetIndex: number): Promise<void> {
    if (this.chapterId.length === 0 || this.coordinator === null) {
      return
    }
    const envelope = await this.coordinator.replayChapterProof(this.requestFrame(targetIndex))
    if (!envelope.success || envelope.data === undefined || envelope.data === null) {
      this.errors = envelope.userMessage !== undefined ? envelope.userMessage : '无法读取回放数据'
      return
    }
    const frame = envelope.data as ProofReplayFrame
    this.text = frame.text !== undefined ? frame.text : ''
    this.branches = []
    const branches = frame.branches
    if (branches !== undefined) {
      for (let index = 0; index < branches.length; index++) {
        const branch = branches[index]
        if (branch.deviceId !== undefined && branch.deviceId.length > 0) {
          this.branches.push(branch.deviceId)
        }
      }
    }
    this.eventPosition = frame.position !== undefined ? frame.position : 0
    this.totalEvents = frame.totalEvents !== undefined ? frame.totalEvents : 0
    this.timestampMs = frame.timestampMs !== undefined ? frame.timestampMs : 0
    this.nextTimestampMs = frame.nextTimestampMs !== undefined ? frame.nextTimestampMs : 0
    this.seq = frame.seq !== undefined ? frame.seq : 0
    this.source = frame.source !== undefined ? frame.source : ''
    this.valid = frame.valid === true
    this.errors = frame.errors !== undefined ? frame.errors.join('\n') : ''
  }

  private scheduleNext(): void {
    if (!this.playing) {
      return
    }
    if (this.eventPosition >= this.totalEvents) {
      this.stopPlayback()
      return
    }
    const delta = this.nextTimestampMs > this.timestampMs
      ? this.nextTimestampMs - this.timestampMs
      : 260
    const delay = Math.max(40, Math.min(2000, Math.round(delta / Math.max(0.5, this.speed))))
    this.timerId = setTimeout((): void => {
      if (!this.playing) {
        return
      }
      void this.loadFrame(this.eventPosition + 1).then((): void => {
        this.scheduleNext()
      })
    }, delay)
  }

  private startPlayback(): void {
    if (this.totalEvents <= 0 || this.eventPosition >= this.totalEvents) {
      return
    }
    this.playing = true
    this.scheduleNext()
  }

  private selectSpeed(speed: number): void {
    this.speed = speed
  }

  private async exportPackage(): Promise<void> {
    if (this.coordinator === null) {
      return
    }
    const envelope = await this.coordinator.exportChapterProof(this.chapterId, this.deviceId)
    if (!envelope.success || envelope.data === undefined || envelope.data === null) {
      this.errors = envelope.userMessage !== undefined ? envelope.userMessage : '导出失败'
      return
    }
    this.exportJson = JSON.stringify(envelope.data, null, 2)
  }

  build() {
    Column({ space: 10 }) {
      Row({ space: 8 }) {
        Button(this.playing ? '暂停' : '播放')
          .enabled(this.totalEvents > 0)
          .onClick(() => {
            if (this.playing) {
              this.stopPlayback()
            } else {
              this.startPlayback()
            }
          })
        Button('上一步')
          .enabled(this.eventPosition > 0)
          .onClick(() => {
            this.stopPlayback()
            void this.loadFrame(this.eventPosition - 1)
          })
        Button('下一步')
          .enabled(this.eventPosition < this.totalEvents)
          .onClick(() => {
            this.stopPlayback()
            void this.loadFrame(this.eventPosition + 1)
          })
        Blank()
        Text(`事件 ${this.eventPosition} / ${this.totalEvents} · ${this.source}`)
          .fontSize(12)
      }
      .width('100%')

      if (this.branches.length > 1) {
        Scroll() {
          Row({ space: 6 }) {
            Text('设备分支').fontSize(12)
            ForEach(this.branches, (id: string) => {
              Button(id)
                .fontColor(this.deviceId === id
                  ? $r('sys.color.ohos_id_color_primary')
                  : $r('sys.color.ohos_id_color_text_primary'))
                .onClick(() => {
                  this.stopPlayback()
                  this.deviceId = id
                  void this.loadFrame(-1)
                })
            }, (id: string) => id)
          }
        }
        .scrollable(ScrollDirection.Horizontal)
        .width('100%')
      }

      Row({ space: 6 }) {
        Text('倍速').fontSize(12)
        Button('0.5x').onClick(() => { this.selectSpeed(0.5) })
        Button('1x').onClick(() => { this.selectSpeed(1) })
        Button('2x').onClick(() => { this.selectSpeed(2) })
        Button('4x').onClick(() => { this.selectSpeed(4) })
        Text(`${this.speed}x`).fontSize(12).fontColor($r('sys.color.ohos_id_color_primary'))
      }
      .width('100%')

      Slider({
        value: this.eventPosition,
        min: 0,
        max: this.totalEvents > 0 ? this.totalEvents : 1,
        step: 1,
        style: SliderStyle.OutSet
      })
        .width('100%')
        .enabled(this.totalEvents > 0)
        .onChange((value: number) => {
          this.stopPlayback()
          void this.loadFrame(Math.round(value))
        })

      Text(`发生时间 ${this.formatTime(this.timestampMs)} · 链序号 ${this.seq}`)
        .fontSize(12)
        .width('100%')

      Scroll() {
        Text(this.text.length > 0 ? this.text : '该章节还没有可回放的编辑事件')
          .fontSize(15)
          .width('100%')
      }
      .layoutWeight(1)
      .width('100%')

      if (this.errors.length > 0) {
        Text(this.errors)
          .fontSize(12)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .width('100%')
      }

      Row({ space: 8 }) {
        Text(this.valid ? '哈希链校验通过' : '校验未通过或记录不完整')
          .fontSize(12)
          .fontColor(this.valid ? $r('sys.color.ohos_id_color_primary') : $r('sys.color.ohos_id_color_alert'))
        Blank()
        Button('导出证明包（含历史正文）')
          .enabled(this.totalEvents > 0)
          .onClick(() => { void this.exportPackage() })
      }
      .width('100%')

      if (this.exportJson.length > 0) {
        Scroll() {
          Text(this.exportJson)
            .fontSize(11)
            .fontFamily('monospace')
            .width('100%')
        }
        .height(140)
        .width('100%')
      }
    }
    .padding(16)
    .width('100%')
    .height('100%')
  }
}
