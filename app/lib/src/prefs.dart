import 'dart:convert';
import 'dart:io';

/// Preferências só da interface (as do núcleo ficam em `config.toml`). Guardadas em
/// `<dados>/ui.json`.
class UiPrefs {
  UiPrefs._(this._file, this.closeToTray);
  final File _file;
  bool closeToTray;

  static Future<UiPrefs> load(String dataDir) async {
    final file = File('$dataDir${Platform.pathSeparator}ui.json');
    var close = true; // padrão: fechar a janela mantém o nó ativo
    try {
      final j = jsonDecode(await file.readAsString()) as Map<String, dynamic>;
      close = (j['closeToTray'] as bool?) ?? true;
    } catch (_) {}
    return UiPrefs._(file, close);
  }

  Future<void> save() async {
    await _file.parent.create(recursive: true);
    await _file.writeAsString(jsonEncode({'closeToTray': closeToTray}));
  }
}
