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

add_library(writer_core_napi SHARED
    napi_init.cpp
)

target_include_directories(writer_core_napi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

target_link_libraries(writer_core_napi PUBLIC
    libace_napi.z.so
    libhilog_ndk.z.so
    writer_core_ffi
)

set_target_properties(writer_core_napi PROPERTIES
    PREFIX ""
    OUTPUT_NAME "writer_core"
)
