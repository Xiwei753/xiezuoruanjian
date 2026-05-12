#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

echo "Building Linux Native..."
cd apps/linux
mkdir -p build && cd build
cmake ..
make
