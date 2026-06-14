declare module '@ohos.router' {
  interface RouterParams {
    [key: string]: string | number | boolean | object
  }
  interface PushUrlOptions {
    url: string
    params?: RouterParams
  }
  interface Router {
    pushUrl(options: PushUrlOptions): Promise<void>
    back(): void
    getParams(): RouterParams
  }
  const router: Router
  export default router
}
