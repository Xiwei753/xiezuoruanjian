# libwriter_core_ffi.so placeholder

This directory is where the cross-compiled Rust `libwriter_core_ffi.so` should be placed.

To build the .so file, run:

```bash
./tools/build_harmony.sh
```

Requirements:
- Rust toolchain with `aarch64-unknown-linux-ohos` target
- OHOS NDK (set `OHOS_NDK_HOME` environment variable)

If the .so is not present, the DevEco build will proceed without linking
the Rust core library. The NAPI shim will still compile but native calls
will fail at runtime.