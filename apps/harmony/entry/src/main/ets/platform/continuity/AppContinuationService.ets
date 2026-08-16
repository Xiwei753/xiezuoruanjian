// AppContinuationService — 当前任务接续（HarmonyOS Continuation）。
// updateCurrentState(payload) 由 Navigation/编辑会话状态变化时调用，更新当前可接续任务状态；
// save(payload) 保留为别名（兼容旧调用点）。
// getCurrentPayload() 由 EntryAbility.onContinue 调用取当前 payload——读取不清空，
// pendingPayload 保存当前可接续任务状态，由 updateCurrentState 覆写。
// restoreFromWant(wantParam) 目标设备从 wantParam 恢复 payload。
// 不写成第二套同步：正文数据仍由作品数据层和同步体系负责。
//
// 不用进程内存"伪持久化"。payload 只带 projectId/volumeId/chapterId/selection/cursor/viewport/contentHash；
// 正文由作品仓库/同步负责。
//
// 真实接续流程（Ability Kit）：
// 1. 源设备：用户触发接续 → EntryAbility.onContinue → 调 getCurrentPayload() → 写入 wantParam。
// 2. 目标设备：EntryAbility.onCreate/onNewWant → 检查 launchReason === CONTINUATION →
//    从 want.parameters 读取 → restoreFromWant() 还原 payload → 交 AppNavigation 手动重建 NavPathStack。

import { ContinuationPayload } from './ContinuationPayload'
import { EditorViewportState } from '../../feature/editor/render/EditorViewportState'

const WANT_PARAM_KEY: string = 'continuationData'

export class AppContinuationService {
  private static instance: AppContinuationService
  // 当前可接续任务 payload。由 updateCurrentState()/save() 设置，由 onContinue 读取。
  // 读取不清空：pendingPayload 保存当前可接续任务状态，onContinue 读取不消费；
  // 状态变化时由 updateCurrentState() 覆写。这不是"伪持久化"——它只暂存接续元数据，
  // 正文由作品仓库/同步负责。
  private pendingPayload: ContinuationPayload | null = null

  static getInstance(): AppContinuationService {
    if (!AppContinuationService.instance) {
      AppContinuationService.instance = new AppContinuationService()
    }
    return AppContinuationService.instance
  }

  // 由 Navigation/编辑会话状态变化时调用，更新当前可接续任务状态。
  // 替代旧 save 的"写入即接续"语义：状态变化即覆写 pendingPayload，
  // onContinue 读取时拿到的是最近一次状态快照。
  updateCurrentState(payload: ContinuationPayload): void {
    this.pendingPayload = this.sanitizePayload(payload)
  }

  // 序列化 payload 准备给 onContinue 使用（updateCurrentState 的别名，兼容旧调用点）。
  // payload 暂存于实例字段，供 EntryAbility.onContinue 调 getCurrentPayload() 读取。
  // 不写第二套持久化；正文由作品仓库/同步负责。
  async save(payload: ContinuationPayload): Promise<void> {
    this.updateCurrentState(payload)
  }

  // EntryAbility.onContinue 调用此方法取当前 payload。
  // 读取不清空：pendingPayload 保存当前可接续任务状态，由 updateCurrentState 覆写；
  // onContinue 多次调用拿到同一份最近状态快照，避免重复接续时丢状态。
  getCurrentPayload(): ContinuationPayload | null {
    return this.pendingPayload
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
    if (payload.contentHash !== undefined) {
      result.contentHash = payload.contentHash
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
      result.viewport = EditorViewportState.fromObject(obj['viewport'] as Record<string, Object>)
    }
    if (obj['contentHash'] !== undefined) {
      result.contentHash = obj['contentHash'] as string
    }
    return result
  }

  // 序列化 payload 成 JSON 字符串，供 EntryAbility.onContinue 写入 wantParam。
  serializeForWant(payload: ContinuationPayload): string {
    return JSON.stringify(this.sanitizePayload(payload))
  }
}
