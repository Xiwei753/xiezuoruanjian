// ⚠️ UNVERIFIED — 本接口未经 DevEco 编译验证。
// 方法签名基于 HarmonyOS NEXT 文档推测，必须用 DevEco 真编译校验。

export type NetworkType = 'wifi' | 'cellular' | 'ethernet' | 'none' | 'unknown'
export type ConnectionStatus = 'connected' | 'disconnected' | 'connecting' | 'unknown'

export interface NetworkState {
  type: NetworkType
  status: ConnectionStatus
  isMetered: boolean
  isAvailable: boolean
  strength: number // 0-100
  ssid?: string
  ipAddress?: string
}

export interface ProxyConfig {
  enabled: boolean
  host: string
  port: number
  username?: string
  password?: string
  noProxy: string[]
  protocol: 'http' | 'https' | 'socks5'
}

export interface NetworkManager {
  // State queries
  getCurrentState(): Promise<NetworkState>
  isConnected(): Promise<boolean>
  getConnectionType(): Promise<NetworkType>

  // Monitoring
  onStateChange(callback: (state: NetworkState) => void): void
  offStateChange(callback: (state: NetworkState) => void): void

  // Proxy
  getProxyConfig(): Promise<ProxyConfig>
  setProxyConfig(config: ProxyConfig): Promise<boolean>
  clearProxyConfig(): Promise<boolean>

  // Diagnostics
  getNetworkDiagnostics(): Promise<NetworkDiagnostics>
  testConnectivity(url: string): Promise<ConnectivityTestResult>
}

export interface NetworkDiagnostics {
  dnsResolution: boolean
  internetReachable: boolean
  latencyMs: number
  bandwidthKbps: number
  lastChecked: number
}

export interface ConnectivityTestResult {
  success: boolean
  url: string
  statusCode?: number
  latencyMs: number
  error?: string
}

export class MockNetworkManager implements NetworkManager {
  private listeners: Set<(state: NetworkState) => void> = new Set()
  private currentState: NetworkState = {
    type: 'wifi',
    status: 'connected',
    isMetered: false,
    isAvailable: true,
    strength: 85,
    ssid: 'MockWiFi',
    ipAddress: '192.168.1.100'
  }

  async getCurrentState(): Promise<NetworkState> {
    return { ...this.currentState }
  }

  async isConnected(): Promise<boolean> {
    return this.currentState.status === 'connected'
  }

  async getConnectionType(): Promise<NetworkType> {
    return this.currentState.type
  }

  onStateChange(callback: (state: NetworkState) => void): void {
    this.listeners.add(callback)
  }

  offStateChange(callback: (state: NetworkState) => void): void {
    this.listeners.delete(callback)
  }

  async getProxyConfig(): Promise<ProxyConfig> {
    return {
      enabled: false,
      host: '',
      port: 0,
      noProxy: [],
      protocol: 'http'
    }
  }

  async setProxyConfig(config: ProxyConfig): Promise<boolean> {
    return true
  }

  async clearProxyConfig(): Promise<boolean> {
    return true
  }

  async getNetworkDiagnostics(): Promise<NetworkDiagnostics> {
    return {
      dnsResolution: true,
      internetReachable: true,
      latencyMs: 25,
      bandwidthKbps: 50000,
      lastChecked: Date.now()
    }
  }

  async testConnectivity(url: string): Promise<ConnectivityTestResult> {
    return {
      success: true,
      url,
      statusCode: 200,
      latencyMs: 30
    }
  }

  // Mock helper methods
  simulateStateChange(state: Partial<NetworkState>): void {
    this.currentState = { ...this.currentState, ...state }
    this.listeners.forEach(cb => cb(this.currentState))
  }

  simulateOffline(): void {
    this.simulateStateChange({
      status: 'disconnected',
      isAvailable: false,
      type: 'none'
    })
  }

  simulateOnline(): void {
    this.simulateStateChange({
      status: 'connected',
      isAvailable: true,
      type: 'wifi'
    })
  }
}

export class NativeNetworkManager implements NetworkManager {
  // TODO: Implement with HarmonyOS Network Kit
  // This will use @ohos.net.connection for real network state

  async getCurrentState(): Promise<NetworkState> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async isConnected(): Promise<boolean> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async getConnectionType(): Promise<NetworkType> {
    throw new Error('NativeNetworkManager not implemented')
  }

  onStateChange(callback: (state: NetworkState) => void): void {
    throw new Error('NativeNetworkManager not implemented')
  }

  offStateChange(callback: (state: NetworkState) => void): void {
    throw new Error('NativeNetworkManager not implemented')
  }

  async getProxyConfig(): Promise<ProxyConfig> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async setProxyConfig(config: ProxyConfig): Promise<boolean> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async clearProxyConfig(): Promise<boolean> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async getNetworkDiagnostics(): Promise<NetworkDiagnostics> {
    throw new Error('NativeNetworkManager not implemented')
  }

  async testConnectivity(url: string): Promise<ConnectivityTestResult> {
    throw new Error('NativeNetworkManager not implemented')
  }
}
