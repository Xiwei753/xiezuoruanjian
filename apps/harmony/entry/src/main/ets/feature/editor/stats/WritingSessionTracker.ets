// WritingSessionTracker.ets — Issue #981 真实写作会话计时。
//
// 平台端只负责两类事实：窗口/页面是否仍可写，以及哪一次编辑事务是真人输入。
// 计时状态机与 Core 的 WritingSessionClock 对齐：
//   - 第一次真人编辑才开始计时（粘贴 / AI / 撤销重做不算）；
//   - 前台写作期间没有输入也继续计时，思考时间不被“5 分钟无输入”扣掉；
//   - 退后台、失焦、离开页面、手动暂停立即结算当前片段；
//   - 回到前台不补算离开期间，下一次真人编辑或显式继续才开启新片段。
// 这里不做速度折算；片段交给 Core 持久化，字/时由 Core 统一计算。

import { systemDateTime } from '@kit.BasicServicesKit'
import { util } from '@kit.ArkTS'
import { WritingActivitySegmentInput } from '../../../corebridge/dto/StatsDtos'
import { EditorStatsQueue } from './EditorStatsQueue'

const FLUSH_INTERVAL_MS: number = 30000

enum TrackerState {
  Idle = 0,
  Writing = 1,
  Paused = 2
}

export class WritingSessionTracker {
  private readonly queue: EditorStatsQueue
  private readonly platform: string
  private state: TrackerState = TrackerState.Idle
  private foreground: boolean = false
  private sessionId: string = ''
  private projectId: string = ''
  private volumeId: string = ''
  private chapterId: string = ''
  private monotonicAnchorMs: number = 0
  private wallAnchorMs: number = 0
  private sliceStartMonoMs: number = 0

  constructor(queue: EditorStatsQueue, platform: string) {
    this.queue = queue
    this.platform = platform
  }

  private nowMonoMs(): number {
    return systemDateTime.getUptime(systemDateTime.TimeType.ACTIVE, false)
  }

  private beginSlice(monoMs: number): void {
    this.sliceStartMonoMs = monoMs
  }

  /** 当前真实写作会话 ID；未开始计时时为空。 */
  currentSessionId(): string {
    return this.sessionId
  }

  /** 真人编辑事务入口。章节切换时先结算旧章节片段，再从当前时刻开新片段。 */
  onHumanEdit(projectId: string, volumeId: string, chapterId: string, occurredAtMs: number): void {
    // 后台/失焦后迟到的 IME 提交只能进入字数事实，绝不能重新开启计时。
    if (!this.foreground) {
      return
    }
    if (chapterId.length === 0) {
      return
    }
    const nowMono: number = this.nowMonoMs()
    if (this.state === TrackerState.Writing) {
      if (this.chapterId !== chapterId || this.projectId !== projectId || this.volumeId !== volumeId) {
        this.flush(nowMono, 'chapterSwitch')
        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        this.beginSlice(nowMono)
      }
      return
    }

    this.projectId = projectId
    this.volumeId = volumeId
    this.chapterId = chapterId
    if (this.state === TrackerState.Idle) {
      this.sessionId = util.generateRandomUUID(false)
      this.wallAnchorMs = occurredAtMs
      this.monotonicAnchorMs = nowMono
    }
    // 从暂停恢复：不把离开期间补进来，从这次编辑重新起算。
    this.beginSlice(nowMono)
    this.state = TrackerState.Writing
  }

  /** 前台写作中每约 30 秒结算一次已完成片段，避免异常退出丢失过多。 */
  heartbeat(): void {
    if (this.state !== TrackerState.Writing) {
      return
    }
    const nowMono: number = this.nowMonoMs()
    if (nowMono - this.sliceStartMonoMs < FLUSH_INTERVAL_MS) {
      return
    }
    this.flush(nowMono, 'heartbeat')
    this.beginSlice(nowMono)
  }

  /** 真实离开前台 / 失焦 / 离开正文页：立即结算并暂停。重复调用不重复计。 */
  pause(reason: string): void {
    this.foreground = false
    if (this.state !== TrackerState.Writing) {
      if (this.state === TrackerState.Idle) {
        return
      }
      this.state = TrackerState.Paused
      return
    }
    this.flush(this.nowMonoMs(), reason)
    this.state = TrackerState.Paused
  }

  /** 回到前台只解除“已离开”状态；不补算离开期间，等下一次真人编辑。 */
  resume(): void {
    // 状态保持 Paused：下一次真人编辑才重新开启片段。
    this.foreground = true
  }

  /** 关闭正文 / 退出应用：结算最后不足 30 秒的片段并结束本次会话。 */
  close(reason: string): void {
    this.foreground = false
    if (this.state === TrackerState.Writing) {
      this.flush(this.nowMonoMs(), reason)
    }
    this.state = TrackerState.Idle
    this.sessionId = ''
    this.chapterId = ''
    this.projectId = ''
    this.volumeId = ''
  }

  private flush(monoMs: number, reason: string): void {
    const durationMs: number = monoMs - this.sliceStartMonoMs
    if (durationMs <= 0 || this.sessionId.length === 0 || this.chapterId.length === 0) {
      return
    }
    const startAtMs: number = this.wallAnchorMs + (this.sliceStartMonoMs - this.monotonicAnchorMs)
    const endAtMs: number = this.wallAnchorMs + (monoMs - this.monotonicAnchorMs)
    const input: WritingActivitySegmentInput = {
      eventId: util.generateRandomUUID(false),
      platform: this.platform,
      projectId: this.projectId,
      volumeId: this.volumeId,
      chapterId: this.chapterId,
      sessionId: this.sessionId,
      startAtMs: startAtMs,
      endAtMs: endAtMs,
      pauseReason: reason
    }
    // Issue #981 复核：片段只交给编辑字数事件共用的串行队列，由它统一保持顺序、
    // 固定 eventId 并在失败时重发；本类不再维护第二条 submitChain。
    this.queue.enqueueActivitySegment(input)
  }
}
