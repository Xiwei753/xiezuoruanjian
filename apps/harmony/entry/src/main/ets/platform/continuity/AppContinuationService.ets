// AppContinuationService — 当前任务接续（HarmonyOS Continuation）。
// save(payload) 在适当时机持久化接续状态；restore() 在新设备/恢复时读回。
// 不写成第二套同步：正文数据仍由作品数据层和同步体系负责。

import { ContinuationPayload } from './ContinuationPayload'

export class AppContinuationService {
  private static instance: AppContinuationService
  private current: ContinuationPayload | null = null

  static getInstance(): AppContinuationService {
    if (!AppContinuationService.instance) {
      AppContinuationService.instance = new AppContinuationService()
    }
    return AppContinuationService.instance
  }

  async save(payload: ContinuationPayload): Promise<void> {
    this.current = payload
    // 实际持久化由 HarmonyOS ContinuationManager / DistributedDataStore 完成。
  }

  async restore(): Promise<ContinuationPayload | null> {
    return this.current
  }

  clear(): void {
    this.current = null
  }
}
