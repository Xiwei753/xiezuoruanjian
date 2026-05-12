# Linux Native Client

This directory contains the placeholder structure for the Linux native client.

## Architecture Note
This client **MUST** use the `core/writer_core` Rust library via bindings (FFI). It is strictly forbidden to implement workspace formats, save logic, or syncing directly in C++.
The future implementation of this client will utilize Qt/C++ (or Qt/QML) for its UI, while relying on the Rust core for all business logic.

## Building
Currently, this is just a stub.
```bash
mkdir build
cd build
cmake ..
make
```
