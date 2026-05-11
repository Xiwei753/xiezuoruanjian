#!/bin/bash
echo "Building Linux Native..."
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/../apps/linux_native"
mkdir -p build && cd build
cmake ..
make
