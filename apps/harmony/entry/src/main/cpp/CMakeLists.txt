cmake_minimum_required(VERSION 3.5.0)
project(writer_core_napi)

set(CMAKE_CXX_STANDARD 17)

add_library(writer_core SHARED IMPORTED)
set_target_properties(writer_core PROPERTIES
    IMPORTED_LOCATION ${CMAKE_CURRENT_SOURCE_DIR}/../../../prebuilt/${OHOS_ARCH}/libwriter_core.so
)

add_library(writer_core_napi SHARED
    napi_init.cpp
    writer_core_napi.cpp
)

target_include_directories(writer_core_napi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

target_link_libraries(writer_core_napi PUBLIC
    libace_napi.z.so
    libhilog_ndk.z.so
    writer_core
)

set_target_properties(writer_core_napi PROPERTIES
    PREFIX ""
    OUTPUT_NAME "writer_core"
)
