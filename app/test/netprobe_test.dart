import 'dart:io';
import 'dart:typed_data';

import 'package:enxame_app/src/netprobe.dart';
import 'package:flutter_test/flutter_test.dart';

Uint8List _response(Uint8List txid, {required String ip, required int port}) {
  const cookie = 0x2112A442;
  final p = ip.split('.').map(int.parse).toList();
  final addr = (p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]) ^ cookie;
  final b = ByteData(32)
    ..setUint16(0, 0x0101)
    ..setUint16(2, 12)
    ..setUint32(4, cookie)
    ..setUint16(20, 0x0020)
    ..setUint16(22, 8)
    ..setUint8(25, 1)
    ..setUint16(26, port ^ (cookie >> 16))
    ..setUint32(28, addr);
  final out = b.buffer.asUint8List();
  out.setRange(8, 20, txid);
  return out;
}

void main() {
  final txid = Uint8List.fromList(List.generate(12, (i) => i + 1));

  test('o pedido STUN tem o cabeçalho certo', () {
    final r = stunBindingRequest(txid);
    expect(r.length, 20);
    expect(r.sublist(0, 8), [0, 1, 0, 0, 0x21, 0x12, 0xA4, 0x42]);
    expect(r.sublist(8), txid);
  });

  test('lê o XOR-MAPPED-ADDRESS e ignora resposta de outro pedido', () {
    final m = parseStunMapped(
      _response(txid, ip: '203.0.113.9', port: 40001),
      txid,
    );
    expect(m, (ip: '203.0.113.9', port: 40001));
    final other = Uint8List.fromList(List.filled(12, 9));
    expect(
      parseStunMapped(_response(txid, ip: '203.0.113.9', port: 1), other),
      isNull,
    );
    expect(parseStunMapped(Uint8List(5), txid), isNull);
  });

  test('faixa 100.64.0.0/10', () {
    expect(isCgnatRange('100.64.0.1'), isTrue);
    expect(isCgnatRange('100.127.227.46'), isTrue);
    expect(isCgnatRange('100.128.0.1'), isFalse);
    expect(isCgnatRange('100.63.255.255'), isFalse);
    expect(isCgnatRange('192.168.0.1'), isFalse);
  });

  test('IPv6 global: ignora link-local, ULA e loopback', () {
    bool g(String a) => isGlobalIpv6(InternetAddress(a));
    expect(g('2804:14c:1::5'), isTrue);
    expect(g('fe80::1'), isFalse);
    expect(g('fd7a:115c:a1e0::1'), isFalse);
    expect(g('::1'), isFalse);
  });

  group('classifyNat', () {
    const home = {'192.168.0.31', '100.127.227.46'}; // inclui o IP do Tailscale
    test('IP público em 100.64/10 é CGNAT', () {
      final h = classifyNat(
        mapped: [(ip: '100.70.1.2', port: 5000)],
        localIps: home,
        v6: false,
      );
      expect(h.cgnat, isTrue);
    });
    test('roteador comum não é CGNAT, mesmo com Tailscale na máquina', () {
      final h = classifyNat(
        mapped: [(ip: '189.1.2.3', port: 5000), (ip: '189.1.2.3', port: 5000)],
        localIps: home,
        v6: false,
      );
      expect(h.cgnat, isFalse);
    });
    test('porta que muda por destino é NAT simétrico (CGNAT)', () {
      final h = classifyNat(
        mapped: [(ip: '189.1.2.3', port: 5000), (ip: '189.1.2.3', port: 5001)],
        localIps: home,
        v6: false,
      );
      expect(h.cgnat, isTrue);
    });
    test('IP público direto na interface', () {
      final h = classifyNat(
        mapped: [(ip: '189.1.2.3', port: 5000)],
        localIps: {'189.1.2.3'},
        v6: true,
      );
      expect(h.cgnat, isFalse);
      expect(h.v6, isTrue);
    });
    test('compartilhamento do celular conta como CGNAT', () {
      final h = classifyNat(
        mapped: [(ip: '189.1.2.3', port: 5000)],
        localIps: {'172.20.10.2'},
        v6: false,
      );
      expect(h.cgnat, isTrue);
    });
    test('sem resposta do STUN: não afirma nada', () {
      final h = classifyNat(mapped: [], localIps: home, v6: false);
      expect(h.cgnat, isFalse);
    });
  });
}
