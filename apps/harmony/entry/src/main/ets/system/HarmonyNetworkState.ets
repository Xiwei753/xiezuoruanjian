// HarmonyOS NEXT Network — 包装 @ohos.net.connection（API 12）。
// NativeNetworkManager 内部应 import connection from '@ohos.net.connection'。
// 接口签名对齐 connection.getDefaultNet() / connection.on('netAvailable')。

export type NetBearType = 'cellular' | 'wifi' | 'ethernet' | 'none'

export interface NetStatusInfo {
  bearType: NetBearType
  isAvailable: boolean
  isMetered: boolean
}

export interface NetworkManager {
  getCurrentStatus(): Promise<NetStatusInfo>
  isConnected(): Promise<boolean>

  onStatusChange(callback: (info: NetStatusInfo) => void): void
  offStatusChange(callback: (info: NetStatusInfo) => void): void
}

export class MockNetworkManager implements NetworkManager {
  private listeners: Set<(info: NetStatusInfo) => void> = new Set()
  private current: NetStatusInfo = {
    bearType: 'wifi',
    isAvailable: true,
    isMetered: false
  }

  async getCurrentStatus(): Promise<NetStatusInfo> {
    const info: NetStatusInfo = {
      bearType: this.current.bearType,
      isAvailable: this.current.isAvailable,
      isMetered: this.current.isMetered
    }
    return info
  }

  async isConnected(): Promise<boolean> {
    return this.current.isAvailable
  }

  onStatusChange(callback: (info: NetStatusInfo) => void): void {
    this.listeners.add(callback)
  }

  offStatusChange(callback: (info: NetStatusInfo) => void): void {
    this.listeners.delete(callback)
  }

  simulateOffline(): void {
    this.current = { bearType: 'none', isAvailable: false, isMetered: false }
    this.listeners.forEach(cb => cb(this.current))
  }

  simulateOnline(): void {
    this.current = { bearType: 'wifi', isAvailable: true, isMetered: false }
    this.listeners.forEach(cb => cb(this.current))
  }
}

export class NativeNetworkManager implements NetworkManager {
  // TODO: 用 connection.getDefaultNet() + connection.on('netAvailable') / 'netUnavailable' 实现。
  // import connection from '@ohos.net.connection'

  async getCurrentStatus(): Promise<NetStatusInfo> {
    throw new Error('NativeNetworkManager not implemented — 需对接 @ohos.net.connection')
  }

  async isConnected(): Promise<boolean> {
    throw new Error('NativeNetworkManager not implemented')
  }

  onStatusChange(_callback: (info: NetStatusInfo) => void): void {
    throw new Error('NativeNetworkManager not implemented — 需对接 connection.on()')
  }

  offStatusChange(_callback: (info: NetStatusInfo) => void): void {
    throw new Error('NativeNetworkManager not implemented — 需对接 connection.off()')
  }
}
