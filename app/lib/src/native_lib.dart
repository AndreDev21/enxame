import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

/// Abre a biblioteca nativa (`enxame-ffi`). No app empacotado ela fica em `lib/` ao lado do
/// executável; em desenvolvimento, `ENXAME_FFI_LIB` ou `target/release` do repositório.
Future<ExternalLibrary> openEnxameLib() async {
  final name = Platform.isWindows ? 'enxame_ffi.dll' : 'libenxame_ffi.so';
  final exeDir = File(Platform.resolvedExecutable).parent.path;
  final sep = Platform.pathSeparator;
  final candidates = <String>[
    ?Platform.environment['ENXAME_FFI_LIB'],
    '$exeDir${sep}lib$sep$name',
    '$exeDir$sep$name',
    '${Directory.current.path}$sep..${sep}target${sep}release$sep$name',
    '${Directory.current.path}${sep}target${sep}release$sep$name',
  ];
  for (final path in candidates) {
    if (File(path).existsSync()) {
      return ExternalLibrary.open(path);
    }
  }
  // Último recurso: deixa o carregador do sistema procurar.
  return ExternalLibrary.open(name);
}
