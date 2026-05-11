import 'dart:io';

class LinuxRuntimeEnvironment {
  static final LinuxRuntimeEnvironment _instance =
      LinuxRuntimeEnvironment._internal();

  factory LinuxRuntimeEnvironment() => _instance;

  LinuxRuntimeEnvironment._internal() {
    _init();
  }

  bool isLinux = false;
  String xdgSessionType = '';
  String waylandDisplay = '';
  String display = '';
  String gdkBackend = '';
  String gtkImModule = '';
  String qtImModule = '';
  String sdlImModule = '';
  String xmodifiers = '';
  String kdeFullSession = '';
  String desktopSession = '';
  String xdgCurrentDesktop = '';

  void _init() {
    isLinux = Platform.isLinux;
    if (!isLinux) return;

    final env = Platform.environment;
    xdgSessionType = env['XDG_SESSION_TYPE'] ?? '';
    waylandDisplay = env['WAYLAND_DISPLAY'] ?? '';
    display = env['DISPLAY'] ?? '';
    gdkBackend = env['GDK_BACKEND'] ?? '';
    gtkImModule = env['GTK_IM_MODULE'] ?? '';
    qtImModule = env['QT_IM_MODULE'] ?? '';
    sdlImModule = env['SDL_IM_MODULE'] ?? '';
    xmodifiers = env['XMODIFIERS'] ?? '';
    kdeFullSession = env['KDE_FULL_SESSION'] ?? '';
    desktopSession = env['DESKTOP_SESSION'] ?? '';
    xdgCurrentDesktop = env['XDG_CURRENT_DESKTOP'] ?? '';
  }

  bool get isWayland =>
      xdgSessionType.toLowerCase() == 'wayland' || waylandDisplay.isNotEmpty;

  bool get isX11 =>
      xdgSessionType.toLowerCase() == 'x11' ||
      (!isWayland && display.isNotEmpty);

  bool get isLikelyKde =>
      kdeFullSession.isNotEmpty ||
      desktopSession.toLowerCase().contains('plasma') ||
      xdgCurrentDesktop.toLowerCase().contains('kde');

  bool get isFcitxConfigured =>
      gtkImModule == 'fcitx' ||
      qtImModule == 'fcitx' ||
      sdlImModule == 'fcitx' ||
      xmodifiers.contains('fcitx');

  bool get isGtkImModuleForced => gtkImModule.isNotEmpty;
  bool get isQtImModuleForced => qtImModule.isNotEmpty;

  String get summaryText {
    if (!isLinux) return 'Non-Linux';

    String displayType = isWayland ? 'Wayland' : (isX11 ? 'X11' : 'Unknown');
    String desktopType = isLikelyKde ? 'KDE' : 'Unknown/Other';

    return 'Linux | Session: $displayType | Desktop: $desktopType | GTK_IM: $gtkImModule | QT_IM: $qtImModule | XMODIFIERS: $xmodifiers';
  }
}
