// PeerSessionMirror — 附近设备会话的隔离远端镜像（Issue #988）。
//
// 收到的对端对象只写入本会话的临时目录，随后调用 Core 的 peer sync 入口：
// Core 仍按 staging → LWW 合并 → commit 写 live 数据根，平台层绝不直接覆盖作品。
// Core 写回该镜像的变更（mutations）再按文本协议发回对端。
//
// 镜像目录放在 filesDir/peer-sessions/<sessionId> 下，会话结束由协调器清理。

import fs from '@ohos.file.fs'
import util from '@ohos.util'
import { NativeSyncBridge } from '../../corebridge/native/NativeSyncBridge'
import { ResultEnvelope } from '../../corebridge/dto/ResultEnvelope'
import { PeerSyncResult } from '../../corebridge/dto/SyncDtos'
import {
  PeerObjectDescriptor,
  PeerSyncCommitResult,
  PeerSyncCoreMutation,
  PeerSyncCorePort
} from './PeerSyncSession'

function failed(message: string): PeerSyncCommitResult {
  return { success: false, applied: 0, message: message }
}

export class PeerSessionMirror implements PeerSyncCorePort {
  readonly root: string
  private bridge: NativeSyncBridge
  private applied: number = 0

  constructor(root: string, bridge: NativeSyncBridge) {
    this.root = root
    fs.mkdirSync(root, true)
    this.bridge = bridge
  }

  listLocalObjects(): PeerObjectDescriptor[] {
    // 本地对象由 Core 在 performPeerSync 中计算并写入镜像；平台层不自行遍历数据根，
    // 避免绕开 Core 的同步对象过滤和删除标记。
    return []
  }

  readLocalObject(_path: string): string | null {
    return null
  }

  applyRemoteObject(path: string, content: string, _contentHash: string): PeerSyncCommitResult {
    try {
      const fullPath: string = this.safePath(path)
      const parent: string = fullPath.substring(0, fullPath.lastIndexOf('/'))
      if (parent.length > 0) {
        fs.mkdirSync(parent, true)
      }
      const file = fs.openSync(fullPath,
        fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
      try {
        fs.writeSync(file.fd, content)
      } finally {
        fs.closeSync(file)
      }
      this.applied += 1
      return { success: true, applied: 1, message: 'staged' }
    } catch (e) {
      return failed(`写入附近设备会话对象失败：${String(e)}`)
    }
  }

  deleteRemoteObject(path: string, _contentHash: string): PeerSyncCommitResult {
    try {
      const fullPath: string = this.safePath(path)
      if (fs.accessSync(fullPath)) {
        fs.unlinkSync(fullPath)
      }
      return { success: true, applied: 1, message: 'deleted' }
    } catch (e) {
      return failed(`删除附近设备会话对象失败：${String(e)}`)
    }
  }

  async performCoreSync(): Promise<PeerSyncCoreMutation[]> {
    const result: ResultEnvelope<PeerSyncResult> = await this.bridge.performPeerSync(this.root)
    if (!result.success || result.data === undefined) {
      throw new Error(result.userMessage ?? '附近设备 Core 同步失败')
    }
    const mutations: PeerSyncCoreMutation[] = []
    const decoder = new util.TextDecoder('utf-8')
    const base64 = new util.Base64Helper()
    result.data.mutations.forEach((mutation): void => {
      if (mutation.kind === 'delete') {
        mutations.push({
          kind: 'delete',
          path: mutation.path,
          content: '',
          version: mutation.version ?? ''
        })
        return
      }
      if (mutation.contentBase64 === null) {
        return
      }
      const bytes: Uint8Array = base64.decodeSync(mutation.contentBase64)
      mutations.push({
        kind: 'put',
        path: mutation.path,
        content: decoder.decodeWithStream(bytes),
        version: mutation.version ?? ''
      })
    })
    return mutations
  }

  clear(): void {
    try {
      if (fs.accessSync(this.root)) {
        this.removeTree(this.root)
      }
    } catch (_e) {
      // 清理失败不改变同步结果；下次会话使用新的目录。
    }
  }

  private removeTree(path: string): void {
    const stat = fs.statSync(path)
    if (!stat.isDirectory()) {
      fs.unlinkSync(path)
      return
    }
    fs.listFileSync(path).forEach((name: string): void => {
      this.removeTree(`${path}/${name}`)
    })
    fs.rmdirSync(path)
  }

  appliedCount(): number {
    return this.applied
  }

  private safePath(path: string): string {
    if (path.length === 0 || path.startsWith('/') || path.includes('..') || path.includes('\\')) {
      throw new Error(`非法附近设备对象路径：${path}`)
    }
    return `${this.root}/${path}`
  }
}
