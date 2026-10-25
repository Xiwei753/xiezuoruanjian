# ONNX Runtime for HarmonyOS arm64

This directory contains the pinned `libonnxruntime.so` used by the HarmonyOS
arm64 build. The library was built from the official Microsoft ONNX Runtime
1.26.0 source at commit `8c546c37b43caaca1fa25db430dab94b901cf277` with
HarmonyOS Native SDK 26.0.0.105 and HarmonyOS Clang 15.0.4 for
`aarch64-linux-ohos26.0.0`.

The compatibility patch applied to upstream is
[`onnxruntime-ohos-arm64-v8a-1.26.0-harmony-sdk26.patch`](onnxruntime-ohos-arm64-v8a-1.26.0-harmony-sdk26.patch).
The build profile and runtime dependencies are recorded in
[`BUILDINFO.txt`](BUILDINFO.txt). The profile retains the CPU provider and
disables the BF16 and NEON FP16 acceleration paths that this compiler cannot
build.

The checked-in library is 16,744,224 bytes with SHA-256
`10130b64cf38fc5f55c4009b1baaa5c7b7b72ab16907e4623c7dd5b2735130c5`.
`crates/sujian-proof/assets/model_manifest.json` pins these values and the
repository-relative path. The original release archive SHA-256 was
`2c2acc6ac083404b679d43ac539bbc1e0e85fa76ed0d420c76b0fbdff9793d10`.

ONNX Runtime is MIT licensed; the repository's existing
[`LICENSE`](../LICENSE) and [`NOTICE`](../NOTICE) apply. Device-side
loading has not been verified because no HarmonyOS device was connected for
the build.
