// ShoulderSurfingProvider（acl target）— ACL 包防窥能力 Provider。
// 从 privacy_acl HSP 模块获得真实 DLP 实现，向 ShoulderSurfingPort 接口映射。
// 仅在 acl product 下装配；通过 packageName import 访问 main 下的 Port 接口。
import { common } from '@kit.AbilityKit'
import { ShoulderSurfingPort } from 'entry/ets/platform/privacy/ShoulderSurfingPort'
import { DlpAntiPeepApi20, DlpAntiPeepApi23 } from 'privacy_acl'

export class ShoulderSurfingProvider implements ShoulderSurfingPort {
  private abilityContext: common.UIAbilityContext | null = null

  // 接收 UIAbilityContext，供 requestAntiPeepOptions 打开防窥设置页用。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
  }

  // 查询系统是否支持防窥。委托 DlpAntiPeepApi20 facade 判断 SystemCapability + API Level。
  isSupported(): boolean {
    return DlpAntiPeepApi20.getInstance().isSupported()
  }

  // 防窥开关是否启用。委托 DlpAntiPeepApi20.isSwitchOn()。
  async isEnabled(): Promise<boolean> {
    return await DlpAntiPeepApi20.getInstance().isSwitchOn()
  }

  // 请求打开防窥保护（打开系统设置页）。
  // API>=23 且有 context 时调 DlpAntiPeepApi23.requestAntiPeepOptions(context)；
  // 请求完成后重新调 DlpAntiPeepApi20.isSwitchOn() 确认真实开关。
  // 不支持/无 context 时返回当前真实开关状态，不伪造。
  async requestEnable(): Promise<boolean> {
    if (!this.isSupported()) {
      return false
    }
    const context: common.UIAbilityContext | null = this.abilityContext
    if (context === null || !DlpAntiPeepApi23.getInstance().isSupported()) {
      // 未注入 context 或 API23 防窥设置页能力不可用；返回当前真实开关。
      return await DlpAntiPeepApi20.getInstance().isSwitchOn()
    }
    try {
      await DlpAntiPeepApi23.getInstance().requestAntiPeepOptions(context)
      return await DlpAntiPeepApi20.getInstance().isSwitchOn()
    } catch (e) {
      // 系统调用失败；重新查真实开关，不伪造。
      return await DlpAntiPeepApi20.getInstance().isSwitchOn()
    }
  }

  // 启动防窥状态监听。委托 DlpAntiPeepApi20.startListening。
  startListening(onPeeping: (peeping: boolean) => void): boolean {
    return DlpAntiPeepApi20.getInstance().startListening(onPeeping)
  }

  // 停止防窥监听。委托 DlpAntiPeepApi20.stopListening。
  stopListening(): void {
    DlpAntiPeepApi20.getInstance().stopListening()
  }

  // 当前窥屏状态（三态）。委托 DlpAntiPeepApi20.getPeepingStatus。
  getPeepingStatus(): boolean | null {
    return DlpAntiPeepApi20.getInstance().getPeepingStatus()
  }
}
