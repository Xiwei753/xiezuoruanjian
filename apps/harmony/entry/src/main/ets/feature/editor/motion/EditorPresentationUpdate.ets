// EditorPresentationUpdate — 不可变的 UI 消费数据包。
//
// Issue #879：写作区文字运动层的入口数据结构。
// 把 EditorLayoutSnapshot（静态布局快照）和 EditorEditResult（编辑事实）
// 打包成一份不可变数据，供运动协调器（EditorMotionCoordinator）消费。
//
// 初始打开、切换章节、改变字号/宽度时 editResult=null——这些场景没有编辑事务，
// 只有布局重建，运动层应直接显示静态画面，不启动任何动画。
//
// 只引用已有类型，不修改 DTO 与 Rust。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'

/**
 * 不可变的 UI 消费数据包。
 *
 * - layoutSnapshot：当前编辑器的静态布局快照（正文、选区、光标、字号等）。
 * - editResult：触发本次更新的编辑事务结果；null 表示非编辑命令触发的更新
 *   （初始打开、切换章节、改变字号/宽度等）。
 * - sessionId：Core 编辑会话 ID，用于区分不同会话的更新流。
 */
export class EditorPresentationUpdate {
  readonly layoutSnapshot: EditorLayoutSnapshot
  readonly editResult: EditorEditResult | null
  readonly sessionId: number

  constructor(
    layoutSnapshot: EditorLayoutSnapshot,
    editResult: EditorEditResult | null,
    sessionId: number
  ) {
    this.layoutSnapshot = layoutSnapshot
    this.editResult = editResult
    this.sessionId = sessionId
  }

  /**
   * 判断本次更新是否由编辑事务触发（而非初始加载/布局重建）。
   * editResult !== null 且 outcome 为 Applied/AppliedWithAdjustedSelection 时才视为真实编辑。
   */
  hasEditResult(): boolean {
    return this.editResult !== null
  }

  /**
   * 判断本次更新是否为初始加载或布局重建（无编辑事务）。
   */
  isInitialOrLayoutChange(): boolean {
    return this.editResult === null
  }
}
