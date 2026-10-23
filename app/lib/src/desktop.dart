import 'dart:async';
import 'dart:io';

import 'package:dbus/dbus.dart';
import 'package:flutter/foundation.dart';
import 'package:launch_at_startup/launch_at_startup.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

import '../l10n/app_localizations.dart';
import 'prefs.dart';
import 'state.dart';

/// Integração com a área de trabalho: fechar a janela mantém o nó ativo na bandeja, menu da
/// bandeja (status, modo jogo, contribuição, sair) e abrir com o sistema (opt-in).
class DesktopShell with WindowListener {
  DesktopShell(this.state, this.prefs);

  final AppState state;
  final UiPrefs prefs;
  L? _l;
  TrayIcon? _tray;
  final List<MenuItem> _items = [];
  bool _quitting = false;

  /// A bandeja foi criada (se o ambiente a suporta, o ícone aparece).
  bool get hasTray => _tray != null;

  Future<void> init(L l) async {
    _l = l;
    await windowManager.ensureInitialized();
    await windowManager.setPreventClose(true);
    windowManager.addListener(this);
    // Num AppImage o executável fica num ponto de montagem que muda a cada execução: o caminho
    // estável é o do próprio arquivo `.AppImage` (variável `APPIMAGE` do runtime).
    LaunchAtStartup.instance.setup(
      appName: 'Enxame',
      appPath: Platform.environment['APPIMAGE'] ?? Platform.resolvedExecutable,
    );
    await _createTray();
    state.addListener(_rebuildMenu);
  }

  void updateLocale(L l) {
    _l = l;
    _rebuildMenu();
  }

  Future<bool> get autostartEnabled => LaunchAtStartup.instance.isEnabled();

  Future<void> setAutostart(bool on) => on
      ? LaunchAtStartup.instance.enable()
      : LaunchAtStartup.instance.disable();

  // ------------------------------------------------------------------ janela

  @override
  Future<void> onWindowClose() async {
    if (_quitting) return;
    if (prefs.closeToTray && hasTray) {
      await windowManager.hide();
    } else {
      await quit();
    }
  }

  Future<void> show() async {
    await windowManager.show();
    await windowManager.focus();
  }

  Future<void> quit() async {
    _quitting = true;
    _tray?.setVisible(false);
    // O nó em Rust e o WebRTC mantêm threads próprias: fechar a janela não encerra o processo
    // (visto no campo, Linux e Windows). Para o nó com prazo, fecha a janela e encerra de vez.
    try {
      await state.stop().timeout(const Duration(seconds: 5));
    } catch (e) {
      debugPrint('o nó não parou a tempo: $e');
    }
    try {
      await windowManager.destroy().timeout(const Duration(seconds: 2));
    } catch (_) {}
    exit(0);
  }

  // ------------------------------------------------------------------ bandeja

  /// No Linux o ícone só aparece se houver um host de bandeja (StatusNotifierWatcher no D-Bus:
  /// KDE, waybar, GNOME com a extensão AppIndicator...). Sem ele, esconder a janela a deixaria
  /// sem volta: nesse caso não há bandeja e fechar a janela encerra o app.
  Future<bool> _trayHostAvailable() async {
    if (!Platform.isLinux) return true;
    final bus = DBusClient.session();
    try {
      final names = await bus.listNames();
      return names.contains('org.kde.StatusNotifierWatcher') ||
          names.contains('org.freedesktop.StatusNotifierWatcher');
    } catch (_) {
      return false;
    } finally {
      await bus.close();
    }
  }

  Future<void> _createTray() async {
    if (!await _trayHostAvailable()) {
      debugPrint('sem host de bandeja: fechar a janela encerra o app');
      return;
    }
    try {
      final tray = TrayIcon.create();
      if (tray == null) return;
      tray.icon = ImageAsset.fromAsset('assets/tray_icon.png');
      tray.setTooltip('Enxame');
      tray.addListener((e) {
        if (e is TrayIconClickedEvent) show();
      });
      tray.setVisible(true);
      _tray = tray;
      _rebuildMenu();
    } catch (e) {
      debugPrint('bandeja indisponível: $e');
    }
  }

  void _rebuildMenu() {
    final tray = _tray;
    final l = _l;
    if (tray == null || l == null) return;
    for (final i in _items) {
      i.dispose();
    }
    _items.clear();
    final menu = Menu.create();
    if (menu == null) return;

    MenuItem? item(
      String label,
      MenuItemType type, {
      bool enabled = true,
      bool checked = false,
      void Function()? onTap,
    }) {
      final it = MenuItem.createWithLabelAndType(label, type);
      if (it == null) return null;
      it.isEnabled = enabled;
      if (type == MenuItemType.checkbox) {
        it.state = checked ? MenuItemState.checked : MenuItemState.unchecked;
      }
      if (onTap != null) {
        it.addListener((e) {
          if (e is MenuItemClickedEvent) onTap();
        });
      }
      _items.add(it);
      return it;
    }

    final role = state.me?.role ?? '';
    menu.addItem(item(l.trayRole(role), MenuItemType.normal, enabled: false));
    menu.addItem(item(l.trayOpen, MenuItemType.normal, onTap: show));
    menu.addSeparator();
    menu.addItem(
      item(
        l.gameMode,
        MenuItemType.checkbox,
        checked: state.gameMode,
        onTap: () => state.setGameMode(!state.gameMode),
      ),
    );
    menu.addItem(
      item(
        l.contribute,
        MenuItemType.checkbox,
        checked: state.contributing,
        onTap: () => state.setContribution(!state.contributing),
      ),
    );
    menu.addSeparator();
    menu.addItem(item(l.trayQuit, MenuItemType.normal, onTap: quit));
    tray.setContextMenu(menu);
  }
}
