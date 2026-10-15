// ShoulderSurfingProvider（standard target）— 普通包防窥能力 Provider。
// 普通构建（default product）不编译、不打包 DLP 实现，明确提供"该能力不包含"的状态。
// 不引用 @kit.DeviceSecurityKit，不注册防窥事件。
// 通过 packageName import 访问 main 下的 Port 接口。
import { common } from '@kit.AbilityKit'
import { ShoulderSurfingPort } from 'entry/ets/platform/privacy/ShoulderSurfingPort'

export class ShoulderSurfingProvider implements ShoulderSurfingPort {
  // 接收 UIAbilityContext（普通包不使用，仅为统一接口签名）。
  setAbilityContext(_context: common.UIAbilityContext): void {
    // 普通包不含防窥能力，忽略 context。
  }

  // 普通包不支持防窥检测。
  isSupported(): boolean {
    return false
  }

  // 不支持，永远返回 false。
  async isEnabled(): Promise<boolean> {
    return false
  }

  // 不支持，请求开启返回 false。
  async requestEnable(): Promise<boolean> {
    return false
  }

  // 不注册监听，返回 false。
  startListening(_onPeeping: (peeping: boolean) => void): boolean {
    return false
  }

  // 空操作。
  stopListening(): void {
  }

  // 无状态，返回 null（未知）。
  getPeepingStatus(): boolean | null {
    return null
  }
}
