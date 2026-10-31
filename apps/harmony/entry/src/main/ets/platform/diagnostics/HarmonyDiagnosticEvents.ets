// HarmonyDiagnosticEvents — HarmonyOS 结构化诊断事件的唯一入口（Issue #906）。
//
// 职责边界（务必保持）：
// - 只做「ArkTS → NAPI → Rust」的单向转发，把事件整理成 Rust DTO 约定的 camelCase JSON。
// - 不做第二套日志后端：不持有内存队列、不写文件、不做轮转（这些都在 Rust `writer_diagnostics`）。
// - 不做第二套脱敏：字段值原样透传给 Rust，脱敏由 `writer_diagnostics` 统一处理。
// - 不复制 enabled/verbose 状态：是否启用、是否 verbose 由 Core 单一事实来源决定；
//   Rust 侧在诊断未初始化/已关闭时静默 no-op，本层不缓存任何开关。
//
// 事件金字塔：只上报低频、稳定、可检索的业务/生命周期事件。以下高频事件
// 严禁调用本入口（会让诊断文件膨胀且无检索价值）：
// - onTouch(Move) / 指针移动 / 滚轮与滚动手势；
// - 滚动偏移、动画帧、逐帧布局/渲染回调；
// - 输入法每次按键、preedit 更新等编辑器高频回调。
// 这些高频数据应留在平台侧采样/聚合，不能逐条进结构化事件通道。

import hilog from '@ohos.hilog'
import { NativeDiagnosticsBridge } from '../../corebridge/native/NativeDiagnosticsBridge'
import { NativeCoreModule } from '../../corebridge/native/NativeCoreModule'

/**
 * 诊断日志级别 — 与 Rust `DiagnosticLevelDto` 的变体名一一对应。
 */
export type DiagnosticLevel = 'Error' | 'Warn' | 'Info' | 'Debug' | 'Trace'

/**
 * 事件来源 — 与 Rust `DiagnosticOriginDto` 的变体名一一对应。
 *
 * - `User`：用户主动操作（点击、选择、输入、手动触发同步）
 * - `System`：系统回调/环境变化（生命周期、网络变化、系统主题变化、崩溃）
 * - `App`：应用自身生命周期/内部状态（设置保存、主题解析、自动同步）
 */
export type DiagnosticOrigin = 'User' | 'System' | 'App'

/**
 * 诊断事件动态字段 — 与 Rust `DiagnosticFieldDto` 对齐。
 *
 * 值必须是字符串；非字符串值由调用方先 `String(...)` 转换后再传入。
 */
export interface DiagnosticField {
  key: string
  value: string
}

/**
 * 事件 payload — 与 Rust `HarmonyDiagnosticEventInput` 的 camelCase 字段对齐。
 *
 * `message` 省略时不参与序列化（不产出 `"message":null`）。
 */
interface DiagnosticEventPayload {
  level: DiagnosticLevel
  origin: DiagnosticOrigin
  event: string
  target: string
  message?: string
  fields: DiagnosticField[]
}

// HiLog 输出参数 — 与仓库其它诊断代码保持一致：domain 0x0000 + 稳定 TAG。
const LOG_DOMAIN: number = 0x0000
const TAG: string = 'HarmonyDiagEvents'

export class HarmonyDiagnosticEvents {
  // 进程内唯一的诊断 bridge。`libwriter_core.so` 本身是全局单例，
  // 这里只做一次懒初始化，避免每个事件都重新 `new` 一套 bridge。
  private static bridge: NativeDiagnosticsBridge | null = null

  // 懒获取诊断 bridge：首次调用时构造，之后复用同一实例。
  private static getBridge(): NativeDiagnosticsBridge {
    if (HarmonyDiagnosticEvents.bridge === null) {
      // NativeCoreModule 只是 NAPI 入口的薄封装，创建实例安全（都指向同一个 .so）。
      HarmonyDiagnosticEvents.bridge = new NativeDiagnosticsBridge(new NativeCoreModule())
    }
    return HarmonyDiagnosticEvents.bridge
  }

