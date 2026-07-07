# Windows Bridge — Rust writer_core → C# P/Invoke

本目录是 Windows 原生客户端调用 Rust `writer_core` 的桥接层。

## 架构

```
core/writer_core  →  cdylib (writer_core.dll)  →  P/Invoke  →  C# WriterCoreBridge
```

- Rust `writer_core` 编译为 `cdylib`，导出 C ABI 函数（`writer_core_init`、`writer_core_list_projects` 等）。
- C# 通过 `DllImport` / P/Invoke 调用这些函数。
- 复杂数据通过 JSON C string 传递：Rust 序列化 → C string → C# `Marshal.PtrToStringUTF8` → `JsonSerializer.Deserialize`。
- 调用方须用 `writer_core_free_string` 释放 Rust 分配的 C string。

## 构建步骤

1. 编译 Rust `writer_core`（启用 `harmony-ffi` feature 以导出 C ABI）：
   ```powershell
   cd core/writer_core
   cargo build --release --features harmony-ffi
   ```
   产物位于 `target/release/writer_core.dll`。

2. 将 DLL 复制到 Windows 客户端输出目录：
   ```powershell
   copy target\release\writer_core.dll apps\windows\bin\
   ```

3. 在 Visual Studio 或 `dotnet build` 中构建 `SujianWindows.csproj`。

## C ABI 契约

所有导出函数签名见 `core/writer_core/src/ffi/mod.rs`。Windows 客户端只使用以下第一批桥接：

| Rust C ABI 函数 | C# Bridge 方法 | 说明 |
|---|---|---|
| `writer_core_init(path)` | `InitWorkspace(path)` | 初始化全局单例 |
| `writer_core_open_workspace(path)` | `OpenWorkspace(path)` | 打开/切换工作区 |
| `writer_core_list_projects()` | `ListProjects()` | 列出项目 |
| `writer_core_list_volumes(pid)` | `ListVolumes(pid)` | 列出卷 |
| `writer_core_list_chapters(pid, vid)` | `ListChapters(pid, vid)` | 列出章节 |
| `writer_core_open_chapter(pid, vid, cid)` | `OpenChapter(pid, vid, cid)` | 打开章节正文 |
| `writer_core_save_chapter(pid, vid, cid, content)` | `SaveChapter(pid, vid, cid, content)` | 保存章节正文 |
| `writer_core_load_local_settings()` | `LoadSettings()` | 读取本地设置 |
| `writer_core_save_local_settings(json)` | `SaveSettings(json)` | 保存本地设置 |
| `writer_core_calculate_word_count(text)` | `CalculateWordCount(text)` | 计算字数 |
| `writer_core_free_string(ptr)` | (内部释放) | 释放 Rust C string |

## 设计原则

- **UI 层不复制业务逻辑**：所有工作区、项目、章节、保存、同步、设置逻辑留在 `writer_core`。
- **只做薄适配**：`WriterCoreBridge` 只负责 P/Invoke 调用和 JSON 反序列化。
- **错误走 ResultEnvelope**：所有 JSON 返回 `{"success":bool,"data":...,"errorCode":...,"userMessage":...}`，C# 端统一检查 `success`。
- **不泄露 Rust 内存**：所有 Rust 分配的 C string 必须通过 `writer_core_free_string` 释放。
