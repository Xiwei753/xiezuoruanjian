cmake_minimum_required(VERSION 3.5.0)
project(writer_core_napi)

set(CMAKE_CXX_STANDARD 17)

set(PREBUILT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../prebuilt/${OHOS_ARCH}")
set(PREBUILT_SO "${PREBUILT_DIR}/libwriter_core_ffi.so")

if(EXISTS "${PREBUILT_SO}")
    add_library(writer_core_ffi SHARED IMPORTED)
    set_target_properties(writer_core_ffi PROPERTIES
        IMPORTED_LOCATION "${PREBUILT_SO}"
    )
    message(STATUS "Found prebuilt libwriter_core_ffi.so: ${PREBUILT_SO}")
    set(HAS_RUST_CORE TRUE)
else()
    message(WARNING "libwriter_core_ffi.so NOT found at ${PREBUILT_SO}")
    message(WARNING "App will start with mock fallback. Run tools/build_harmony.sh to produce the .so")
    set(HAS_RUST_CORE FALSE)
endif()

add_library(writer_core_napi SHARED
    napi_init.cpp
)

target_include_directories(writer_core_napi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

if(HAS_RUST_CORE)
    target_link_libraries(writer_core_napi PUBLIC
        libace_napi.z.so
        libhilog_ndk.z.so
        writer_core_ffi
    )
else()
    target_link_libraries(writer_core_napi PUBLIC
        libace_napi.z.so
        libhilog_ndk.z.so
    )
endif()

set_target_properties(writer_core_napi PROPERTIES
    PREFIX ""
    OUTPUT_NAME "writer_core"
)
