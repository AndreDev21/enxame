// Protótipo do bloco 6.4: captura a tela de verdade e leva os quadros por WebRTC até um segundo
// RTCPeerConnection no mesmo processo (sem rede externa), conferindo no receptor que há vídeo
// decodificado. Prova que o flutter_webrtc captura/codifica/decodifica neste sistema.
// Uso: flutter test integration_test/webrtc_test.dart -d linux
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('tela capturada chega decodificada ao outro lado', (
    tester,
  ) async {
    final sources = await desktopCapturer.getSources(
      types: [SourceType.Screen],
    );
    expect(sources, isNotEmpty, reason: 'nenhuma tela para capturar');
    debugPrint0(
      'fontes: ${sources.map((s) => '${s.id}:${s.name}').join(', ')}',
    );

    final stream = await navigator.mediaDevices.getDisplayMedia({
      'video': {
        'deviceId': {'exact': sources.first.id},
        'mandatory': {'frameRate': 15},
      },
      'audio': false,
    });
    expect(stream.getVideoTracks(), isNotEmpty);

    final a = await createPeerConnection({'sdpSemantics': 'unified-plan'});
    final b = await createPeerConnection({'sdpSemantics': 'unified-plan'});
    a.onIceCandidate = (c) => b.addCandidate(c);
    b.onIceCandidate = (c) => a.addCandidate(c);
    final gotTrack = Completer<MediaStreamTrack>();
    b.onTrack = (e) {
      if (!gotTrack.isCompleted) gotTrack.complete(e.track);
    };

    for (final t in stream.getVideoTracks()) {
      await a.addTrack(t, stream);
    }
    final offer = await a.createOffer();
    await a.setLocalDescription(offer);
    await b.setRemoteDescription(offer);
    final answer = await b.createAnswer();
    await b.setLocalDescription(answer);
    await a.setRemoteDescription(answer);

    final track = await gotTrack.future.timeout(const Duration(seconds: 20));
    expect(track.kind, 'video');

    // Espera quadros decodificados no receptor.
    num frames = 0;
    num width = 0;
    for (var i = 0; i < 30 && frames < 5; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 500));
      for (final r in await b.getStats()) {
        if (r.type == 'inbound-rtp' && r.values['kind'] == 'video') {
          frames = (r.values['framesDecoded'] as num?) ?? 0;
          width = (r.values['frameWidth'] as num?) ?? 0;
        }
      }
    }
    debugPrint0('quadros decodificados: $frames, largura: $width');
    expect(frames, greaterThanOrEqualTo(5));
    expect(width, greaterThan(0));

    for (final t in stream.getTracks()) {
      await t.stop();
    }
    await a.close();
    await b.close();
  }, timeout: const Timeout(Duration(minutes: 3)));
}

void debugPrint0(String s) => print('[webrtc] $s'); // ignore: avoid_print
