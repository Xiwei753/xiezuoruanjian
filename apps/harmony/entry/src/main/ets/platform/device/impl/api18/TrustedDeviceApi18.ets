// TrustedDeviceApi18 — DistributedServiceKit / AbilityConnection 的 API18+ 适配。
// Issue #988：只使用系统已批准的公开能力：
// - distributedDeviceManager：查询系统可信设备及在线状态（需要 DISTRIBUTED_DATASYNC）；
// - abilityConnectionManager：同应用跨设备 UIAbility 连接，第三方官方保证文本消息。
// 不把华为账号登录态当同账号证明，不传 GitHub token，不假设 sendData 二进制通道可用。

import { abilityConnectionManager, distributedDeviceManager } from '@kit.DistributedServiceKit'
import { abilityAccessCtrl, common, PermissionRequestResult, Permissions } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'
import { TrustedDeviceInfo } from '../../TrustedDeviceService'

const BUNDLE_NAME: string = 'com.xiwei.sujian'
const MODULE_NAME: string = 'entry'
const ABILITY_NAME: string = 'EntryAbility'
const SERVICE_NAME: string = 'SujianPeerSync'
const DOMAIN: number = 0x9880

export class TrustedDeviceApi18 {
  private static instance: TrustedDeviceApi18
  private deviceManager: distributedDeviceManager.DeviceManager | null = null
  private context: common.UIAbilityContext | null = null
  private activeSessions: Set<number> = new Set<number>()
  private messageListener: ((sessionId: number, message: string) => void) | null = null
  private disconnectListener: ((sessionId: number, reason: string) => void) | null = null

  static getInstance(): TrustedDeviceApi18 {
    if (!TrustedDeviceApi18.instance) {
      TrustedDeviceApi18.instance = new TrustedDeviceApi18()
    }
    return TrustedDeviceApi18.instance
  }

  isSupported(): boolean {
    const resolver = PlatformApiResolver.getInstance()
    return resolver.isApiAtLeast(18)
      && resolver.hasSystemCapability('SystemCapability.DistributedSched.AppCollaboration')
      && resolver.hasSystemCapability('SystemCapability.DistributedHardware.DeviceManager')
  }

  setContext(context: common.UIAbilityContext): void {
    this.context = context
  }

  setObservers(
    onMessage: (sessionId: number, message: string) => void,
    onDisconnect: (sessionId: number, reason: string) => void
  ): void {
    this.messageListener = onMessage
    this.disconnectListener = onDisconnect
  }

