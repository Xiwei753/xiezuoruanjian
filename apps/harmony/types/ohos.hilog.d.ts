declare module '@ohos.hilog' {
  interface HiLog {
    info(domain: number, tag: string, format: string, ...args: object[]): void
    error(domain: number, tag: string, format: string, ...args: object[]): void
    warn(domain: number, tag: string, format: string, ...args: object[]): void
    debug(domain: number, tag: string, format: string, ...args: object[]): void
  }
  const hilog: HiLog
  export default hilog
}
