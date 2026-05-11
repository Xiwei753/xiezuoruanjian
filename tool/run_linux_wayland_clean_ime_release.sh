#!/bin/bash
unset GTK_IM_MODULE
unset QT_IM_MODULE
unset SDL_IM_MODULE
export XMODIFIERS=@im=fcitx

flutter run -d linux --release
