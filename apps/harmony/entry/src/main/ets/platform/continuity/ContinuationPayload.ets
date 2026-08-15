// ContinuationPayload — 应用接续（Continuation）携带的数据。
// 正文数据仍由作品数据层和同步体系负责，本 payload 只携带当前在写哪一章/选区/视口等接续状态。

export interface ContinuationPayload {
  projectId?: string
  volumeId?: string
  chapterId?: string
  selection?: number[]
  cursor?: number
  viewport?: object
  revision?: number
}
