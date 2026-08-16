// InputDeviceService — 鸿蒙输入设备发现/上下线/能力查询（单例）。
// 产生 keyboard/mouse/touchpad/stylus 平台状态，供 UI 订阅调整交互。
// 不把 DeviceInfo 传进 Core；Core 只消费编辑命令。
//
// 真实事件源：@kit.InputKit 的 inputDevice.getDeviceList() + inputDevice.on('change')。
// notify* 改成 private 分发方法，仅由系统回调调用，外部不得直接 notify。

import { inputDevice } from '@kit.InputKit'

export interface InputDeviceInfo {
  id: number
  name: string
  kinds: string[]  // 'keyboard' | 'mouse' | 'touchpad' | 'stylus' | 'pointer'
  isConnected: boolean
}

export interface InputDeviceObserver {
  onDeviceAdded?(device: InputDeviceInfo): void
  onDeviceRemoved?(deviceId: number): void
  onDeviceChanged?(device: InputDeviceInfo): void
}

// HarmonyOS inputDevice SourceType 常量（@ohos.multimodalInput.inputDevice）。
// 用于把 sources 数组映射成业务可读的 kinds。
const SOURCE_KEYBOARD: number = 1
const SOURCE_POINTER: number = 2
const SOURCE_TOUCHPAD: number = 4
const SOURCE_STYLUS: number = 6

export class InputDeviceService {
  private static instance: InputDeviceService
  private devices: Map<number, InputDeviceInfo> = new Map<number, InputDeviceInfo>()
  private observers: Set<InputDeviceObserver> = new Set<InputDeviceObserver>()
  private listening: boolean = false

  static getInstance(): InputDeviceService {
    if (!InputDeviceService.instance) {
      InputDeviceService.instance = new InputDeviceService()
    }
    return InputDeviceService.instance
  }

  listDevices(): InputDeviceInfo[] {
    return Array.from(this.devices.values())
  }

  getDevice(deviceId: number): InputDeviceInfo | undefined {
    return this.devices.get(deviceId)
  }

  hasDeviceOfKind(kind: string): boolean {
    for (const d of this.devices.values()) {
      if (d.kinds.indexOf(kind) >= 0 && d.isConnected) {
        return true
      }
    }
    return false
  }

  addObserver(observer: InputDeviceObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: InputDeviceObserver): void {
    this.observers.delete(observer)
  }

  isListening(): boolean {
    return this.listening
  }

  // 启动系统设备枚举 + 上下线监听。
  // 调用 inputDevice.getDeviceList() 初始化设备列表，注册 inputDevice.on('change')。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    // 1. 拉取当前在线设备列表。
    const deviceList: Array<inputDevice.InputDeviceData> = await inputDevice.getDeviceList()
    for (const raw of deviceList) {
      const info: InputDeviceInfo = this.mapDevice(raw, true)
      this.notifyDeviceAdded(info)
    }
    // 2. 注册设备增删/变化监听。系统回调里只调 private notify*。
    inputDevice.on('change', (data: inputDevice.DeviceListener) => {
      if (data.type === 'add') {
        this.handleDeviceChange(data.deviceId, true)
      } else if (data.type === 'remove') {
        this.notifyDeviceRemoved(data.deviceId)
      } else {
        // 'change' — 设备能力变化，重新拉取并覆盖。
        this.handleDeviceChange(data.deviceId, true)
      }
    })
    this.listening = true
  }

  // 停止系统监听，清空设备表。
  stopListening(): void {
    if (!this.listening) {
      return
    }
    inputDevice.off('change')
    this.devices.clear()
    this.listening = false
  }

  // —— private ——
  // 系统 'add' / 'change' 回调只给 deviceId，需重新查询设备详情。
  private async handleDeviceChange(deviceId: number, connected: boolean): Promise<void> {
    try {
      const all: Array<inputDevice.InputDeviceData> = await inputDevice.getDeviceList()
      let found: inputDevice.InputDeviceData | undefined = undefined
      for (const d of all) {
        if (d.id === deviceId) {
          found = d
          break
        }
      }
      if (found) {
        const info: InputDeviceInfo = this.mapDevice(found, connected)
        this.notifyDeviceAdded(info)
      } else if (!connected) {
        this.notifyDeviceRemoved(deviceId)
      }
    } catch (e) {
      // 查询失败时不伪造状态；保持现有表不变。
    }
  }

  // 把系统 InputDeviceData 映射成业务 InputDeviceInfo。
  // sources 字段含 SOURCE_* 常量，对应 keyboard/mouse/touchpad/stylus。
  private mapDevice(raw: inputDevice.InputDeviceData, connected: boolean): InputDeviceInfo {
    const kinds: string[] = []
    const sources: number[] = raw.sources ?? []
    if (sources.indexOf(SOURCE_KEYBOARD) >= 0) {
      kinds.push('keyboard')
    }
    if (sources.indexOf(SOURCE_POINTER) >= 0) {
      kinds.push('pointer')
      kinds.push('mouse')
    }
    if (sources.indexOf(SOURCE_TOUCHPAD) >= 0) {
      kinds.push('touchpad')
    }
    if (sources.indexOf(SOURCE_STYLUS) >= 0) {
      kinds.push('stylus')
    }
    if (kinds.length === 0) {
      kinds.push('pointer')
    }
    return {
      id: raw.id,
      name: raw.name ?? `device-${raw.id}`,
      kinds: kinds,
      isConnected: connected
    }
  }

  // private 分发：仅由系统回调触发，外部不得直接调用。
  private notifyDeviceAdded(device: InputDeviceInfo): void {
    this.devices.set(device.id, device)
    this.observers.forEach(o => { if (o.onDeviceAdded) o.onDeviceAdded(device) })
  }

  private notifyDeviceRemoved(deviceId: number): void {
    this.devices.delete(deviceId)
    this.observers.forEach(o => { if (o.onDeviceRemoved) o.onDeviceRemoved(deviceId) })
  }
}
