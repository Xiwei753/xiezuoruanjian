# 客户端功能矩阵 (Client Feature Matrix)

本表格展示了各个客户端目前在架构迁移阶段的功能和状态。

| 功能项 (Feature) | Flutter Legacy | Android Native | Linux Native |
| :--- | :--- | :--- | :--- |
| **状态 (Status)** | **Legacy / Prototype (历史原型)** | **Skeleton (仅骨架，不可用)** | **Skeleton (仅骨架，不可用)** |
| UI 框架 | Flutter | Kotlin / Views | Qt / C++ |
| 编辑器组件 | 自定义 Flutter Widget | 原生 EditText (待实现) | QPlainTextEdit (待实现) |
| Rust Core 集成 | 无 (使用旧版 Dart 逻辑) | 是 (通过 FFI/JNI Bindings，待实现) | 是 (通过 FFI Bindings，待实现) |
| 工作区校验 (Workspace Validation)| 是 (通过 Dart) | 是 (将通过 Rust Core) | 是 (将通过 Rust Core) |
| 同步实现 (Sync Implementation) | 原型阶段 (Prototype) | 待定 (TBD) | 待定 (TBD) |

**注**：Android Native 和 Linux Native 目前**仅包含项目构建骨架**，并没有实现真正的可用逻辑。请勿将其作为可运行软件使用。
