// Chamada 1:1 de ponta a ponta no mesmo processo: dois CallController trocam os sinais (convite,
// aceite, oferta, resposta, ICE) por um transporte em memória e a mídia vai por WebRTC de verdade.
// Cobre a transmissão de tela: a tela real é capturada pelo chamador e decodificada pelo outro lado.
// Uso: flutter test integration_test/call_test.dart -d linux
import 'dart:async';
import 'dart:io';

import 'package:enxame_app/src/callmetrics.dart';
import 'package:enxame_app/src/calls.dart';
import 'package:enxame_app/src/device_media.dart';
import 'package:enxame_app/src/netprobe.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:integration_test/integration_test.dart';

class _Wire implements SignalTransport {
  _Wire(this.self);
  final String self;
  late _Wire other;
  final _in = StreamController<IncomingSignal>.broadcast();
  final sent = <Map<String, dynamic>>[];

  /// Descarta os candidatos ICE da primeira tentativa: a conexão não abre e os papéis se invertem.
  bool dropFirstAttemptIce = false;

  /// Descarta todos os candidatos ICE: nenhuma tentativa conecta.
  bool dropAllIce = false;

  @override
  Stream<IncomingSignal> get incoming => _in.stream;

  @override
  Future<void> send(String peer, Map<String, dynamic> msg) async {
    sent.add(msg);
    if (msg['t'] == 'ice' &&
        (dropAllIce ||
            (dropFirstAttemptIce && ((msg['a'] as int?) ?? 0) == 0))) {
      return;
    }
    // Passa por JSON como no app e entrega depois de um ciclo (rede de verdade é assíncrona).
    final copy = decodeSignal(encodeSignal(msg))!;
    Future<void>.delayed(const Duration(milliseconds: 5), () {
      other._in.add(IncomingSignal(self, 'Pessoa $self', copy));
    });
  }
}

/// Ponte de mentira: anota o que o controlador pede; o TURN que devolve não existe (o ICE ignora).
class _FakeBridge implements MediaBridgeProvider {
  _FakeBridge({this.fail = false});
  final bool fail;
  final starts = <bool>[]; // `caller` de cada subida
  int stops = 0;

  @override
  Future<BridgeInfo> start(String peerFp, {required bool caller}) async {
    starts.add(caller);
    if (fail) throw 'sem núcleo';
    return const BridgeInfo('turn:127.0.0.1:9?transport=udp', 'u', 'p');
  }

  @override
  Future<void> stop() async => stops++;

  @override
  Future<BridgeStatus?> status() async => const BridgeStatus(
    linked: true,
    direct: true,
    maxDatagram: 1414,
    sent: 3,
    received: 2,
    droppedTooLarge: 0,
    droppedOther: 0,
  );
}

