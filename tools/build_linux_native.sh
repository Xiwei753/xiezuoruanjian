#!/bin/bash
echo "Building Linux Native..."
cd apps/linux_native
mkdir -p build && cd build
cmake ..
make
