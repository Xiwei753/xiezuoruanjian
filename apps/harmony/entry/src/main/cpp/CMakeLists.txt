cmake_minimum_required(VERSION 3.5.0)
project(writer_core_napi)

set(CMAKE_CXX_STANDARD 17)

set(PREBUILT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../prebuilt/${OHOS_ARCH}")
set(PREBUILT_SO "${PREBUILT_DIR}/libwriter_core_ffi.so")

if(NOT EXISTS "${PREBUILT_SO}")
    message(FATAL_ERROR "Missing libwriter_core_ffi.so at ${PREBUILT_SO}. Run tools/build_harmony.sh first.")
endif()

add_library(writer_core_ffi SHARED IMPORTED)
set_target_properties(writer_core_ffi PROPERTIES
    IMPORTED_LOCATION "${PREBUILT_SO}"
)
message(STATUS "Found prebuilt libwriter_core_ffi.so: ${PREBUILT_SO}")

# 鸿蒙目标必须随 HAP 交付 ONNX Runtime 供纠错功能按需加载，而不是借用
# Android/Linux 的 .so。tools/build_harmony.sh 从固定 OHOS 产物获取并写入
# prebuilt/arm64-v8a；它不能成为写作 NAPI 模块的启动时链接依赖。
set(ORT_SO "${PREBUILT_DIR}/libonnxruntime.so")
if(NOT EXISTS "${ORT_SO}")
    message(FATAL_ERROR "Missing OHOS libonnxruntime.so at ${ORT_SO}. Run tools/build_harmony.sh first.")
endif()
message(STATUS "Found OHOS ONNX Runtime for correction: ${ORT_SO}")

# libonnxruntime.so 的 DT_NEEDED 明确包含 libc++_shared.so。CLI 构建脚本和
# hvigor 构建入口都从当前 HarmonyOS Native SDK 准备到同一 PREBUILT_DIR；
# 此处只引用该单一来源，缺失时不得继续生成不完整 HAP。
set(CXX_SHARED_SO "${PREBUILT_DIR}/libc++_shared.so")
if(NOT EXISTS "${CXX_SHARED_SO}")
    message(FATAL_ERROR "Missing required HarmonyOS libc++_shared.so at ${CXX_SHARED_SO}. Run tools/build_harmony.sh or let hvigor prepare the native runtime first.")
endif()
add_library(cxx_shared SHARED IMPORTED)
set_target_properties(cxx_shared PROPERTIES
    IMPORTED_LOCATION "${CXX_SHARED_SO}"
)
message(STATUS "Found required libc++_shared.so: ${CXX_SHARED_SO}")

# napi_init.cpp 通过 #include "napi/napi_xxx.cpp" 内联各域实现，
# 故只需编译 napi_init.cpp 一个翻译单元（与历史结构一致）。
add_library(writer_core_napi SHARED
    corebridge/napi/napi_init.cpp
)

target_include_directories(writer_core_napi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/corebridge
)

target_link_libraries(writer_core_napi PUBLIC
    libace_napi.z.so
    libhilog_ndk.z.so
    librawfile.z.so
    writer_core_ffi
    cxx_shared
)

set_target_properties(writer_core_napi PROPERTIES
    OUTPUT_NAME "writer_core"
)

# ONNX Runtime 由纠错 Rust 代码通过 ort 的 load-dynamic 在实际打开模型时
# 加载。它仍作为 HAP 资源交付，但不进入 libwriter_core.so 的 DT_NEEDED。
add_custom_command(TARGET writer_core_napi POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${ORT_SO}" "$<TARGET_FILE_DIR:writer_core_napi>/libonnxruntime.so"
    COMMENT "Packaging OHOS libonnxruntime.so for on-demand correction"
)
# Keep the SDK-matched C++ runtime in the HAP for the native module and ORT.
add_custom_command(TARGET writer_core_napi POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CXX_SHARED_SO}" "$<TARGET_FILE_DIR:writer_core_napi>/libc++_shared.so"
    COMMENT "Copying the canonical libc++_shared.so into HAP native libraries"
)

# Audit the just-linked NAPI binary as well as its packaged native dependencies.
# CMake's Harmony toolchain supplies the target sysroot; the script resolves
# llvm-readelf beside that sysroot and reports providers from the SDK/package.
if(UNIX)
    get_filename_component(REPO_ROOT
        "${CMAKE_CURRENT_SOURCE_DIR}/../../../../../../"
        ABSOLUTE
    )
    set(HARMONY_RUNTIME_SCRIPT "${REPO_ROOT}/tools/prepare_harmony_native_runtime.sh")
    if(CMAKE_SYSROOT)
        add_custom_command(TARGET writer_core_napi POST_BUILD
            COMMAND bash "${HARMONY_RUNTIME_SCRIPT}" --diagnose-only
                "${PREBUILT_DIR}" "$<TARGET_FILE:writer_core_napi>" "${CMAKE_SYSROOT}"
            COMMENT "Diagnosing HarmonyOS native dynamic dependencies"
            VERBATIM
        )
    else()
        add_custom_command(TARGET writer_core_napi POST_BUILD
            COMMAND bash "${HARMONY_RUNTIME_SCRIPT}" --diagnose-only
                "${PREBUILT_DIR}" "$<TARGET_FILE:writer_core_napi>"
            COMMENT "Diagnosing HarmonyOS native dynamic dependencies"
            VERBATIM
        )
    endif()
endif()
