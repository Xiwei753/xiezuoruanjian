# Windows 客户端（预留）

状态：待更改至原生。

- 当前不复用 `apps/Linux_qt` 的 Qt/QML 代码。
- 旧 Windows Qt 打包、安装器、DWM 标题栏、pending key/IME adapter 等兼容路径已从 Linux Qt 路线移除。
- 后续如启动 Windows 客户端，应在本目录重新规划原生 Windows 技术路线，并继续复用 `core/writer_core` 作为唯一业务核心。

## 后续路线

- Windows：待更改至原生。
- Linux GTK：如未来需要，可新增 Linux GTK 客户端目录；当前仅预留路线说明，不新增实现。
