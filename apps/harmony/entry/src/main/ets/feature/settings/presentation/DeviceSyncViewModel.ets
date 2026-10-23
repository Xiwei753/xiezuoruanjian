// DeviceSyncViewModel — 设置中的“附近设备同步”状态（Issue #988）。
// 只负责系统可信设备发现、连接状态和给用户看的结果；对象清单/LWW/墓碑由 ETS
// PeerObjectStore 按现有 manifest 规则处理，不复制一份云端 Core 状态，
// 也不要求每台设备配置 GitHub 仓库或登录应用内华为账号。
// 云端 GitHub 只是可选转发：任一台有仓库的可信设备可以把合并结果继续同步到云端。

import { getAppContext } from '../../../app/AppContext'
import { TrustedDeviceInfo, TrustedDeviceService } from '../../../platform/device/TrustedDeviceService'
import { NearbyDeviceSyncCoordinator, PeerStatusHandler } from '../../../platform/peer/NearbyDeviceSyncCoordinator'
import { PeerSyncStatus } from '../../../platform/peer/PeerSyncSession'

export type DeviceSyncConnectionState = 'idle' | 'connecting' | 'connected' | 'unsupported' | 'error'

@ObservedV2
export class DeviceSyncViewModel {
  @Trace supported: boolean = false
  @Trace devices: TrustedDeviceInfo[] = []
  @Trace selectedNetworkId: string = ''
  @Trace connectionState: DeviceSyncConnectionState = 'idle'
  @Trace statusMessage: string = '尚未选择附近设备'
  @Trace lastSyncMessage: string = ''
  @Trace isRefreshing: boolean = false
  // null 表示会话尚未协商；true/false 表示对端设备是否配置了可选云备份。
  @Trace remoteCloudRepoAvailable: boolean | null = null

  private activeSessionId: number = -1

  constructor() {
    const coordinator: NearbyDeviceSyncCoordinator = NearbyDeviceSyncCoordinator.getInstance()
    coordinator.attach()
    const handler: PeerStatusHandler = (_sessionId: number, status: PeerSyncStatus): void => {
      this.updatePeerStatus(status)
    }
    coordinator.setStatusHandler(handler)
    this.refresh()
  }

  refresh(): void {
    void this.refreshDevices()
  }

  private async refreshDevices(): Promise<void> {
    this.isRefreshing = true
    this.supported = TrustedDeviceService.getInstance().isSupported()
    if (!this.supported) {
      this.devices = []
      this.connectionState = 'unsupported'
      this.statusMessage = '当前系统不支持附近设备同步'
      this.isRefreshing = false
      return
    }
    const context = getAppContext().getAbilityContext()
    if (context === null) {
      this.devices = []
      this.connectionState = 'error'
      this.statusMessage = '附近设备权限不可用：缺少应用上下文'
      this.isRefreshing = false
      return
    }
    const granted: boolean = await TrustedDeviceService.getInstance().requestPermission(context)
    if (!granted) {
      this.devices = []
      this.connectionState = 'error'
      this.statusMessage = '未获得分布式设备权限；无法发现附近可信设备'
      this.isRefreshing = false
      return
    }
    this.devices = TrustedDeviceService.getInstance().listTrustedDevices()
    if (this.devices.length === 0) {
      this.statusMessage = '未发现系统可信设备；请先在同一分布式网络内完成系统设备连接'
    } else if (this.selectedNetworkId.length === 0) {
      this.statusMessage = '选择一台可信设备开始同步'
    }
    this.isRefreshing = false
  }

  selectDevice(device: TrustedDeviceInfo): void {
    this.selectedNetworkId = device.networkId
    this.connectionState = 'idle'
    this.statusMessage = `已选择 ${device.deviceName}`
    this.lastSyncMessage = ''
    this.remoteCloudRepoAvailable = null
  }

  async connectSelected(): Promise<void> {
    const device: TrustedDeviceInfo | undefined =
      this.devices.find((item: TrustedDeviceInfo): boolean => item.networkId === this.selectedNetworkId)
    const context = getAppContext().getAbilityContext()
    if (device === undefined || context === null) {
      this.connectionState = 'error'
      this.statusMessage = '附近设备连接不可用'
      return
    }
    const granted: boolean = await TrustedDeviceService.getInstance().requestPermission(context)
    if (!granted) {
      this.connectionState = 'error'
      this.statusMessage = '未获得分布式设备权限，无法连接附近设备'
      return
    }
    // 连接前刷新本机云备份能力，hello 时只交换“是否配置”布尔值，不传仓库凭据。
    await NearbyDeviceSyncCoordinator.getInstance().refreshLocalCapabilities()
    this.connectionState = 'connecting'
    this.statusMessage = `正在连接 ${device.deviceName}`
    const sessionId: number =
      await TrustedDeviceService.getInstance().connect(device, context)
    if (sessionId <= 0) {
      this.connectionState = 'error'
      this.statusMessage = '连接失败：请确认两台设备在同一系统网络且应用在前台'
      return
    }
    this.activeSessionId = sessionId
    this.connectionState = 'connected'
    this.statusMessage = `已连接 ${device.deviceName}；正在交换对象清单`
    NearbyDeviceSyncCoordinator.getInstance().startInitiator(sessionId, device)
  }

  disconnect(): void {
    if (this.activeSessionId > 0) {
      NearbyDeviceSyncCoordinator.getInstance().disconnect(this.activeSessionId)
    }
    this.activeSessionId = -1
    this.connectionState = 'idle'
    this.statusMessage = '附近设备连接已断开'
  }

  remoteCloudLabel(): string {
    if (this.remoteCloudRepoAvailable === null) {
      return '对方云备份：连接后显示（不影响附近设备同步）'
    }
    return this.remoteCloudRepoAvailable
      ? '对方已配置可选云备份；合并结果可由对端继续上云'
      : '对方未配置云备份；仍可直接与这台设备同步'
  }

  updatePeerStatus(status: PeerSyncStatus): void {
    this.lastSyncMessage = status.message
    if (status.remoteHasCloudRepo !== undefined) {
      this.remoteCloudRepoAvailable = status.remoteHasCloudRepo
    }
    if (status.state === 'error') {
      this.connectionState = 'error'
    }
  }
}
