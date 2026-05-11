#!/bin/bash
echo "Environment before:"
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
echo "GTK_IM_MODULE=$GTK_IM_MODULE"
echo "QT_IM_MODULE=$QT_IM_MODULE"
echo "SDL_IM_MODULE=$SDL_IM_MODULE"
echo "XMODIFIERS=$XMODIFIERS"
echo "GDK_BACKEND=$GDK_BACKEND"

unset GTK_IM_MODULE
unset QT_IM_MODULE
unset SDL_IM_MODULE
export XMODIFIERS=@im=fcitx

flutter run -d linux
