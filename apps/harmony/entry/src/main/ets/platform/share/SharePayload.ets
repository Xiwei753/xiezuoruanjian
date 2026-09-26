// SharePayload — 系统分享携带的数据。
// ShareChannel — 分享通道：'system' 系统分享 / 'tap' 碰一碰 / 'airTransfer' 隔空传送。
//
// 字段语义（Issue #766 收紧）：
//   kind  = 决定主记录类型（'file' → general.file-uri 主记录；'text' → general.text 主记录）
//   title = SharedRecord 元数据标题（写入 record.title，不伪装成独立正文记录）
//   text  = 真正正文内容（写入 record.content）
//   uris  = 真正文件 URI 列表（写入 general.file-uri record）

export type ShareChannel = 'system' | 'tap' | 'airTransfer'

export interface SharePayload {
  kind: string  // 'text' | 'file' | 'image' | ...
  title?: string
  text?: string
  uris?: string[]
  utd?: string
}
