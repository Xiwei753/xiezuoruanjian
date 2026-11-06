// StarMapWritingSessionTracker.ets — Issue #1036 星图前台计时。
//
// 第一次真实人工星图操作才开始计时；思考、停留继续累计；离开星图/页面时结算。
// 星图 ID 放进 starmapId，chapterId 保持为空；纯后台 MCP 写入不经过这里。
// 时长用系统单调时钟测量，墙钟只用于锚定本地日期。

import { systemDateTime } from '@kit.BasicServicesKit'
import { util } from '@kit.ArkTS'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { WritingActivitySegmentInput } from '../../../corebridge/dto/StatsDtos'

const FLUSH_INTERVAL_MS: number = 30000

export class StarMapWritingSessionTracker {
  private bridge: IWriterCoreBridge | null = null
  private active: boolean = false
  private sessionId: string = ''
  private projectId: string = ''
  private starMapId: string = ''
  private monoAnchorMs: number = 0
  private wallAnchorMs: number = 0
  private sliceStartMonoMs: number = 0

  private nowMonoMs(): number {
    return systemDateTime.getUptime(systemDateTime.TimeType.ACTIVE, false)
  }

  /** 第一次真实人工操作；同一目标重复调用不重开。 */
  start(bridge: IWriterCoreBridge | null, projectId: string, starMapId: string): void {
    if (starMapId.length === 0) {
      return
    }
    this.bridge = bridge
    if (this.active && this.starMapId === starMapId && this.projectId === projectId) {
      return
    }
    if (this.active) {
      this.flush('targetSwitch')
    }
    this.active = true
    this.sessionId = util.generateRandomUUID(false)
    this.projectId = projectId
    this.starMapId = starMapId
    const mono = this.nowMonoMs()
    this.sliceStartMonoMs = mono
    this.monoAnchorMs = mono
    this.wallAnchorMs = Date.now()
  }

  private flush(reason: string): void {
    if (!this.active || this.bridge === null || this.starMapId.length === 0) {
      return
    }
    const mono = this.nowMonoMs()
    const durationMs = mono - this.sliceStartMonoMs
    if (durationMs <= 0) {
      return
    }
    const input: WritingActivitySegmentInput = {
      eventId: util.generateRandomUUID(false),
      platform: 'harmony',
      projectId: this.projectId,
      volumeId: '',
      chapterId: '',
      starmapId: this.starMapId,
      sessionId: this.sessionId,
      startAtMs: this.wallAnchorMs + (this.sliceStartMonoMs - this.monoAnchorMs),
      endAtMs: this.wallAnchorMs + (mono - this.monoAnchorMs),
      pauseReason: reason
    }
    this.bridge.recordWritingActivitySegment(input)
    this.sliceStartMonoMs = mono
  }

  heartbeat(): void {
    if (!this.active) {
      return
    }
    if (this.nowMonoMs() - this.sliceStartMonoMs < FLUSH_INTERVAL_MS) {
      return
    }
    this.flush('heartbeat')
  }

  close(reason: string): void {
    if (!this.active) {
      return
    }
    this.flush(reason)
    this.active = false
    this.sessionId = ''
    this.starMapId = ''
    this.projectId = ''
  }
}