Future<void> until(bool Function() ok, {int seconds = 30, String? why}) async {
  for (var i = 0; i < seconds * 10 && !ok(); i++) {
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  expect(ok(), isTrue, reason: why);
}

class _MicOnly extends DeviceMedia {
  _MicOnly({required super.pickScreen});

  @override
  Future<MediaStream?> camera() => microphone();
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  Future<DesktopCapturerSource?> firstScreen() async {
    final s = await desktopCapturer.getSources(types: [SourceType.Screen]);
    return s.isEmpty ? null : s.first;
  }

  late _Wire wa;
  late _Wire wb;
  late CallController a;
  late CallController b;

  setUp(() {
    wa = _Wire('A');
    wb = _Wire('B');
    wa.other = wb;
    wb.other = wa;
    // Sem microfone/câmera nos testes: só a tela é capturada de verdade.
    a = CallController(
      transport: wa,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
    );
    b = CallController(
      transport: wb,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
    );
  });

  tearDown(() async {
    await a.hangup();
    await b.hangup();
    a.dispose();
    b.dispose();
  });

  testWidgets('vídeo: a câmera real de A chega a B', (tester) async {
    // Depende de hardware (algumas webcams USB travam depois de abrir e fechar): só com ENXAME_TEST_CAMERA=1.
    final devices = await navigator.mediaDevices.enumerateDevices();
    if (Platform.environment['ENXAME_TEST_CAMERA'] != '1' ||
        !devices.any((d) => d.kind == 'videoinput')) {
      markTestSkipped('sem câmera, ou ENXAME_TEST_CAMERA não definido');
      return;
    }
    // Só A usa a câmera de verdade (ela não abre duas vezes); B manda só o microfone.
    a.dispose();
    b.dispose();
    a = CallController(
      transport: wa,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
    );
    b = CallController(
      transport: wb,
      media: _MicOnly(pickScreen: firstScreen),
      iceServers: const [],
    );
    await a.call('B', 'Pessoa B', CallKind.video);
    await until(() => b.phase == CallPhase.incoming);
    await b.accept();
    await until(
      () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
      why:
          'conectou: A=${a.phase} (${a.endedReason}) B=${b.phase} (${b.endedReason})',
    );
    Future<num> decoded(CallController c) async {
      num frames = 0;
      for (final r in await c.stats()) {
        if (r.type == 'inbound-rtp' && r.values['kind'] == 'video') {
          frames = (r.values['framesDecoded'] as num?) ?? 0;
        }
      }
      return frames;
    }

    num fb = 0;
    for (var i = 0; i < 30 && fb < 5; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 500));
      fb = await decoded(b);
    }
    final out = [
      for (final r in await a.stats())
        if (r.type == 'outbound-rtp' && r.values['kind'] == 'video') r.values,
    ];
    expect(
      fb,
      greaterThanOrEqualTo(5),
      reason:
          'B decodificou a câmera de A; prévia local '
          '${a.localRenderer.videoWidth}x${a.localRenderer.videoHeight}; A enviou $out',
    );
  });

  testWidgets(
    'quem atende está atrás de CGNAT e o chamador não: o atendente oferece na 1ª tentativa',
    (tester) async {
      a.dispose();
      b.dispose();
      a = CallController(
        transport: wa,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        netProbe: () async => const NetHint(why: 'teste'),
      );
      b = CallController(
        transport: wb,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        netProbe: () async => const NetHint(cgnat: true, why: 'teste'),
      );
      await a.call('B', 'Pessoa B', CallKind.screen);
      await until(() => b.phase == CallPhase.incoming);
      await b.accept();
      await until(
        () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
        why:
            'conectou: A=${a.phase} (${a.endedReason}) B=${b.phase} (${b.endedReason})',
      );
      expect(wb.sent.map((m) => m['t']), contains('offer'));
      expect(wa.sent.map((m) => m['t']), isNot(contains('offer')));
      expect(wa.sent.map((m) => m['t']), isNot(contains('swap')));
      expect(wb.sent.firstWhere((m) => m['t'] == 'accept')['cg'], 1);
    },
  );

  testWidgets('a conexão deixa um registro de medição sem IPs (Fase 7.1)', (
    tester,
  ) async {
    final dir = Directory.systemTemp.createTempSync('enxame-metrics-it');
    final log = CallMetricsLog(File('${dir.path}/m.jsonl'));
    a.metrics = log;
    await a.call('B', 'Pessoa B', CallKind.screen);
    await until(() => b.phase == CallPhase.incoming);
    await b.accept();
    await until(
      () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
      why: 'conectou: A=${a.phase} (${a.endedReason}) B=${b.phase}',
    );
    await Future<void>.delayed(
      const Duration(seconds: 2),
    ); // dá tempo da 1ª amostra
    await a.hangup();
    final rows = log.read();
    expect(rows, hasLength(1));
    final r = rows.single;
    expect(r['outcome'], 'connected');
    expect(r['role'], 'caller');
    expect(r['attempts'], 1);
    expect(r['swapped'], false);
    expect(r['ms_to_connect'], isA<int>());
    expect(
      r['ladder'],
      isNot('none'),
      reason: 'o par escolhido foi medido: $r',
    );
    final raw = File('${dir.path}/m.jsonl').readAsStringSync();
    expect(RegExp(r'\d+\.\d+\.\d+\.\d+').hasMatch(raw), isFalse, reason: raw);
  });

  testWidgets(
    'ponte de mídia: sobe um TURN por conexão nos dois lados, derruba no fim e entra na medição',
    (tester) async {
      final fa = _FakeBridge(), fb = _FakeBridge();
      final dir = Directory.systemTemp.createTempSync('enxame-bridge-it');
      final log = CallMetricsLog(File('${dir.path}/m.jsonl'));
      a.dispose();
      b.dispose();
      a = CallController(
        transport: wa,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        bridge: fa,
      )..metrics = log;
      b = CallController(
        transport: wb,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        bridge: fb,
      );
      await a.call('B', 'Pessoa B', CallKind.screen);
      await until(() => b.phase == CallPhase.incoming);
      await b.accept();
      await until(
        () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
        why: 'conectou: A=${a.phase} (${a.endedReason}) B=${b.phase}',
      );
      expect(fa.starts, [true]);
      expect(fb.starts, [false]);
      await Future<void>.delayed(const Duration(seconds: 5));
      await a.hangup();
      await until(() => b.phase == CallPhase.idle);
      expect(fa.stops, 1);
      expect(fb.stops, 1);
      final r = log.read().single;
      expect(r['bridge'], containsPair('max_datagram', 1414));
      expect(r['bridge'], containsPair('iroh_direct', true));
    },
  );

  testWidgets('ponte indisponível: a chamada conecta como antes', (
    tester,
  ) async {
    a.dispose();
    b.dispose();
    a = CallController(
      transport: wa,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
      bridge: _FakeBridge(fail: true),
    );
    b = CallController(
      transport: wb,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
      bridge: _FakeBridge(fail: true),
    );
    await a.call('B', 'Pessoa B', CallKind.screen);
    await until(() => b.phase == CallPhase.incoming);
    await b.accept();
    await until(
      () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
      why: 'conectou: A=${a.phase} (${a.endedReason}) B=${b.phase}',
    );
  });

  testWidgets(
    'sem caminho: a mensagem final diz a causa provável (as duas pontas em CGNAT)',
    (tester) async {
      wa.dropAllIce = true;
      wb.dropAllIce = true;
      a.dispose();
      b.dispose();
      a = CallController(
        transport: wa,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        netProbe: () async => const NetHint(cgnat: true, why: 'teste'),
      );
      b = CallController(
        transport: wb,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: const [],
        netProbe: () async => const NetHint(cgnat: true, why: 'teste'),
      );
      await a.call('B', 'Pessoa B', CallKind.screen);
      await until(() => b.phase == CallPhase.incoming);
      await b.accept();
      await until(
        () => a.phase == CallPhase.idle && b.phase == CallPhase.idle,
        seconds: 90,
        why: 'A=${a.phase} B=${b.phase}',
      );
      expect(wa.sent.map((m) => m['t']), contains('swap'));
      expect(a.endedReason, contains('CGNAT'));
      expect(a.endedReason, contains('Wi-Fi'));
    },
    timeout: const Timeout(Duration(minutes: 3)),
  );

  testWidgets(
    'conexão que não abre: os papéis se invertem e a tela de A chega a B',
    (tester) async {
      wa.dropFirstAttemptIce = true;
      wb.dropFirstAttemptIce = true;
      final dir = Directory.systemTemp.createTempSync('enxame-roles');
      a.rolesFile = File('${dir.path}/a.json');
      b.rolesFile = File('${dir.path}/b.json');
      await a.call('B', 'Pessoa B', CallKind.screen);
      await until(() => b.phase == CallPhase.incoming);
      await b.accept();
      await until(
        () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
        seconds: 60,
        why:
            'conectou com os papéis invertidos: A=${a.phase} (${a.endedReason}) '
            'B=${b.phase} (${b.endedReason}) '
            'A enviou ${wa.sent.map((m) => m['t']).toList()} '
            'B enviou ${wb.sent.map((m) => m['t']).toList()}',
      );
      expect(wa.sent.map((m) => m['t']), contains('swap'));
      expect(wb.sent.map((m) => m['t']), contains('offer'));
      num frames = 0;
      for (var i = 0; i < 40 && frames < 5; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 500));
        for (final r in await b.stats()) {
          if (r.type == 'inbound-rtp' && r.values['kind'] == 'video') {
            frames = (r.values['framesDecoded'] as num?) ?? 0;
          }
        }
      }
      expect(
        frames,
        greaterThanOrEqualTo(5),
        reason: 'B decodificou a tela de A',
      );
    },
  );

  testWidgets(
    'transmissão de tela: A chama, B atende, B decodifica a tela de A',
    (tester) async {
      await a.call('B', 'Pessoa B', CallKind.screen);
      expect(a.phase, CallPhase.outgoing);
      await until(
        () => b.phase == CallPhase.incoming,
        why: 'B recebeu o convite',
      );
      expect(b.kind, CallKind.screen);
      expect(b.peerName, 'Pessoa A');
      // Nada foi capturado antes de aceitar: nenhuma oferta enviada ainda.
      expect(wa.sent.map((m) => m['t']), ['invite']);

      await b.accept();
      await until(
        () => a.phase == CallPhase.connected && b.phase == CallPhase.connected,
        why:
            'as duas pontas conectaram: A=${a.phase} (${a.endedReason}) '
            'B=${b.phase} (${b.endedReason}) '
            'A enviou ${wa.sent.map((m) => m['t']).toList()} '
            'B enviou ${wb.sent.map((m) => m['t']).toList()}',
      );
      expect(a.sharing, isTrue);

      // O receptor decodifica quadros da tela do chamador.
      num frames = 0;
      for (var i = 0; i < 40 && frames < 5; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 500));
        for (final r in await b.stats()) {
          if (r.type == 'inbound-rtp' && r.values['kind'] == 'video') {
            frames = (r.values['framesDecoded'] as num?) ?? 0;
          }
        }
      }
      expect(
        frames,
        greaterThanOrEqualTo(5),
        reason: 'B decodificou a tela de A',
      );
      expect(b.remoteHasVideo, isTrue);

      // Nitidez e fluidez do envio: resolução de leitura e perto de 30 quadros por segundo.
      num sentWidth = 0;
      num bestFps = 0;
      for (var i = 0; i < 30 && (sentWidth < 1280 || bestFps < 25); i++) {
        await Future<void>.delayed(const Duration(milliseconds: 500));
        for (final r in await a.stats()) {
          if (r.type == 'outbound-rtp' && r.values['kind'] == 'video') {
            sentWidth = (r.values['frameWidth'] as num?) ?? sentWidth;
            final f = (r.values['framesPerSecond'] as num?) ?? 0;
            if (f > bestFps) bestFps = f;
          }
        }
      }
      // ignore: avoid_print
      print('[tela] largura enviada: $sentWidth, fps enviados: $bestFps');
      expect(
        sentWidth,
        greaterThanOrEqualTo(1280),
        reason: 'resolução de leitura',
      );
      expect(
        bestFps,
        greaterThanOrEqualTo(25),
        reason: 'alvo de 30 fps na tela',
      );

      // Parar e desligar: as duas voltam a ociosas e a captura para.
      await a.stopShare();
      expect(a.sharing, isFalse);
      await a.hangup();
      await until(
        () => a.phase == CallPhase.idle && b.phase == CallPhase.idle,
        why: 'encerradas',
      );
      expect(b.endedReason, contains('outro lado'));
    },
    timeout: const Timeout(Duration(minutes: 3)),
  );

  testWidgets('recusar, ocupado e cancelar não deixam a chamada pendurada', (
    tester,
  ) async {
    // Recusada.
    await a.call('B', 'Pessoa B', CallKind.audio);
    await until(() => b.phase == CallPhase.incoming);
    await b.reject();
    await until(() => a.phase == CallPhase.idle);
    expect(a.endedReason, contains('recusada'));

    // Ocupado: B já está em chamada (tocando) quando uma segunda chega.
    await a.call('B', 'Pessoa B', CallKind.audio);
    await until(() => b.phase == CallPhase.incoming);
    final c = _Wire('C')..other = wb;
    await c.send('B', {'t': 'invite', 'id': 'outra', 'kind': 'audio'});
    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(b.peer, 'A', reason: 'a chamada em curso não foi trocada');

    // Cancelar enquanto chama.
    await a.hangup();
    await until(() => b.phase == CallPhase.idle, why: 'B vê o cancelamento');

    // Sinal de outra chamada (id errado) não derruba nada.
    await a.call('B', 'Pessoa B', CallKind.audio);
    await until(() => b.phase == CallPhase.incoming);
    await wa.send('B', {'t': 'hangup', 'id': 'id-de-outra-chamada'});
    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(b.phase, CallPhase.incoming);
    await b.reject();
    await until(() => a.phase == CallPhase.idle);
  }, timeout: const Timeout(Duration(minutes: 2)));

  for (final (label, servers) in [
    (
      'só um STUN por IPv4 literal',
      <Map<String, dynamic>>[
        {'urls': 'stun:74.125.250.129:19302'},
      ],
    ),
    (
      'STUN por nome (como no app)',
      <Map<String, dynamic>>[
        {'urls': 'stun:stun.l.google.com:19302'},
      ],
    ),
  ]) {
    testWidgets('tempo para conectar: $label', (tester) async {
      // Fios próprios: os controladores do setUp também escutam wa/wb e atrapalhariam a medição.
      final w1 = _Wire('A');
      final w2 = _Wire('B');
      w1.other = w2;
      w2.other = w1;
      final a2 = CallController(
        transport: w1,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: servers,
      );
      final b2 = CallController(
        transport: w2,
        media: DeviceMedia(pickScreen: firstScreen),
        iceServers: servers,
      );
      final sw = Stopwatch()..start();
      await a2.call('B', 'Pessoa B', CallKind.screen);
      await until(() => b2.phase == CallPhase.incoming);
      await b2.accept();
      var ok = false;
      for (var i = 0; i < 400 && !ok; i++) {
        ok = a2.phase == CallPhase.connected && b2.phase == CallPhase.connected;
        if (!ok) await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      // ignore: avoid_print
      print(
        '[tempo] $label: ${ok ? '${sw.elapsedMilliseconds} ms' : 'NÃO conectou em 40 s'}',
      );
      await a2.hangup();
      await Future<void>.delayed(const Duration(milliseconds: 300));
      a2.dispose();
      b2.dispose();
    }, timeout: const Timeout(Duration(minutes: 3)));
  }

  testWidgets('teto de largura da tela é respeitado no envio', (tester) async {
    final w1 = _Wire('A');
    final w2 = _Wire('B');
    w1.other = w2;
    w2.other = w1;
    final a2 = CallController(
      transport: w1,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
      screenMaxWidth: 960,
    );
    final b2 = CallController(
      transport: w2,
      media: DeviceMedia(pickScreen: firstScreen),
      iceServers: const [],
    );
    await a2.call('B', 'Pessoa B', CallKind.screen);
    await until(() => b2.phase == CallPhase.incoming);
    await b2.accept();
    await until(
      () => a2.phase == CallPhase.connected && b2.phase == CallPhase.connected,
    );
    num width = 0;
    for (var i = 0; i < 20 && (width == 0 || width > 960); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 500));
      for (final r in await a2.stats()) {
        if (r.type == 'outbound-rtp' && r.values['kind'] == 'video') {
          width = (r.values['frameWidth'] as num?) ?? width;
        }
      }
    }
    // ignore: avoid_print
    print('[teto] largura enviada com teto 960: $width');
    expect(
      width,
      inInclusiveRange(320, 960),
      reason: 'a redução pedida no transceptor vale',
    );
    await a2.hangup();
    await Future<void>.delayed(const Duration(milliseconds: 300));
    a2.dispose();
    b2.dispose();
  }, timeout: const Timeout(Duration(minutes: 3)));
}
