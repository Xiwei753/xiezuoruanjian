// NativeDiagnosticsBridge — 诊断 NAPI 桥接层。
//
// 封装 Task #6 创建的 4 个诊断 NAPI handler：
//   nativeSetDiagnosticsConfig(enabled, verbose) → int32
//   nativeFlushDiagnostics()                    → int32
//   nativeClearDiagnostics()                    → int32
//   nativeExportDiagnostics(outputDir, attachJson) → string
//
// 与其他 Native*Bridge 不同，诊断操作不走 JSON envelope 协议：
//   set/flush/clear 返回 int32 状态码（0=成功，非0=失败）
//   export 返回 zip 文件路径字符串（失败时返回 JSON 错误 envelope）
//
// 脱敏由 Rust 层 redact 模块统一处理，ArkTS 层不做脱敏。

import { NativeCoreModule } from './NativeCoreModule'

export class NativeDiagnosticsBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  // 设置诊断配置（enabled/verbose）。
  // 返回 0 表示成功。
  setDiagnosticsConfig(enabled: boolean, verbose: boolean): number {
    const result = this.core.getNativeModule().nativeSetDiagnosticsConfig(
      enabled ? 1 : 0, verbose ? 1 : 0
    ) as number
    return result
  }

  // flush 日志落盘。
  // 返回 0 表示成功，-1 表示 flush 失败。
  flushDiagnostics(): number {
    return this.core.getNativeModule().nativeFlushDiagnostics() as number
  }

  // 清空日志文件。
  // 返回 0 表示成功，-1 表示 clear 失败。
  clearDiagnostics(): number {
    return this.core.getNativeModule().nativeClearDiagnostics() as number
  }

  // 导出诊断包，返回 zip 文件路径。
  // 失败时返回 JSON 错误 envelope 字符串（如 {"success":false,"errorCode":"EXPORT_FAILED"}）。
  exportDiagnostics(outputDir: string, attachmentsJson: string): string {
    return this.core.getNativeModule().nativeExportDiagnostics(outputDir, attachmentsJson) as string
  }
}
