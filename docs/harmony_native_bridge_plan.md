# HarmonyOS Native Bridge 接入计划

Status: active
Last verified: 2026-06-23
Truth source: docs/harmony_bridge_contract.md

---

## 概述

本文档规划鸿蒙端从 Mock Bridge 过渡到 Native Bridge 的技术方案和实施步骤。

---

## 当前状态

- **Mock Bridge**: 完成，用于验证页面结构和接口形状
- **Native Bridge**: 空壳，等待接入真实 Rust Core

---

## 接入方案评估

### 方案 A：C-ABI + NAPI（推荐）

**架构**：
```
ArkTS (UI) → NAPI Binding → C ABI (.so) → Rust Core
```

**优点**：
- 性能最优，直接调用
- 与 Android UniFFI 路线一致
- 类型安全，编译时检查

**缺点**：
- 需要鸿蒙 NAPI 工具链支持
- 需要处理 native 生命周期
- 需要编译 Rust 为 HarmonyOS 目标

**实施步骤**：
1. 配置 Rust 交叉编译环境
2. 编译 `writer_core` 为 HarmonyOS AArch64 目标
3. 导出 C ABI 接口
4. 使用 NAPI 工具生成 ArkTS 绑定
5. 实现 `NativeWriterCoreBridge`

**前置条件**：
- Rust 支持 `aarch64-unknown-linux-ohos` 目标
- HarmonyOS NAPI 工具链可用
- Desktop 抢修完成，Core 接口稳定

### 方案 B：本地服务

**架构**：
```
ArkTS (UI) → HTTP/gRPC Client → Local Server (Rust) → 文件系统
```

**优点**：
- 进程隔离，稳定性好
- 不依赖 native 工具链
- 易于调试和测试

**缺点**：
- 性能开销较大（网络序列化）
- 需要处理服务生命周期
- 需要实现服务发现机制

**实施步骤**：
1. 实现 Rust HTTP/gRPC 服务
2. 编译服务为 HarmonyOS 可执行文件
3. 实现服务启动/停止管理
4. 实现 ArkTS HTTP/gRPC 客户端
5. 实现 `NativeWriterCoreBridge` 调用客户端

**前置条件**：
- Rust HTTP/gRPC 框架支持 HarmonyOS
- HarmonyOS 后台服务机制可用
- 网络权限配置完成

### 方案 C：UniFFI 扩展

**架构**：
```
ArkTS (UI) → UniFFI Binding → Rust Core
```

**优点**：
- 与 Android 完全一致
- 维护成本最低
- 自动类型映射

**缺点**：
- 依赖 UniFFI 鸿蒙支持（可能需要社区贡献）
- 可能需要扩展 UniFFI 代码生成器

**实施步骤**：
1. 评估 UniFFI 鸿蒙支持状态
2. 如不支持，贡献鸿蒙目标代码生成
3. 配置 UniFFI 绑定生成
4. 实现 `NativeWriterCoreBridge` 使用生成的绑定

**前置条件**：
- UniFFI 支持 ArkTS/鸿蒙目标
- 或社区贡献完成

---

## 推荐路线

### 阶段 1：准备期（当前）

- 完成 Mock Bridge 验证
- 稳定 Core API 接口
- 等待 Desktop 抢修完成
- 评估鸿蒙 NAPI 工具链可用性

### 阶段 2：原型验证

- 选择接入方案（推荐方案 A）
- 搭建最小可运行原型
- 验证数据流通路
- 性能基准测试

### 阶段 3：完整接入

- 实现完整 `NativeWriterCoreBridge`
- 处理所有错误场景
- 实现生命周期管理
- 集成测试

### 阶段 4：优化迭代

- 性能优化
- 内存管理优化
- 错误处理完善
- 用户体验打磨

---

## 技术细节

### Rust 目标配置

```toml
# .cargo/config.toml
[target.aarch64-unknown-linux-ohos]
linker = "ohos-clang"
ar = "ohos-ar"
```

### C ABI 导出示例

```rust
// core/writer_core/src/ffi/mod.rs (及 ffi/ 目录下各模块)
#[no_mangle]
pub extern "C" fn writer_core_open_workspace(
    path: *const c_char,
    result: *mut FfiResult
) -> i32 {
    // Implementation
}
```

### NAPI 绑定示例

```typescript
// entry/src/main/ets/bridge/native_bindings.ts
import nativeModule from 'libwriter_core.so'

export function openWorkspace(path: string): Promise<WorkspaceState> {
  return new Promise((resolve, reject) => {
    nativeModule.openWorkspace(path, (err, result) => {
      if (err) reject(err)
      else resolve(result)
    })
  })
}
```

### 生命周期管理

```typescript
// entry/src/main/ets/bridge/NativeWriterCoreBridge.ets
export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private isInitialized: boolean = false

  async initialize(): Promise<void> {
    if (this.isInitialized) return
    
    // Initialize native module
    await nativeModule.initialize()
    this.isInitialized = true
  }

  async dispose(): Promise<void> {
    if (!this.isInitialized) return
    
    // Cleanup native resources
    await nativeModule.dispose()
    this.isInitialized = false
  }
}
```

---

## 风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| Rust 不支持 HarmonyOS 目标 | 使用方案 B（本地服务）作为备选 |
| NAPI 工具链不稳定 | 等待工具链成熟，或使用方案 C |
| 性能不达标 | 优化序列化，使用共享内存 |
| 内存泄漏 | 实现完善的生命周期管理 |
| 接口变动 | 保持 Mock Bridge 作为兼容层 |

---

## 时间线

| 阶段 | 预计时间 | 前置条件 |
|------|---------|---------|
| 准备期 | 当前 - Desktop 抢修完成 | 无 |
| 原型验证 | 2-4 周 | Desktop 稳定，工具链可用 |
| 完整接入 | 4-8 周 | 原型验证通过 |
| 优化迭代 | 持续 | 完整接入完成 |

---

## 参考文档

- [harmony_bridge_contract.md](harmony_bridge_contract.md) - 鸿蒙桥接契约
- [harmony_route.md](harmony_route.md) - 鸿蒙技术路线
- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) - 跨平台能力契约（已归档）

- [API_CONTRACTS.md](archive/API_CONTRACTS.md) - 接口边界与交互契约（已归档）