  // DISTRIBUTED_DATASYNC 是 user_grant 权限。拒绝时保持空列表和不可用状态，
  // 不把“没有权限”伪装成“附近没有设备”。
  async requestPermission(context: common.UIAbilityContext): Promise<boolean> {
    if (!this.isSupported()) {
      return false
    }
    try {
      const atManager: abilityAccessCtrl.AtManager = abilityAccessCtrl.createAtManager()
      const permissions: Array<Permissions> = ['ohos.permission.DISTRIBUTED_DATASYNC']
      const result: PermissionRequestResult =
        await atManager.requestPermissionsFromUser(context, permissions)
      return result.authResults.length > 0 && result.authResults[0] === 0
    } catch (err) {
      hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'requestPermission failed: %{public}s', JSON.stringify(err))
      return false
    }
  }

  // 只读系统可信设备列表。权限缺失/系统能力不可用时返回空数组，不伪造设备。
  listTrustedDevices(): TrustedDeviceInfo[] {
    if (!this.isSupported()) {
      return []
    }
    try {
      if (this.deviceManager === null) {
        this.deviceManager = distributedDeviceManager.createDeviceManager(BUNDLE_NAME)
      }
      const devices: Array<distributedDeviceManager.DeviceBasicInfo> =
        this.deviceManager.getAvailableDeviceListSync()
      const result: Array<TrustedDeviceInfo> = []
      devices.forEach((device: distributedDeviceManager.DeviceBasicInfo): void => {
        const networkId: string = device.networkId !== undefined ? device.networkId : device.deviceId
        result.push({
          networkId: networkId,
          deviceId: device.deviceId,
          deviceName: device.deviceName,
          deviceType: device.deviceType,
          online: true
        })
      })
      return result
    } catch (err) {
      hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'listTrustedDevices failed: %{public}s', JSON.stringify(err))
      return []
    }
  }

  async connect(device: TrustedDeviceInfo): Promise<number> {
    if (!this.isSupported() || this.context === null) {
      return -1
    }
    try {
      const peerInfo: abilityConnectionManager.PeerInfo = {
        deviceId: device.networkId,
        bundleName: BUNDLE_NAME,
        moduleName: MODULE_NAME,
        abilityName: ABILITY_NAME,
        serviceName: SERVICE_NAME
      }
      const options: abilityConnectionManager.ConnectOptions = {
        needSendData: false,
        parameters: { 'source': 'sujian-peer-sync' }
      }
      const sessionId: number = abilityConnectionManager.createAbilityConnectionSession(
        SERVICE_NAME, this.context, peerInfo, options)
      this.registerSession(sessionId)
      const result: abilityConnectionManager.ConnectResult =
        await abilityConnectionManager.connect(sessionId)
      if (!result.isConnected) {
        hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'connect rejected code=%{public}d', result.errorCode ?? -1)
        this.destroySession(sessionId)
        return -1
      }
      return sessionId
    } catch (err) {
      hilog.error(DOMAIN, 'TrustedDeviceApi18', 'connect failed: %{public}s', JSON.stringify(err))
      return -1
    }
  }

  // 接收端由 EntryAbility.onCollaborate 调用。系统在 want 中附带 token 和呼叫方 PeerInfo。
  acceptCollaboration(wantParam: Record<string, Object>): boolean {
    if (!this.isSupported() || this.context === null) {
      return false
    }
    const tokenValue: Object | undefined = wantParam['ohos.extra.param.key.supportCollaborateIndex']
    const peerValue: Object | undefined =
      wantParam[abilityConnectionManager.CollaborationKeys.PEER_INFO]
    if (tokenValue === undefined || peerValue === undefined) {
      return false
    }
    try {
      const peerInfo: abilityConnectionManager.PeerInfo =
        peerValue as abilityConnectionManager.PeerInfo
      const serviceName: string =
        peerInfo.serviceName !== undefined ? peerInfo.serviceName : SERVICE_NAME
      const sessionId: number = abilityConnectionManager.createAbilityConnectionSession(
        serviceName, this.context, peerInfo, { needSendData: false })
      this.registerSession(sessionId)
      abilityConnectionManager.acceptConnect(sessionId, tokenValue as string).catch((err: Object): void => {
        hilog.error(DOMAIN, 'TrustedDeviceApi18', 'acceptConnect failed: %{public}s', JSON.stringify(err))
        this.destroySession(sessionId)
      })
      return true
    } catch (err) {
      hilog.error(DOMAIN, 'TrustedDeviceApi18', 'acceptCollaboration failed: %{public}s', JSON.stringify(err))
      return false
    }
  }

  async sendMessage(sessionId: number, message: string): Promise<boolean> {
    try {
      await abilityConnectionManager.sendMessage(sessionId, message)
      return true
    } catch (err) {
      hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'sendMessage failed: %{public}s', JSON.stringify(err))
      return false
    }
  }

  disconnect(sessionId: number): void {
    try {
      abilityConnectionManager.disconnect(sessionId)
    } catch (err) {
      hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'disconnect failed: %{public}s', JSON.stringify(err))
    }
    this.destroySession(sessionId)
  }

  private registerSession(sessionId: number): void {
    this.activeSessions.add(sessionId)
    abilityConnectionManager.on('receiveMessage', sessionId,
      (info: abilityConnectionManager.EventCallbackInfo): void => {
        const message: string = info.msg !== undefined ? info.msg : ''
        if (message.length > 0 && this.messageListener !== null) {
          this.messageListener(sessionId, message)
        }
      })
    abilityConnectionManager.on('disconnect', sessionId,
      (info: abilityConnectionManager.EventCallbackInfo): void => {
        this.activeSessions.delete(sessionId)
        const reason: string = info.reason !== undefined ? String(info.reason) : ''
        if (this.disconnectListener !== null) {
          this.disconnectListener(sessionId, reason)
        }
      })
  }

  private destroySession(sessionId: number): void {
    this.activeSessions.delete(sessionId)
    try {
      abilityConnectionManager.off('receiveMessage', sessionId)
      abilityConnectionManager.off('disconnect', sessionId)
      abilityConnectionManager.destroyAbilityConnectionSession(sessionId)
    } catch (err) {
      hilog.warn(DOMAIN, 'TrustedDeviceApi18', 'destroySession failed: %{public}s', JSON.stringify(err))
    }
  }
}
