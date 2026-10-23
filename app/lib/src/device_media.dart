import 'package:flutter_webrtc/flutter_webrtc.dart';

import 'calls.dart';

/// Microfone, câmera e tela de verdade (`getUserMedia` / `getDisplayMedia`).
class DeviceMedia implements MediaProvider {
  DeviceMedia({required this.pickScreen});

  /// Deixa o usuário escolher a tela ou janela; `null` = cancelou.
  final Future<DesktopCapturerSource?> Function() pickScreen;

  /// Quais tipos de entrada o sistema lista. No Windows, pedir `getUserMedia` de um dispositivo que
  /// não existe pode derrubar o app inteiro: só se pede o que foi listado. Se a listagem falhar,
  /// segue-se como antes (tenta e deixa o erro subir).
  Future<({bool audio, bool video})> _inputs() async {
    try {
      final devices = await navigator.mediaDevices.enumerateDevices();
      return (
        audio: devices.any((d) => d.kind == 'audioinput'),
        video: devices.any((d) => d.kind == 'videoinput'),
      );
    } catch (_) {
      return (audio: true, video: true);
    }
  }

  @override
  Future<MediaStream?> microphone() async {
    if (!(await _inputs()).audio) {
      throw StateError('este computador não tem microfone');
    }
    return navigator.mediaDevices.getUserMedia({'audio': true, 'video': false});
  }

  @override
  Future<MediaStream?> camera() async {
    final inputs = await _inputs();
    if (!inputs.video) throw StateError('este computador não tem câmera');
    return navigator.mediaDevices.getUserMedia({
      'audio': inputs.audio,
      'video': {'facingMode': 'user'},
    });
  }

  @override
  Future<MediaStream?> screen() async {
    final source = await pickScreen();
    if (source == null) return null;
    return navigator.mediaDevices.getDisplayMedia({
      'video': {
        'deviceId': {'exact': source.id},
        'mandatory': {'frameRate': 30, 'minFrameRate': 30},
      },
      'audio': false,
    });
  }
}
