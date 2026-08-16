// AppContinuationService — 当前任务接续（HarmonyOS Continuation）。
// save(payload) 序列化 payload 准备给 onContinue 使用；
// getCurrentPayload() 由 EntryAbility.onContinue 调用取当前 payload；
// restoreFromWant(wantParam) 目标设备从 wantParam 恢复 payload。
// 不写成第二套同步：正文数据仍由作品数据层和同步体系负责。
//
// 不用进程内存"伪持久化"。payload 只带 projectId/volumeId/chapterId/selection/viewport/revision；
// 正文由作品仓库/同步负责。
//
// 真实接续流程（Ability Kit）：
// 1. 源设备：用户触发接续 → EntryAbility.onContinue → 调 getCurrentPayload() → 写入 wantParam。
// 2. 目标设备：EntryAbility.onCreate/onNewWant → 检查 launchReason === CONTINUATION →
//    从 want.parameters 读取 → restoreFromWant() 还原 payload。

import { ContinuationPayload } from './ContinuationPayload'

const WANT_PARAM_KEY: string = 'continuationData'

export class AppContinuationService {
  private static instance: AppContinuationService
  // 当前待接续 payload。由 save() 设置，由 onContinue 读取后清空。
  // 这不是"伪持久化"——它是 onContinue 同步读取的临时持有，生命周期与 onContinue 调用绑定。
  private pendingPayload: ContinuationPayload | null = null

  static getInstance(): AppContinuationService {
    if (!AppContinuationService.instance) {
      AppContinuationService.instance = new AppContinuationService()
    }
    return AppContinuationService.instance
  }

  // 序列化 payload 准备给 onContinue 使用。
  // payload 暂存于实例字段，供 EntryAbility.onContinue 调 getCurrentPayload() 取走。
  // 不写第二套持久化；正文由作品仓库/同步负责。
  async save(payload: ContinuationPayload): Promise<void> {
    this.pendingPayload = this.sanitizePayload(payload)
  }

  // EntryAbility.onContinue 调用此方法取当前 payload。
  // 取出后清空 pendingPayload，避免重复接续。
  getCurrentPayload(): ContinuationPayload | null {
    const p: ContinuationPayload | null = this.pendingPayload
    this.pendingPayload = null
    return p
  }

  // 目标设备从 wantParam 恢复 payload。
  // wantParam 是 EntryAbility.onCreate/onNewWant 收到的 want.parameters。
  // 返回还原后的 payload；若 wantParam 不含接续数据返回 null。
  restoreFromWant(wantParam: Record<string, Object>): ContinuationPayload | null {
    const raw: Object | undefined = wantParam[WANT_PARAM_KEY]
    if (raw === undefined || raw === null) {
      return null
    }
    try {
      const json: string = raw as string
      const obj: Record<string, Object> = JSON.parse(json) as Record<string, Object>
      return this.sanitizePayload(this.fromObject(obj))
    } catch (e) {
      // 反序列化失败；不伪造 payload。
      return null
    }
  }

  // 清空待接续 payload（取消接续时调用）。
  clear(): void {
    this.pendingPayload = null
  }

  // —— private ——
  // 只保留接续必需字段；不带正文。
  private sanitizePayload(payload: ContinuationPayload): ContinuationPayload {
    const result: ContinuationPayload = {}
    if (payload.projectId !== undefined) {
      result.projectId = payload.projectId
    }
    if (payload.volumeId !== undefined) {
      result.volumeId = payload.volumeId
    }
    if (payload.chapterId !== undefined) {
      result.chapterId = payload.chapterId
    }
    if (payload.selection !== undefined) {
      result.selection = payload.selection
    }
    if (payload.cursor !== undefined) {
      result.cursor = payload.cursor
    }
    if (payload.viewport !== undefined) {
      result.viewport = payload.viewport
    }
    if (payload.revision !== undefined) {
      result.revision = payload.revision
    }
    return result
  }

  // 从反序列化的对象重建 ContinuationPayload。
  private fromObject(obj: Record<string, Object>): ContinuationPayload {
    const result: ContinuationPayload = {}
    if (obj['projectId'] !== undefined) {
      result.projectId = obj['projectId'] as string
    }
    if (obj['volumeId'] !== undefined) {
      result.volumeId = obj['volumeId'] as string
    }
    if (obj['chapterId'] !== undefined) {
      result.chapterId = obj['chapterId'] as string
    }
    if (obj['selection'] !== undefined) {
      result.selection = obj['selection'] as number[]
    }
    if (obj['cursor'] !== undefined) {
      result.cursor = obj['cursor'] as number
    }
    if (obj['viewport'] !== undefined) {
      result.viewport = obj['viewport'] as object
    }
    if (obj['revision'] !== undefined) {
      result.revision = obj['revision'] as number
    }
    return result
  }

  // 序列化 payload 成 JSON 字符串，供 EntryAbility.onContinue 写入 wantParam。
  serializeForWant(payload: ContinuationPayload): string {
    return JSON.stringify(this.sanitizePayload(payload))
  }
}
