// ShoulderSurfingPort — 防窥能力端口接口。
// 由各 target（standard/acl）的 ShoulderSurfingProvider 实现。
// main 下的 ShoulderSurfingService 通过此接口操作，不直接接触 DLP Kit。
//
// 本接口放在 src/main 下（defaultSourceRoot），所有 target 的 Provider 都能通过
// packageName import（entry/ets/platform/privacy/ShoulderSurfingPort）访问。
import { common } from '@kit.AbilityKit'

export interface ShoulderSurfingPort {
  // 注入 UIAbilityContext（acl Provider 调 requestAntiPeepOptions 需要；standard 忽略）。
  // 放进 Port 契约，Service 无需向下转成具体 Provider 类。
  setAbilityContext(context: common.UIAbilityContext): void

  // 查询系统是否支持防窥检测。
  isSupported(): boolean

  // 防窥开关是否启用。不支持时返回 false，不伪造。
  isEnabled(): Promise<boolean>

  // 请求打开防窥保护（打开系统设置页）。返回最终开关状态。
  requestEnable(): Promise<boolean>

  // 启动防窥状态监听。返回真实注册结果：只有真正注册成功才返回 true。
  startListening(onPeeping: (peeping: boolean) => void): boolean

  // 停止防窥监听。
  stopListening(): void

  // 当前窥屏状态（三态）：true=被窥视，false=安全，null=未知/查询失败。
  getPeepingStatus(): boolean | null
}
