import 'package:flutter/foundation.dart';

import 'dart:io';

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
      debugPrint(
        '[call] dispositivos: ${devices.map((d) => '${d.kind}:${d.label}').toList()}',
      );
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

  /// No Linux o libwebrtc embutido não decodifica MJPEG ("Failed to convert capture frame from
  /// type 11 to I420"): câmeras USB que só dão 30 fps em MJPEG ficam sem enviar nenhum quadro.
  /// Pedir 640x480 a ~20 fps faz o V4L2 escolher YUYV, que a maioria das câmeras oferece nessa
  /// resolução. Nos outros sistemas fica o padrão.
  static Map<String, dynamic> _cameraConstraints() => Platform.isLinux
      ? {
          'facingMode': 'user',
          'mandatory': {
            'minWidth': '640',
            'maxWidth': '640',
            'minHeight': '480',
            'maxHeight': '480',
            'minFrameRate': '15',
            'maxFrameRate': '24',
          },
        }
      : {'facingMode': 'user'};

  @override
  Future<MediaStream?> camera() async {
    final inputs = await _inputs();
    if (!inputs.video) throw StateError('este computador não tem câmera');
    final stream = await navigator.mediaDevices.getUserMedia({
      'audio': inputs.audio,
      'video': _cameraConstraints(),
    });
    final v = stream.getVideoTracks().firstOrNull;
    debugPrint('[call] câmera aberta: ${v?.getSettings()}');
    return stream;
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
