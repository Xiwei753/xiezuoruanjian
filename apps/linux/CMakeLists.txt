cmake_minimum_required(VERSION 3.16)
project(writer_linux_native)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Placeholder for Qt and Rust bindings integration
# find_package(Qt5 COMPONENTS Core Gui Widgets REQUIRED)

add_executable(writer_linux_native src/main.cpp)

# target_link_libraries(writer_linux_native Qt5::Core Qt5::Gui Qt5::Widgets)
