import 'package:flutter_webrtc/flutter_webrtc.dart';

import 'calls.dart';

/// Microfone, câmera e tela de verdade (`getUserMedia` / `getDisplayMedia`).
class DeviceMedia implements MediaProvider {
  DeviceMedia({required this.pickScreen});

  /// Deixa o usuário escolher a tela ou janela; `null` = cancelou.
  final Future<DesktopCapturerSource?> Function() pickScreen;

  @override
  Future<MediaStream?> microphone() =>
      navigator.mediaDevices.getUserMedia({'audio': true, 'video': false});

  @override
  Future<MediaStream?> camera() => navigator.mediaDevices.getUserMedia({
    'audio': true,
    'video': {'facingMode': 'user'},
  });

  @override
  Future<MediaStream?> screen() async {
    final source = await pickScreen();
    if (source == null) return null;
    return navigator.mediaDevices.getDisplayMedia({
      'video': {
        'deviceId': {'exact': source.id},
        'mandatory': {'frameRate': 15},
      },
      'audio': false,
    });
  }
}
