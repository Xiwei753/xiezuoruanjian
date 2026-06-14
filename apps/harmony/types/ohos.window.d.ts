declare module '@ohos.window' {
  interface LoadContentCallback {
    (err: BusinessError, data: string): void
  }
  interface BusinessError {
    code: number
    message: string
  }
  export interface WindowStage {
    loadContent(url: string, callback: LoadContentCallback): void
  }
}
