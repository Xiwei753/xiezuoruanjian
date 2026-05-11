# Linux IME Notes

If you are running Writer App on Linux, particularly on Fedora KDE Wayland with fcitx5, you might experience issues with the Flutter `TextField` where the pre-editing candidate box (composing region) flickers or twitches during input. This is a known interaction issue between Flutter's text input handling and certain Linux IME/Wayland setups.

## Workarounds

1.  **Release Mode**: The issue is often exacerbated in Debug mode due to performance overhead and frequent rebuilds. Try running or building the app in release mode:
    ```bash
    flutter run --release
    ```
    or
    ```bash
    flutter build linux
    ```

2.  **Use X11 Backend**: Forcing the app to use the X11 backend under Wayland can bypass some Wayland-specific IME rendering bugs. You can set the `GDK_BACKEND` environment variable before launching:
    ```bash
    GDK_BACKEND=x11 flutter run
    ```

3.  **KDE X11 Session**: If you are still experiencing severe issues and the above steps do not help, consider logging out of Wayland and running a KDE X11 session as a temporary workaround.

4.  **IME Safe Mode (In-App)**: Writer App includes an experimental "IME Safe Mode" built specifically to mitigate this issue. This mode:
    *   Prioritizes reducing the number of global state saves and UI rebuilds during active text input.
    *   Completely pauses saving editor state (like cursor position or scroll offset) while the IME composing region is active.
    *   Uses silent saves to update disk state without triggering `ChangeNotifier` listeners that might cause the application's root `MaterialApp` to unnecessarily rebuild and interrupt the input context.
    *   Can be enabled within the App Settings (if exposed to the UI) or by manually modifying the `imeSafeModeEnabled` flag in your configuration.

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
