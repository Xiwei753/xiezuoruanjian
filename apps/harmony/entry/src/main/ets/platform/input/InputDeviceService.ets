// InputDeviceService — 鸿蒙输入设备发现/上下线/能力查询（单例）。
// 产生 keyboard/mouse/touchpad/touchscreen/joystick/trackball 平台状态，供 UI 订阅调整交互。
// 不把 DeviceInfo 传进 Core；Core 只消费编辑命令。
// stylus 不在此推断，由 StylusInputService 依据笔事件能力独立判断。
//
// 真实事件源：@kit.InputKit 的 inputDevice.getDeviceList()（返回设备 id 数组）+
// inputDevice.getDeviceInfo(id) + inputDevice.on('change')。
// notify* 改成 private 分发方法，仅由系统回调调用，外部不得直接 notify。

import { inputDevice } from '@kit.InputKit'

export interface InputDeviceInfo {
  id: number
  name: string
  // kinds 为 HarmonyOS SourceType 字符串子集：
  // 'keyboard' | 'mouse' | 'touchpad' | 'touchscreen' | 'joystick' | 'trackball'
  kinds: string[]
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
  // getDeviceList() 返回在线设备 id 数组（number[]），逐 id 调 getDeviceInfo 取详情；
  // 注册 inputDevice.on('change') 只处理 add/remove。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    // 1. 拉取当前在线设备 id 列表，逐 id 查询详情并上线。
    // SDK 类型声明可能把 getDeviceList 标为 InputDeviceData[]，但实际返回 number[]，
    // 这里按评论契约强制成 Array<number>。
    const ids: Array<number> = (await inputDevice.getDeviceList()) as unknown as Array<number>
    for (const id of ids) {
      try {
        const raw: inputDevice.InputDeviceData = await inputDevice.getDeviceInfo(id)
        const info: InputDeviceInfo = this.mapDevice(raw, true)
        this.notifyDeviceAdded(info)
      } catch (e) {
        // 单个设备查询失败跳过，不伪造状态。
      }
    }
    // 2. 注册设备增删监听。系统回调里只调 private notify*；只处理 add/remove。
    inputDevice.on('change', (data: inputDevice.DeviceListener) => {
      if (data.type === 'add') {
        this.handleDeviceChange(data.deviceId, true)
      } else if (data.type === 'remove') {
        this.notifyDeviceRemoved(data.deviceId)
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
  // 系统 'add' 回调只给 deviceId，直接 getDeviceInfo 查询详情，不再遍历 getDeviceList。
  private async handleDeviceChange(deviceId: number, connected: boolean): Promise<void> {
    try {
      const raw: inputDevice.InputDeviceData = await inputDevice.getDeviceInfo(deviceId)
      const info: InputDeviceInfo = this.mapDevice(raw, connected)
      this.notifyDeviceAdded(info)
    } catch (e) {
      // 查询失败时不伪造状态；设备已下线时清理。
      if (!connected) {
        this.notifyDeviceRemoved(deviceId)
      }
    }
  }

  // 把系统 InputDeviceData 映射成业务 InputDeviceInfo。
  // sources 为 HarmonyOS SourceType 字符串数组，按字符串 includes 映射成 kinds。
  // 不再从 sources 推断 stylus；stylus 由 StylusInputService 独立判断。
  private mapDevice(raw: inputDevice.InputDeviceData, connected: boolean): InputDeviceInfo {
    const kinds: string[] = []
    const sources: string[] = raw.sources ?? []
    if (sources.includes('keyboard')) {
      kinds.push('keyboard')
    }
    if (sources.includes('mouse')) {
      kinds.push('mouse')
    }
    if (sources.includes('touchpad')) {
      kinds.push('touchpad')
    }
    if (sources.includes('touchscreen')) {
      kinds.push('touchscreen')
    }
    if (sources.includes('joystick')) {
      kinds.push('joystick')
    }
    if (sources.includes('trackball')) {
      kinds.push('trackball')
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
