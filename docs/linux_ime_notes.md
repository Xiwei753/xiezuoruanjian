# Linux 输入法笔记

如果你在 Linux 上运行写作应用，特别是在 Fedora KDE Wayland 配合 fcitx5 的环境下，可能会遇到 Flutter `TextField` 的预编辑候选框（组合区域）在输入时闪烁或抖动的问题。这是 Flutter 文本输入处理与某些 Linux 输入法/Wayland 配置之间的已知交互问题。

## 解决方法

1.  **Release 模式**：由于性能开销和频繁重建，该问题在 Debug 模式下通常会加剧。尝试以 release 模式运行或构建应用：
    ```bash
    flutter run --release
    ```
    或
    ```bash
    flutter build linux
    ```

2.  **使用 X11 后端**：在 Wayland 下强制应用使用 X11 后端可以绕过一些 Wayland 特定的输入法渲染 bug。你可以在启动前设置 `GDK_BACKEND` 环境变量：
    ```bash
    GDK_BACKEND=x11 flutter run
    ```

3.  **KDE X11 会话**：如果仍然遇到严重问题且上述步骤无效，可以考虑注销 Wayland 并使用 KDE X11 会话作为临时解决方案。

4.  **输入法安全模式（应用内）**：写作应用包含一个专门为此问题设计的实验性"输入法安全模式"。此模式：
    *   优先减少活动文本输入期间的全局状态保存和 UI 重建次数。
    *   在输入法组合区域活动时，完全暂停保存编辑器状态（如光标位置或滚动偏移）。
    *   使用静默保存更新磁盘状态，而不触发 `ChangeNotifier` 监听器（这些监听器可能导致应用的根 `MaterialApp` 不必要地重建并中断输入上下文）。
    *   可在应用设置中启用（如果暴露给 UI）或通过手动修改配置中的 `imeSafeModeEnabled` 标志启用。

---

## 补充说明：环境变量冲突

在 Fedora KDE Wayland + fcitx5 下，推荐使用 Wayland 输入法前端。KDE 设置里应选择：系统设置 -> 键盘 -> 虚拟键盘 -> Fcitx 5。

Wayland 下不建议全局设置 `GTK_IM_MODULE` / `QT_IM_MODULE` / `SDL_IM_MODULE`。
可以保留 `XMODIFIERS=@im=fcitx` 给 XWayland 应用。

如果依然出现候选框抽搐，可以使用附带的测试脚本：
```bash
./tool/run_linux_wayland_clean_ime.sh
```
如果清理环境变量后不抽搐，说明是环境变量冲突导致的。永久修复方式是从 `~/.config/environment.d`、`~/.profile`、`~/.bashrc`、`/etc/profile.d` 等地方移除 `GTK_IM_MODULE`/`QT_IM_MODULE`/`SDL_IM_MODULE` 的强制设置。
