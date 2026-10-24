// GripLayoutSide — 智感握姿在 UI 层的左右布局状态（Issue #986）。
//
// 平台层 GripPosture 有 unknown / notHeld / left / right / both 五态；
// 布局只关心明确的左右手：其余状态保持上一次明确握持侧，首次默认右侧。
// 系统订阅由 AppContext.startPlatformServices() 启动，本文件不调用 motion.on。

import { GripPostureService } from '../../platform/awareness/GripPostureService'
import { GripPosture, GripPostureObserver } from '../../platform/awareness/GripPostureTypes'

export type GripLayoutSide = 'left' | 'right'
export type GripLayoutSideListener = (side: GripLayoutSide) => void

class GripLayoutSideObserver implements GripPostureObserver {
  private store: GripLayoutSideStore

  constructor(store: GripLayoutSideStore) {
    this.store = store
  }

  onGripPostureChanged(posture: GripPosture): void {
    this.store.applyPosture(posture)
  }
}

export class GripLayoutSideStore {
  private static instance: GripLayoutSideStore
  private side: GripLayoutSide = 'right'
  private listeners: Set<GripLayoutSideListener> = new Set<GripLayoutSideListener>()
  private observer: GripLayoutSideObserver = new GripLayoutSideObserver(this)

  static getInstance(): GripLayoutSideStore {
    if (!GripLayoutSideStore.instance) {
      GripLayoutSideStore.instance = new GripLayoutSideStore()
    }
    return GripLayoutSideStore.instance
  }

  getSide(): GripLayoutSide {
    return this.side
  }

  subscribe(listener: GripLayoutSideListener): () => void {
    const service: GripPostureService = GripPostureService.getInstance()
    if (this.listeners.size === 0) {
      // 页面晚挂载时也能读到已经识别的手，而不必等下一次换手。
      this.applyPosture(service.getPosture())
      service.addObserver(this.observer)
    }
    this.listeners.add(listener)
    listener(this.side)
    return (): void => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        service.removeObserver(this.observer)
      }
    }
  }

  applyPosture(posture: GripPosture): void {
    // 'both' / 'notHeld' / 'unknown' 不覆盖上一侧。
    if (posture !== 'left' && posture !== 'right') {
      return
    }
    if (posture === this.side) {
      return
    }
    this.side = posture
    this.listeners.forEach((listener: GripLayoutSideListener): void => listener(this.side))
  }
}
