// EditorStatsQueue.ets — 写作统计上报队列（Issue #829 评论 #5995146525 第 5 项起）。
//
// 这一层只做一件事：
// 把编辑事实组装成强类型 EditorChangeStatsInput，交给 SerialStatsDrain 串行送出。
//
// 平台端只透传编辑事实（cause + contentDelta），source 分类（HumanTyped / Pasted /
// Deleted / Unknown）由 Core 的 record_editor_change_stats 决定，这里不猜也不拼 source 字符串。
// 设备身份（deviceId）同样由 Core 自己读/建：早期版本在页面字段初始化时读
// settings.statsDeviceId，那时 loadSettings() 还没跑，拿到的是 null，于是所有 Harmony 统计
// 都写进了同一个 'unknown' 设备桶，且因为是 readonly 再也改不回来。
//
// 串行调度、drain 粒度、barrier 等待条件都在 stats_drain_loop.ts（纯 .ts，可被 Node 单测
// 直接加载真实代码）。之所以要拆开：ArkTS 不允许 .ts import .ets（编译期报 10605999），
// 调度逻辑留在 .ets 里就没法写回归测试，只能写和生产代码分叉的镜像实现。
//
// 早期版本踩过的坑，保留在这里避免以后又走回去：
// - 只在保存时拿整章 old/new 文本比一次：默认自动保存 30 秒，这 30 秒内真实敲的字不形成
//   任何实时事件；而且 Core 的旧启发式「净增 > 20 就当 paste」会把连续敲 30 个字误判成粘贴。
// - flush 等「整轮队列清空」：保存期间用户继续输入，一次自动保存可能永远等不到。
//   现在保存只 flushThrough(自己的 barrier)。
// - drain 一次吃完整个队列：barrier 早就完成了也还在等后面的 seq（#5997254152 第 2 项）。

import { EditorChangeStatsInput, EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { SerialStatsDrain } from './stats_drain_loop'

/** 队列里一条待写事件：seq 交给调度层算好的，业务字段由本队列持有。 */
interface QueuedEditorChange {
  seq: number
  projectId: string
  volumeId: string
  chapterId: string
  cause: EditorTransactionCause
  insertedChars: number
  deletedChars: number
}

export class EditorStatsQueue {
  private readonly platform: string
  private readonly drain: SerialStatsDrain<QueuedEditorChange>

  constructor(bridge: IWriterCoreBridge, platform: string) {
    this.platform = platform
    this.drain = new SerialStatsDrain<QueuedEditorChange>(
      (item: QueuedEditorChange): Promise<boolean> => {
        const input: EditorChangeStatsInput = {
          platform: this.platform,
          projectId: item.projectId,
          volumeId: item.volumeId,
          chapterId: item.chapterId,
          cause: item.cause,
          insertedChars: item.insertedChars,
          deletedChars: item.deletedChars
        }
        // 这里 await 的 Promise 背后是 Node-API async work：Rust 调用跑在线程池，
        // 磁盘 I/O 不在 ArkUI 主线程上。串行保证同时只有一条在飞。
        return bridge.recordEditorChangeStats(input).then((res) => res.success)
      }
    )
  }

  /**
   * 编辑事务发生后调用。入队是纯内存操作，不跨 NAPI、不 await，
   * 所以 state listener 调它不会阻塞编辑主链。
   */
  enqueueChange(projectId: string, volumeId: string, chapterId: string,
    cause: EditorTransactionCause, insertedChars: number, deletedChars: number): void {
    // 纯光标移动 / 选区变化：Core 那侧也不落盘，这里直接不进队列。
    if (insertedChars <= 0 && deletedChars <= 0) {
      return
    }
    // 统计事件必须带章节上下文，否则 Core 归不到 per_chapter 维度。
    if (chapterId.length === 0) {
      return
    }

    this.drain.enqueue({
      seq: 0, // 占位：入队时由调度层从账本分配并覆盖
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      cause: cause,
      insertedChars: insertedChars,
      deletedChars: deletedChars
    })
  }

  /**
   * 当前入队过的最后一个序号。
   *
   * 保存流程拿它当 barrier：保存这一刻之前入队的统计就是这份正文的统计，
   * 保存后 `flushThrough(barrier)` 只等这些。之后的新输入 seq 更大，与这次保存无关，
   * 不会让这次保存一直等下去。
   */
  currentSeq(): number {
    return this.drain.currentSeq()
  }

  /**
   * 等到 barrier（含）之前入队的统计都已被 Core 处理完。
   * 语义见 SerialStatsDrain.flushThrough。
   */
  async flushThrough(barrier: number): Promise<boolean> {
    return this.drain.flushThrough(barrier)
  }

  /** 因积压上限丢弃过多少条。正常应恒为 0，非 0 说明 Core 侧持续写失败。 */
  dropped(): number {
    return this.drain.dropped()
  }
}