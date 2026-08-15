// SharePayload — 系统分享携带的数据。

export interface SharePayload {
  kind: string  // 'text' | 'file' | 'image' | ...
  title?: string
  text?: string
  uris?: string[]
}
