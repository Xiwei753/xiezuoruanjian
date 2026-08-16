// ContinuationPayload — 应用接续（Continuation）携带的数据。
// 正文数据仍由作品数据层和同步体系负责，本 payload 只携带当前在写哪一章/选区/视口等接续状态。
//
// Issue #629 评论 5 第 8 节：跨设备内容身份用 contentHash（稳定内容哈希），
// 不用 TextEditSession.revision。Rust TextEditSession::with_text() 每次新建会话 revision
// 都是 EditorRevision::initial()，目标设备重新打开章节后拿到的是新会话 revision，
// 跟源设备 session revision 没有跨设备比较意义。contentHash 来自：
//   - 加载章节时：ChapterContent.meta.hash
//   - 保存成功后：ChapterSaveReceipt.contentHash
// viewport 用强类型 EditorViewportState，不用裸 object。

import { EditorViewportState } from '../../feature/editor/render/EditorViewportState'

export interface ContinuationPayload {
  projectId?: string
  volumeId?: string
  chapterId?: string
  selection?: number[]
  cursor?: number
  viewport?: EditorViewportState
  contentHash?: string  // 跨设备稳定内容身份（来自 ChapterContent.meta.hash / ChapterSaveReceipt.contentHash）
}
