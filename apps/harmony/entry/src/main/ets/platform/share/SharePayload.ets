// SharePayload — 系统分享携带的数据。
// ShareChannel — 分享通道：'system' 系统分享 / 'tap' 碰一碰 / 'airTransfer' 隔空传送。

export type ShareChannel = 'system' | 'tap' | 'airTransfer'

export interface SharePayload {
  kind: string  // 'text' | 'file' | 'image' | ...
  title?: string
  text?: string
  uris?: string[]
}
