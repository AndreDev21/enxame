// Chamada 1:1 de ponta a ponta no mesmo processo: dois CallController trocam os sinais (convite,
// aceite, oferta, resposta, ICE) por um transporte em memória e a mídia vai por WebRTC de verdade.
// Cobre a transmissão de tela: a tela real é capturada pelo chamador e decodificada pelo outro lado.
// Uso: flutter test integration_test/call_test.dart -d linux
import 'dart:async';

import 'package:enxame_app/src/calls.dart';
import 'package:enxame_app/src/device_media.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:integration_test/integration_test.dart';

class _Wire implements SignalTransport {
  _Wire(this.self);
  final String self;
  late _Wire other;
  final _in = StreamController<IncomingSignal>.broadcast();
  final sent = <Map<String, dynamic>>[];

  @override
  Stream<IncomingSignal> get incoming => _in.stream;

  @override
  Future<void> send(String peer, Map<String, dynamic> msg) async {
    sent.add(msg);
    // Passa por JSON como no app e entrega depois de um ciclo (rede de verdade é assíncrona).
    final copy = decodeSignal(encodeSignal(msg))!;
    Future<void>.delayed(const Duration(milliseconds: 5), () {
      other._in.add(IncomingSignal(self, 'Pessoa $self', copy));
    });
  }
}

Future<void> until(bool Function() ok, {int seconds = 30, String? why}) async {
  for (var i = 0; i < seconds * 10 && !ok(); i++) {
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  expect(ok(), isTrue, reason: why);
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
}
