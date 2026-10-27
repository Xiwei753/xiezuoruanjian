// ProofReplayPanel — 手写证明回放（Issue #1008）。
// 只渲染 Core 返回的正文、选区、来源与校验结果，不在 ArkTS 侧推演编辑规则。

import { WritingWorkflowCoordinator } from '../session/WritingWorkflowCoordinator'

interface ProofReplayFrame {
  chapterId?: string
  seq?: number
  lastSeq?: number
  text?: string
  source?: string
  valid?: boolean
  errors?: string[]
}

@Component
export struct ProofReplayPanel {
  chapterId: string = ''
  coordinator: WritingWorkflowCoordinator | null = null

  @State private text: string = ''
  @State private seq: number = 0
  @State private lastSeq: number = 0
  @State private source: string = ''
  @State private valid: boolean = false
  @State private errors: string = ''
  @State private playing: boolean = false
  @State private exportJson: string = ''
  private timerId: number = -1

  aboutToAppear(): void {
    this.loadFrame(-1)
  }

  aboutToDisappear(): void {
    this.stopPlayback()
  }

  private stopPlayback(): void {
    this.playing = false
    if (this.timerId >= 0) {
      clearInterval(this.timerId)
      this.timerId = -1
    }
  }

  private requestFrame(targetSeq: number): string {
    if (targetSeq < 0) {
      return JSON.stringify({ chapterId: this.chapterId, deviceId: '' })
    }
    return JSON.stringify({ chapterId: this.chapterId, deviceId: '', targetSeq: targetSeq })
  }

  private async loadFrame(targetSeq: number): Promise<void> {
    if (this.chapterId.length === 0 || this.coordinator === null) {
      return
    }
    const envelope = await this.coordinator.replayChapterProof(this.requestFrame(targetSeq))
    if (!envelope.success || envelope.data === undefined || envelope.data === null) {
      this.errors = envelope.userMessage !== undefined ? envelope.userMessage : '无法读取回放数据'
      return
    }
    const frame = envelope.data as ProofReplayFrame
    this.text = frame.text !== undefined ? frame.text : ''
    this.seq = frame.seq !== undefined ? frame.seq : 0
    this.lastSeq = frame.lastSeq !== undefined ? frame.lastSeq : 0
    this.source = frame.source !== undefined ? frame.source : ''
    this.valid = frame.valid === true
    this.errors = frame.errors !== undefined ? frame.errors.join('\n') : ''
  }

  private startPlayback(): void {
    if (this.lastSeq <= this.seq) {
      return
    }
    this.playing = true
    this.timerId = setInterval((): void => {
      if (this.seq >= this.lastSeq) {
        this.stopPlayback()
        return
      }
      void this.loadFrame(this.seq + 1)
    }, 260)
  }

  private async exportPackage(): Promise<void> {
    if (this.coordinator === null) {
      return
    }
    const envelope = await this.coordinator.exportChapterProof(this.chapterId, '')
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
          .enabled(this.lastSeq > 0)
          .onClick(() => {
            if (this.playing) {
              this.stopPlayback()
            } else {
              this.startPlayback()
            }
          })
        Button('上一步')
          .enabled(this.seq > 0)
          .onClick(() => {
            this.stopPlayback()
            void this.loadFrame(this.seq - 1)
          })
        Button('下一步')
          .enabled(this.seq < this.lastSeq)
          .onClick(() => {
            this.stopPlayback()
            void this.loadFrame(this.seq + 1)
          })
        Blank()
        Text(`事件 ${this.seq} / ${this.lastSeq} · ${this.source}`)
          .fontSize(12)
      }
      .width('100%')

      Slider({
        value: this.seq,
        min: 0,
        max: this.lastSeq > 0 ? this.lastSeq : 1,
        step: 1,
        style: SliderStyle.OutSet
      })
        .width('100%')
        .enabled(this.lastSeq > 0)
        .onChange((value: number) => {
          this.stopPlayback()
          void this.loadFrame(Math.round(value))
        })

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
          .enabled(this.lastSeq > 0)
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