  /**
   * 上报一条结构化诊断事件 —— 全应用唯一的诊断事件入口。
   *
   * 本方法只构造参数并转发给 Rust：脱敏、序号/会话、JSONL 持久化与轮转都由
   * Rust `writer_diagnostics` 负责。可在应用启动极早期（诊断后端尚未初始化）
   * 调用：本方法永不抛异常，native 报错（非零返回码或异常）只在 HiLog 留下可见错误，
   * 不阻塞主流程；Rust 在未初始化/关闭时 no-op。
   *
   * 调用方必须只记录事件类型、长度、标识、状态码等低频元数据；
   * 严禁记录正文、preedit 内容、token、密钥或 URL 等敏感/高频数据。
   *
   * @param level 日志级别，取值与 Rust DTO 变体名一致
   * @param origin 事件来源
   * @param event 稳定、可检索的事件名（如 `ability.created`）
   * @param target 模块名（如 `EntryAbility`）
   * @param fields 动态字段；低频、短小，禁止放入正文/URL/密钥
   * @param message 可选的人类可读消息
   */
  static record(
    level: DiagnosticLevel,
    origin: DiagnosticOrigin,
    event: string,
    target: string,
    fields: DiagnosticField[],
    message?: string
  ): void {
    // 仅在 message 提供时才写入，避免序列化出 `"message":null`；
    // Rust 侧虽然是 Option，但省略更贴近其 JSONL 约定。
    let payload: DiagnosticEventPayload
    if (message === undefined) {
      payload = { level: level, origin: origin, event: event, target: target, fields: fields }
    } else {
      payload = {
        level: level,
        origin: origin,
        event: event,
        target: target,
        message: message,
        fields: fields
      }
    }

    // 事件持久化与归档是两条独立链路：这里只写入 Rust 诊断日志，不再隐式
    // 触发 checkpoint / ZIP。归档只由显式导出、真实故障收尾和后台收尾发起。
    try {
      const bridge: NativeDiagnosticsBridge = HarmonyDiagnosticEvents.getBridge()
      const payloadJson: string = JSON.stringify(payload)
      const status: number = bridge.recordDiagnosticEvent(payloadJson)
      if (status !== 0) {
        // Issue #911：非零返回码（-1/-2/-3/-4）此前被丢弃，事件失败无痕。
        // 这里只记事件名/错误码，不写 payload、正文或敏感字段；HilogCallback 只写内存，
        // 不调用 HiLog，因此不会形成递归。status 0 只表示调用被接受，不代表已落盘。
        hilog.error(LOG_DOMAIN, TAG,
          'structured event rejected: event=%{public}s status=%{public}d',
          event, status)
        return
      }
    } catch (_) {
      // native 未加载/未初始化时抛异常也要留下可见错误，否则 C ABI 失败无从诊断。
      hilog.error(LOG_DOMAIN, TAG,
        'structured event bridge exception: event=%{public}s', event)
      return
    }

    // HiLog 仅作为镜像，不是保存通道；失败忽略。默认只镜像 INFO 及以上。
    HarmonyDiagnosticEvents.mirrorToHilog(level, origin, event, target, fields.length)
  }

  // 把 INFO 及以上事件镜像到 DevEco HiLog，便于开发期实时查看。
  // Debug/Trace 默认不镜像（避免噪声），需要时由 Rust verbose 开关决定持久化。
  // 先 `isLoggable` 过滤再调用，保持开销最小；只写事件名/target/origin/字段数量，
  // 不写 message 和字段值，防止 HiLog 变成第二个可能泄漏数据的旁路。
  private static mirrorToHilog(
    level: DiagnosticLevel,
    origin: DiagnosticOrigin,
    event: string,
    target: string,
    fieldCount: number
  ): void {
    try {
      switch (level) {
        case 'Error':
          if (hilog.isLoggable(LOG_DOMAIN, TAG, hilog.LogLevel.ERROR)) {
            hilog.error(LOG_DOMAIN, TAG,
              'event=%{public}s target=%{public}s origin=%{public}s fields=%{public}d',
              event, target, origin, fieldCount)
          }
          break
        case 'Warn':
          if (hilog.isLoggable(LOG_DOMAIN, TAG, hilog.LogLevel.WARN)) {
            hilog.warn(LOG_DOMAIN, TAG,
              'event=%{public}s target=%{public}s origin=%{public}s fields=%{public}d',
              event, target, origin, fieldCount)
          }
          break
        case 'Info':
          if (hilog.isLoggable(LOG_DOMAIN, TAG, hilog.LogLevel.INFO)) {
            hilog.info(LOG_DOMAIN, TAG,
              'event=%{public}s target=%{public}s origin=%{public}s fields=%{public}d',
              event, target, origin, fieldCount)
          }
          break
        default:
          // Debug/Trace 不默认镜像；Rust 侧仍会按 verbose 开关决定是否持久化。
          break
      }
    } catch (_) {
      // 镜像失败不影响主流程。
    }
  }
}
