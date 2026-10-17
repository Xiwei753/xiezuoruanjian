//! 构建脚本（Issue #924）。
//!
//! 为 HarmonyOS 目标编译 HUKS C shim（`src/huks_storage.c`）并链接系统
//! `libhuks_ndk.z.so`；非 HarmonyOS 目标（开发机 `cargo check`/`test`）
//! 不编译 C shim，Rust 侧 `secure_storage` 在非 ohos 目标使用显式失败的占位实现。

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/huks_storage.c");
    println!("cargo:rerun-if-changed=src/huks_storage.h");
    println!("cargo:rerun-if-env-changed=OHOS_NDK_HOME");
    println!("cargo:rerun-if-env-changed=HARMONY_CLI_HOME");
    println!("cargo:rerun-if-env-changed=SUJIAN_HUKS_SYSROOT");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("ohos") {
        return;
    }

    let sysroot = detect_sysroot();
    if sysroot.is_none() {
        println!(
            "cargo:warning=HarmonyOS sysroot not found; \
             relying on --sysroot from the toolchain environment (tools/build_harmony.sh)"
        );
    }

    let mut build = cc::Build::new();
    build.file("src/huks_storage.c");
    build.flag_if_supported("-fPIC");
    build.flag_if_supported("-Wall");
    if let Some(sysroot) = &sysroot {
        build.include(sysroot.join("usr/include"));
        // HUKS NDK 头文件在 sysroot 的 huks/ 子目录下（native_huks_api.h 等）。
        build.include(sysroot.join("usr/include/huks"));
    }
    build.compile("sujian_huks_storage");

    // HUKS NDK 公开接口：libhuks_ndk.z.so（设备系统库）。
    println!("cargo:rustc-link-lib=dylib=huks_ndk.z");
    if let Some(sysroot) = &sysroot {
        // 当前只支持 arm64-v8a，与 tools/build_harmony.sh 的产物架构一致。
        println!(
            "cargo:rustc-link-search=native={}",
            sysroot.join("usr/lib/aarch64-linux-ohos").display()
        );
    }
}

/// 按 `tools/build_harmony.sh` 的同一顺序探测 HarmonyOS Native SDK sysroot。
fn detect_sysroot() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("SUJIAN_HUKS_SYSROOT") {
        let path = PathBuf::from(explicit);
        if path.join("usr/include/huks").is_dir() {
            return Some(path);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(ndk_home) = std::env::var("OHOS_NDK_HOME") {
        candidates.push(PathBuf::from(ndk_home));
    }
    let cli_home = std::env::var("HARMONY_CLI_HOME").ok().map_or_else(
        || {
            std::env::var("HOME").map_or_else(
                |_| PathBuf::from(".harmony-cli"),
                |home| PathBuf::from(home).join(".harmony-cli"),
            )
        },
        PathBuf::from,
    );
    candidates.push(cli_home.join("sdk/default/openharmony/native"));
    candidates.push(PathBuf::from(
        "/opt/devecostudio/sdk/default/openharmony/native",
    ));

    for native_dir in candidates {
        let sysroot = native_dir.join("sysroot");
        if sysroot.join("usr/include/huks").is_dir() {
            return Some(sysroot);
        }
    }
    None
}
