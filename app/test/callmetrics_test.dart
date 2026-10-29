import 'dart:convert';
import 'dart:io';

import 'package:enxame_app/src/callmetrics.dart';
import 'package:enxame_app/src/calls.dart' show BridgeStatus;
import 'package:flutter_test/flutter_test.dart';

StatItem cand(String id, String type, String addr) => (
  id: id,
  type: 'local-candidate',
  values: {'candidateType': type, 'address': addr},
);

StatItem pair(
  String l,
  String r, {
  num bytes = 100,
  num? rtt,
  String state = 'succeeded',
}) => (
  id: 'p-$l-$r',
  type: 'candidate-pair',
  values: {
    'state': state,
    'nominated': true,
    'bytesSent': bytes,
    'localCandidateId': l,
    'remoteCandidateId': r,
    'currentRoundTripTime': ?rtt,
  },
);

void main() {
  test('endereços privados', () {
    expect(isPrivateAddress('192.168.0.5'), isTrue);
    expect(isPrivateAddress('10.61.2.3'), isTrue);
    expect(isPrivateAddress('172.20.1.1'), isTrue);
    expect(isPrivateAddress('172.32.1.1'), isFalse);
    expect(isPrivateAddress('189.1.2.3'), isFalse);
    expect(isPrivateAddress('fe80::1'), isTrue);
    expect(isPrivateAddress('2804:14c::1'), isFalse);
  });

  test('host/host na LAN é o degrau lan', () {
    final sel = selectedPath([
      cand('l', 'host', '192.168.2.2'),
      cand('r', 'host', '192.168.2.87'),
      pair('l', 'r', rtt: 0.004),
    ])!;
    expect(sel.path.ladder, Ladder.lan);
    expect(sel.rttMs, closeTo(4, 1e-9));
  });

  test('prflx remoto sem endereço: LAN só com host privado e RTT baixo', () {
    StatItem remote(String type) =>
        (id: 'r', type: 'remote-candidate', values: {'candidateType': type});
    Ladder of(String la, double rtt, {String lt = 'host'}) => selectedPath([
      cand('l', lt, la),
      remote('prflx'),
      pair('l', 'r', rtt: rtt),
    ])!.path.ladder;
    expect(of('192.168.2.87', 0.006), Ladder.lan);
    expect(of('192.168.2.87', 0.070), Ladder.ipv4Direct); // RTT de internet
    expect(of('179.1.2.3', 0.006), Ladder.ipv4Direct); // este lado é público
    expect(of('192.168.2.87', 0.006, lt: 'srflx'), Ladder.ipv4Direct);
  });

  test('srflx/srflx IPv4 é direto; IPv6 tem degrau próprio; relay vence', () {
    Ladder of(String lt, String la, String rt, String ra) =>
        selectedPath([cand('l', lt, la), cand('r', rt, ra), pair('l', 'r')])!
            .path
            .ladder;
    expect(of('srflx', '179.1.2.3', 'srflx', '177.4.5.6'), Ladder.ipv4Direct);
    expect(of('host', '2804:14c::5', 'host', '2804:14c::9'), Ladder.ipv6);
    expect(of('relay', '1.2.3.4', 'srflx', '5.6.7.8'), Ladder.relay);
  });

  test(
    'sem par bem-sucedido não há caminho; escolhe o par que mais enviou',
    () {
      expect(
        selectedPath([
          cand('l', 'host', '1.1.1.1'),
          pair('l', 'l', state: 'failed'),
        ]),
        isNull,
      );
      final sel = selectedPath([
        cand('a', 'host', '192.168.0.2'),
        cand('b', 'srflx', '177.1.1.1'),
        cand('c', 'srflx', '189.2.2.2'),
        pair('a', 'a', bytes: 10),
        pair('b', 'c', bytes: 5000),
      ])!;
      expect(sel.path.ladder, Ladder.ipv4Direct);
    },
  );

  test('o amostrador conta trocas de caminho, RTT e perda', () {
    final s = PathSampler();
    s.sample([
      cand('l', 'srflx', '179.1.1.1'),
      cand('r', 'srflx', '177.2.2.2'),
      pair('l', 'r', rtt: 0.100),
      (
        id: 'in',
        type: 'inbound-rtp',
        values: {
          'kind': 'audio',
          'packetsLost': 2,
          'packetsReceived': 98,
          'jitter': 0.004,
        },
      ),
    ]);
    s.sample([
      cand('l', 'relay', '1.1.1.1'),
      cand('r', 'srflx', '177.2.2.2'),
      pair('l', 'r', rtt: 0.300),
    ]);
    expect(s.changes, 1);
    expect(s.rttAvg, closeTo(200, 1e-9));
    expect(s.rttMax, closeTo(300, 1e-9));
    expect(s.lossPct, closeTo(2, 1e-9));
    expect(s.jitterMs, closeTo(4, 1e-9));
  });

  test('classes de falha', () {
    expect(
      failClass('não foi possível abrir caminho direto com Fulano'),
      'timeout',
    );
    expect(failClass('a conexão falhou'), 'ice_failed');
    expect(failClass('encerrada pelo outro lado'), 'remote_hangup');
    expect(failClass('falha na negociação: x'), 'negotiation');
    expect(failClass('qualquer coisa'), 'other');
  });

  test('o registro não carrega IP, fingerprint nem texto livre; exporta só o esquema', () {
    final dir = Directory.systemTemp.createTempSync('enxame-metrics');
    final log = CallMetricsLog(File('${dir.path}/m.jsonl'));
    final s = PathSampler()
      ..sample([
        cand('l', 'srflx', '179.159.253.16'),
        cand('r', 'srflx', '177.79.108.65'),
        pair('l', 'r', rtt: 0.28),
      ]);
    log.append(
      CallRecord(
        hour: '2026-10-26T15',
        os: 'linux',
        kind: 'audio',
        caller: true,
        outcome: 'connected',
        attempts: 1,
        swapped: false,
        msToConnect: 900,
        seconds: 30,
        sampler: s,
        net: const {'nat': 'cone', 'cgnat': false, 'v6': false, 'igd': true},
        peer: const {'cgnat': true, 'v6': false},
      ),
    );
    final raw = File('${dir.path}/m.jsonl').readAsStringSync();
    expect(raw, isNot(contains('179.159')));
    expect(raw, isNot(contains('177.79')));
    final row = log.read().single;
    expect(row['ladder'], 'ipv4Direct');
    expect(row['ms_to_connect'], 900);
    // Uma linha com campo estranho (de uma versão futura ou adulterada) não vai na exportação.
    File('${dir.path}/m.jsonl').writeAsStringSync(
      '${jsonEncode({...row, 'ip': '1.2.3.4', 'fingerprint': 'abc'})}\n',
      mode: FileMode.append,
    );
    final out = File('${dir.path}/export.json');
    expect(log.exportAnonymized(out), 2);
    final exported = out.readAsStringSync();
    expect(exported, isNot(contains('1.2.3.4')));
    expect(exported, isNot(contains('fingerprint')));
  });

  group('ponte de mídia (Fase 7.2)', () {
    PathSampler relaySampler() => PathSampler()
      ..sample([
        cand('l', 'relay', '198.18.0.1'),
        cand('r', 'relay', '198.18.0.2'),
        pair('l', 'r', rtt: 0.05),
      ]);

    Map<String, Object?> row(bool? direct) => CallRecord(
      hour: '2026-10-29T14',
      os: 'linux',
      kind: 'audio',
      caller: true,
      outcome: 'connected',
      attempts: 1,
      swapped: false,
      msToConnect: 900,
      seconds: 30,
      sampler: relaySampler(),
      bridge: BridgeStatus(
        linked: true,
        direct: direct,
        maxDatagram: 1414,
        sent: 10,
        received: 9,
        droppedTooLarge: 0,
        droppedOther: 0,
      ),
    ).toJson();

    test('relay do ICE com iroh direto vira irohDirect; com relay do iroh, irohRelay', () {
      expect(row(true)['ladder'], 'irohDirect');
      expect(row(false)['ladder'], 'irohRelay');
      expect(row(true)['bridge'], containsPair('max_datagram', 1414));
    });
  });
}
