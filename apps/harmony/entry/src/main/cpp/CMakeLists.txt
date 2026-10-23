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

# Issue #985：鸿蒙目标必须随 HAP 交付可加载的 ONNX Runtime，而不是借用
# Android/Linux 的 .so。tools/build_harmony.sh 从固定 OHOS 产物获取并写入
# prebuilt/arm64-v8a；链接后由打包器复制到 libs/arm64-v8a。
set(ORT_SO "${PREBUILT_DIR}/libonnxruntime.so")
if(NOT EXISTS "${ORT_SO}")
    message(FATAL_ERROR "Missing OHOS libonnxruntime.so at ${ORT_SO}. Run tools/build_harmony.sh first.")
endif()
add_library(onnxruntime_ohos SHARED IMPORTED)
set_target_properties(onnxruntime_ohos PROPERTIES
    IMPORTED_LOCATION "${ORT_SO}"
)
message(STATUS "Found OHOS ONNX Runtime: ${ORT_SO}")

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
    onnxruntime_ohos
)

set_target_properties(writer_core_napi PROPERTIES
    OUTPUT_NAME "writer_core"
)

# 链接不会自动把依赖 so 放入 HAP 的 libs 目录；显式复制到最终模块目录。
add_custom_command(TARGET writer_core_napi POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${ORT_SO}" "$<TARGET_FILE_DIR:writer_core_napi>/libonnxruntime.so"
    COMMENT "Copying OHOS libonnxruntime.so into HAP native libraries"
)
