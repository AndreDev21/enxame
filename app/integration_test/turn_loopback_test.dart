// Spike do 7.2 (etapa 1): conexões WebRTC passando por servidores TURN em 127.0.0.1 cujo relay liga
// um lado ao outro (scripts/spike/turn-loopback), sem iroh. Responde: o libwebrtc aceita TURN em
// loopback com endereços virtuais, conecta só por `relay` e, havendo caminho direto, prefere o
// direto? Plano: docs/plano-fase7-2-midia-sobre-iroh.md.
// Antes: (cd scripts/spike/turn-loopback && cargo build --release)
// Uso: flutter test integration_test/turn_loopback_test.dart -d linux
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:integration_test/integration_test.dart';

String _bin() {
  for (final dir in ['..', '../..', '.']) {
    final f = File(
      '$dir/scripts/spike/turn-loopback/target/release/turn-loopback-spike',
    );
    if (f.existsSync()) return f.path;
  }
  throw StateError(
    'compile o spike: cargo build --release em scripts/spike/turn-loopback',
  );
}

Map<String, dynamic> _ice(Map<String, dynamic> side, String policy) => {
  'iceServers': [
    {
      'urls': 'turn:127.0.0.1:${side['port']}?transport=udp',
      'username': side['user'],
      'credential': side['pass'],
    },
  ],
  'iceTransportPolicy': policy, // `relay`: nenhum caminho real é permitido
  'sdpSemantics': 'unified-plan',
};

/// Conecta A e B pelos TURN em loopback com a política dada, troca uma mensagem pelo canal de dados
/// e devolve os candidatos de A e os tipos dos pares nomeados (`local/remoto`).
Future<({List<String> candA, List<String> types})> connectPair(
  String policy, {
  Duration settle = Duration.zero,
}) async {
  final proc = await Process.start(_bin(), const []);
  addTearDown(() async {
    proc.stdin.close();
    await proc.exitCode.timeout(
      const Duration(seconds: 3),
      onTimeout: () {
        proc.kill();
        return -1;
      },
    );
  });
  unawaited(
    proc.stderr.transform(utf8.decoder).forEach((s) => debugPrint('[turn] $s')),
  );
  final line = await proc.stdout
      .transform(utf8.decoder)
      .transform(const LineSplitter())
      .first
      .timeout(const Duration(seconds: 10));
  final cfg = jsonDecode(line) as Map<String, dynamic>;

  final a = await createPeerConnection(
    _ice(cfg['a'] as Map<String, dynamic>, policy),
  );
  final b = await createPeerConnection(
    _ice(cfg['b'] as Map<String, dynamic>, policy),
  );
  addTearDown(() async {
    await a.close();
    await b.close();
  });

  final candA = <String>[], candB = <String>[];
  final pendingA = <RTCIceCandidate>[], pendingB = <RTCIceCandidate>[];
  var remoteSetA = false, remoteSetB = false;
  a.onIceCandidate = (c) {
    if (c.candidate == null || c.candidate!.isEmpty) return;
    candA.add(c.candidate!);
    remoteSetB ? b.addCandidate(c) : pendingB.add(c);
  };
  b.onIceCandidate = (c) {
    if (c.candidate == null || c.candidate!.isEmpty) return;
    candB.add(c.candidate!);
    remoteSetA ? a.addCandidate(c) : pendingA.add(c);
  };

  final openA = Completer<void>(), openB = Completer<void>();
  final got = Completer<String>();
  final dcA = await a.createDataChannel('spike', RTCDataChannelInit());
  dcA.onDataChannelState = (s) {
    if (s == RTCDataChannelState.RTCDataChannelOpen && !openA.isCompleted) {
      openA.complete();
    }
  };
  b.onDataChannel = (dc) {
    dc.onDataChannelState = (s) {
      if (s == RTCDataChannelState.RTCDataChannelOpen && !openB.isCompleted) {
        openB.complete();
      }
    };
    dc.onMessage = (m) {
      if (!got.isCompleted) got.complete(m.text);
    };
  };

  final offer = await a.createOffer();
  await a.setLocalDescription(offer);
  await b.setRemoteDescription(offer);
  remoteSetB = true;
  for (final c in List.of(pendingB)) {
    await b.addCandidate(c);
  }
  final answer = await b.createAnswer();
  await b.setLocalDescription(answer);
  await a.setRemoteDescription(answer);
  remoteSetA = true;
  for (final c in List.of(pendingA)) {
    await a.addCandidate(c);
  }

  await Future.wait([openA.future, openB.future]).timeout(
    const Duration(seconds: 20),
    onTimeout: () => fail(
      'o canal de dados não abriu em 20 s; candidatos de A: $candA; de B: $candB',
    ),
  );
  await dcA.send(RTCDataChannelMessage('olá pelo TURN em loopback'));
  expect(
    await got.future.timeout(const Duration(seconds: 5)),
    'olá pelo TURN em loopback',
  );
  await Future<void>.delayed(settle);

  final stats = await a.getStats();
  final byId = {for (final r in stats) r.id: r};
  final types = <String>[];
  for (final r in stats.where((r) => r.type == 'candidate-pair')) {
    if (r.values['state'] == 'succeeded' && r.values['nominated'] == true) {
      types.add(
        '${byId[r.values['localCandidateId']]?.values['candidateType']}/'
        '${byId[r.values['remoteCandidateId']]?.values['candidateType']}',
      );
    }
  }
  debugPrint(
    '[spike] política $policy: pares nomeados $types; '
    'candidatos A: ${candA.length}, B: ${candB.length}',
  );
  return (candA: candA, types: types);
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'duas conexões WebRTC conectam só por TURN em loopback e trocam dados',
    (tester) async {
      final r = await connectPair('relay');
      expect(r.candA, isNotEmpty);
      expect(
        r.candA.every((c) => c.contains(' typ relay')),
        isTrue,
        reason: '${r.candA}',
      );
      expect(r.types, isNotEmpty);
      expect(
        r.types.every((t) => t.startsWith('relay')),
        isTrue,
        reason: '${r.types}',
      );
    },
    timeout: const Timeout(Duration(minutes: 2)),
  );

  testWidgets(
    'com candidatos diretos disponíveis o ICE termina no direto, não no relay',
    (tester) async {
      final r = await connectPair('all', settle: const Duration(seconds: 4));
      expect(r.types, isNotEmpty);
      expect(
        r.types.any((t) => t.startsWith('relay') || t.endsWith('/relay')),
        isFalse,
        reason: 'o par nomeado deveria ser direto: ${r.types}',
      );
    },
    timeout: const Timeout(Duration(minutes: 2)),
  );
}
