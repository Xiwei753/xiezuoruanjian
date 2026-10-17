// HarmonyBackgroundSaveTask — 退后台保存的短时任务包装（Issue #927）。
//
// 背景：AutoSaveLifecycleObserver.onBackground() 之前直接调用保存回调并丢弃 Promise，
// 不向系统申请短时任务；快速切后台/锁屏时，异步正文落盘可能在完成前被系统挂起。
//
// 本类包装官方 @kit.BackgroundTasksKit 的 backgroundTaskManager：
// - requestSuspendDelay(reason, onTimeout)：申请短时任务（API 9+，无需权限）；
// - cancelSuspendDelay(requestId)：一次保存事务结束后释放。
//
// 边界（Issue #927 复核）：
// - 短时任务只是延长后台执行机会，不是正文保存的许可证：申请不到短时任务时
//   只记录真实原因，保存 action 照常执行，结果以 action 的真实返回值为准。
// - 申请到 requestId 就在 try/finally 中释放；申请失败时不做任何伪装成功/失败，
//   也不抛出未捕获异常。
// - UI 页面不直接使用平台后台任务接口，统一从 runSave() 进入。
// - 短时任务只用于完成正在进行的落盘，不用于常驻同步或持续运行。

import { backgroundTaskManager } from '@kit.BackgroundTasksKit'
import { BusinessError } from '@kit.BasicServicesKit'
import hilog from '@ohos.hilog'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../diagnostics/HarmonyDiagnosticEvents'

const LOG_DOMAIN: number = 0x0000
const TAG: string = 'HarmonyBgSaveTask'

// 短时任务接口的 SystemCapability（本机 SDK d.ts @syscap，since 9）。
const TRANSIENT_TASK_SYSCAP: string = 'SystemCapability.ResourceSchedule.BackgroundTaskManager.TransientTask'

// 申请理由（系统侧可见；只描述用途，不含正文/书名等敏感信息）。
const TRANSIENT_TASK_REASON: string = 'save pending writing content'

export class HarmonyBackgroundSaveTask {
  private static instance: HarmonyBackgroundSaveTask

  static getInstance(): HarmonyBackgroundSaveTask {
    if (!HarmonyBackgroundSaveTask.instance) {
      HarmonyBackgroundSaveTask.instance = new HarmonyBackgroundSaveTask()
    }
    return HarmonyBackgroundSaveTask.instance
  }

  // 平台是否提供短时任务能力（canIUse SystemCapability）。
  // 上层据此记录「平台不支持」诊断；短时任务不可用只影响后台执行机会，
  // 不代表保存可以跳过，也不能因此把保存结果假报成功（真正的结果来自 action）。
  isSupported(): boolean {
    return PlatformApiResolver.getInstance().hasSystemCapability(TRANSIENT_TASK_SYSCAP)
  }

  // 在短时任务保护下执行一次保存事务。
  // 申请到 requestId 就用 try/finally 在结束时释放；申请失败/平台不支持只记录
  // 真实原因，保存 action 照常执行——短时任务申请失败与实际正文保存失败是两件事，
  // 不能因为申请不到就放弃还有执行机会的落盘（Issue #927 复核）。
  // action 的真实结果原样返回。
  async runSave(action: () => Promise<boolean>): Promise<boolean> {
    let requestId: number = -1
    if (!this.isSupported()) {
      this.recordUnavailable('syscap_missing', -1)
    } else {
      try {
        const info = backgroundTaskManager.requestSuspendDelay(TRANSIENT_TASK_REASON, () => {
          // 系统约在短时任务超时前 6 秒回调：此时保存还没完成，留下明确诊断。
          this.recordTimeout(requestId)
        })
        requestId = info.requestId
      } catch (err) {
        const error = err as BusinessError
        hilog.error(LOG_DOMAIN, TAG, 'requestSuspendDelay failed: code=%{public}d message=%{public}s',
          error.code, error.message)
        this.recordUnavailable('request_failed', error.code)
      }
    }

    try {
      return await action()
    } finally {
      if (requestId !== -1) {
        this.cancelRequest(requestId)
      }
    }
  }

  // 释放短时任务；释放失败只记日志，不改变保存事务的真实结果（超时后由系统回收）。
  private cancelRequest(requestId: number): void {
    try {
      backgroundTaskManager.cancelSuspendDelay(requestId)
    } catch (err) {
      const error = err as BusinessError
      hilog.error(LOG_DOMAIN, TAG, 'cancelSuspendDelay failed: id=%{public}d code=%{public}d',
        requestId, error.code)
    }
  }

  private recordTimeout(requestId: number): void {
    const field: DiagnosticField = { key: 'requestId', value: String(requestId) }
    HarmonyDiagnosticEvents.record('Warn', 'System', 'background_save_timeout', TAG, [field])
  }

  private recordUnavailable(reason: string, errorCode: number): void {
    const reasonField: DiagnosticField = { key: 'reason', value: reason }
    if (errorCode < 0) {
      HarmonyDiagnosticEvents.record('Warn', 'System', 'background_save_unavailable', TAG, [reasonField])
      return
    }
    const codeField: DiagnosticField = { key: 'errorCode', value: String(errorCode) }
    HarmonyDiagnosticEvents.record('Error', 'System', 'background_save_unavailable', TAG, [reasonField, codeField])
  }
}
