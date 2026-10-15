// PrimaryActionChannel — 当前屏幕「主操作」回调通道。
//
// Issue #905（补回原生「+」光感）：页面把「新建作品 / 新建卷 / 新建星图」的现成回调
// 按 ScreenRole 登记到这里；窄屏底部 HdsTabs 的原生 MiniBar 承载这个「+」，
// 由 HDS 宿主透出系统沉浸光感（华为《开启沉浸光感》只对合规标题栏和底部 TabBar 生效），
// 不再对内容流 Button 硬挂无效的 .systemMaterial。
//
// 职责边界：只登记「role → 回调 + 可用状态」，不含业务逻辑、不新建路由、不自绘材质。
// 业务动作仍是各页面原本的命名框 / 建卷 / 建星图入口，本通道只负责连线。
// 每次注册生成一个 owner token，页面卸载只注销自己的登记，避免迟到回调互相覆盖。

import { ScreenRole, PrimaryNavigationPlacement } from '../../corebridge/dto/PlatformDtos'
import { AdaptiveState } from '../../ui/adaptive/AdaptiveContext'

interface PrimaryActionEntry {
  token: number
  onTap: () => void
  available: boolean
}

type PrimaryActionObserver = () => void

export class PrimaryActionChannel {
  private static instance: PrimaryActionChannel
  private entries: Map<ScreenRole, PrimaryActionEntry> = new Map()
  private observers: PrimaryActionObserver[] = []
  private nextToken: number = 1

  private constructor() {}

  static getInstance(): PrimaryActionChannel {
    if (!PrimaryActionChannel.instance) {
      PrimaryActionChannel.instance = new PrimaryActionChannel()
    }
    return PrimaryActionChannel.instance
  }

  // 登记某个 role 的主操作回调，返回本次登记的 owner token。
  register(role: ScreenRole, onTap: () => void, available: boolean): number {
    const token: number = this.nextToken++
    this.entries.set(role, { token, onTap, available })
    this.notify()
    return token
  }

  // 页面加载 / 路由状态变化后更新可用性；token 不匹配则忽略（旧登记的迟到更新）。
  updateAvailability(role: ScreenRole, token: number, available: boolean): void {
    const entry: PrimaryActionEntry | undefined = this.entries.get(role)
    if (entry === undefined || entry.token !== token) {
      return
    }
    if (entry.available !== available) {
      entry.available = available
      this.notify()
    }
  }

  // 只注销自己登记的条目。
  unregister(role: ScreenRole, token: number): void {
    const entry: PrimaryActionEntry | undefined = this.entries.get(role)
    if (entry !== undefined && entry.token === token) {
      this.entries.delete(role)
      this.notify()
    }
  }

  hasPrimaryAction(role: ScreenRole): boolean {
    const entry: PrimaryActionEntry | undefined = this.entries.get(role)
    return entry !== undefined && entry.available
  }

  // 执行当前 role 已登记且可用的主操作；未登记或不可用时什么都不做。
  dispatch(role: ScreenRole): void {
    const entry: PrimaryActionEntry | undefined = this.entries.get(role)
    if (entry !== undefined && entry.available) {
      entry.onTap()
    }
  }

  subscribe(observer: PrimaryActionObserver): void {
    this.observers.push(observer)
    observer()
  }

  unsubscribe(observer: PrimaryActionObserver): void {
    const index = this.observers.indexOf(observer)
    if (index >= 0) {
      this.observers.splice(index, 1)
    }
  }

  private notify(): void {
    for (const observer of this.observers) {
      observer()
    }
  }
}

// 页面自绘 PrimaryFab 与 HDS MiniBar 的分工判定：
// API23+ 且一级导航在底部（底部是 HdsTabs）时，主操作由 MiniBar 承载，
// 页面不再画内容流 FAB，避免出现两个「+」、重复触摸区和双重创建。
// 宽屏侧栏或 API < 23 时底部没有 HDS TabBar，仍由页面画普通 FAB。
// 与 PrimaryTabShell.isWidePrimaryNavigation 的分流口径保持一致。
export function isHdsMiniBarHostingPrimaryAction(useHdsNav: boolean, adaptiveState: AdaptiveState): boolean {
  if (!useHdsNav) {
    return false
  }
  const wideNavigation: boolean =
    adaptiveState.layoutPlan.primaryNavigationPlacement === PrimaryNavigationPlacement.Side ||
    (adaptiveState.isDedicatedLandscapeTablet && adaptiveState.prefersWideLayout)
  return !wideNavigation
}
