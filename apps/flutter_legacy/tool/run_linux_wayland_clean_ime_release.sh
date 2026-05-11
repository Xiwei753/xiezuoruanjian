#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

unset GTK_IM_MODULE
unset QT_IM_MODULE
unset SDL_IM_MODULE
export XMODIFIERS=@im=fcitx

flutter run -d linux --release
