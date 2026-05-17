# Linux Native Client

This directory contains the Linux native client, built using Rust, `qmetaobject`, and Qt/QML.

## Architecture Note
This client **MUST** use the `core/writer_core` Rust library via bindings (FFI). It is strictly forbidden to implement workspace formats, save logic, or syncing directly in C++ or QML.
The UI is built with Qt 5 / QML, relying on the Rust core for all business logic.

## Dependencies

To build and run the Linux application, you need to install the Qt 5 development packages and qmake.

### Fedora / openSUSE
```bash
sudo dnf install qt5-qtbase-devel qt5-qtdeclarative-devel qt5-qtquickcontrols2-devel qt5-qtwayland
```
*(Note: on Fedora/openSUSE, `qmake` might be available as `qmake-qt5`)*

### Ubuntu / Debian
```bash
sudo apt install qtbase5-dev qtdeclarative5-dev qtquickcontrols2-5-dev qtwayland5
```

## Building & Running

Run the following command from the root workspace or within this directory:

```bash
cargo run -p linux
```

If your system's `qmake` is named `qmake-qt5` (e.g. on Fedora), explicitly provide the path:
```bash
QMAKE=/usr/bin/qmake-qt5 cargo run -p linux
```

If you encounter wayland/xcb display errors, the app defaults to Wayland if available, falling back to xcb. You can override it by setting:
```bash
QT_QPA_PLATFORM=xcb cargo run -p linux
```
