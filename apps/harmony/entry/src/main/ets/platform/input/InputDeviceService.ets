// InputDeviceService — 鸿蒙输入设备发现/上下线/能力查询（单例）。
// 产生 keyboard/mouse/touchpad/stylus 平台状态，供 UI 订阅调整交互。
// 不把 DeviceInfo 传进 Core；Core 只消费编辑命令。

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

export class InputDeviceService {
  private static instance: InputDeviceService
  private devices: Map<number, InputDeviceInfo> = new Map<number, InputDeviceInfo>()
  private observers: Set<InputDeviceObserver> = new Set<InputDeviceObserver>()

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

  addObserver(observer: InputDeviceObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: InputDeviceObserver): void {
    this.observers.delete(observer)
  }

  // 平台层在收到设备事件时调用。
  notifyDeviceAdded(device: InputDeviceInfo): void {
    this.devices.set(device.id, device)
    this.observers.forEach(o => { if (o.onDeviceAdded) o.onDeviceAdded(device) })
  }

  notifyDeviceRemoved(deviceId: number): void {
    this.devices.delete(deviceId)
    this.observers.forEach(o => { if (o.onDeviceRemoved) o.onDeviceRemoved(deviceId) })
  }
}
