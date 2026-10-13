// NetworkStatusService — 统一的网络状态查询。
//
// 平台端把真实网络状态（连通 / 计费）注入 Core，替代此前「查询失败仍默认在线」
// 和「只在启动时拍一次快照」的伪状态。
//
// - query()：查询当前网络，失败时返回 known=false（未知），不伪装成在线。
// - 初始化与每次同步操作（performSync / diagnostics）前都会重新查询，
//   把当前状态推给 Core，因此不是进程首次初始化的旧快照。
//
// 权限：GET_NETWORK_INFO（已在 module.json5 声明）。

import hilog from '@ohos.hilog'
import { connection } from '@kit.NetworkKit'

const TAG = 'NetworkStatusService'
const DOMAIN = 0x0000

/** 三态网络快照。known=false 表示查询失败/未知，不是离线。 */
export interface NetworkSnapshot {
  known: boolean
  isConnected: boolean
  isMetered: boolean
  queryFailed: boolean
  error: string
}

export class NetworkStatusService {
  private static instance: NetworkStatusService

  static getInstance(): NetworkStatusService {
    if (!NetworkStatusService.instance) {
      NetworkStatusService.instance = new NetworkStatusService()
    }
    return NetworkStatusService.instance
  }

  /** 查询当前网络状态。查询失败时 known=false，绝不返回伪造的在线状态。 */
  async query(): Promise<NetworkSnapshot> {
    try {
      const hasNet: boolean = await connection.hasDefaultNet()
      let metered: boolean = false
      if (hasNet) {
        metered = await connection.isDefaultNetMetered()
      }
      return { known: true, isConnected: hasNet, isMetered: metered, queryFailed: false, error: '' }
    } catch (e) {
      hilog.warn(DOMAIN, TAG, 'network query failed, state is UNKNOWN (not online): %{public}s', String(e))
      return { known: false, isConnected: false, isMetered: false, queryFailed: true, error: String(e) }
    }
  }
}
