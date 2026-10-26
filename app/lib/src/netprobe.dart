import 'dart:async';
import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

/// O que se sabe da rede deste lado antes de ligar (Fase 7.0). `cgnat` é uma estimativa: serve só
/// para escolher quem oferece primeiro, e a inversão de papéis segue de rede de segurança.
class NetHint {
  const NetHint({this.cgnat = false, this.v6 = false, this.why = ''});

  /// Provavelmente atrás de NAT de operadora (CGNAT, rede móvel ou hotspot).
  final bool cgnat;

  /// Tem endereço IPv6 global (candidato a caminho direto sem NAT).
  final bool v6;

  /// Motivo da estimativa, para o log.
  final String why;

  @override
  String toString() => 'cgnat=$cgnat v6=$v6 ($why)';
}

const _stunServers = [
  ('stun.l.google.com', 19302),
  ('stun.cloudflare.com', 3478),
];

const _stunCookie = 0x2112A442;

/// 100.64.0.0/10 (RFC 6598): faixa reservada ao CGNAT.
bool isCgnatRange(String ip) {
  final p = ip.split('.').map(int.tryParse).toList();
  return p.length == 4 && p[0] == 100 && p[1] != null && (p[1]! & 0xC0) == 64;
}

/// Sub-redes padrão do compartilhamento de internet do celular (iOS e Android antigo): o
/// computador está atrás do CGNAT da operadora.
bool isHotspotRange(String ip) =>
    ip.startsWith('172.20.10.') || ip.startsWith('192.168.43.');

/// IPv6 global (2000::/3): ignora link-local, ULA (Tailscale) e loopback.
bool isGlobalIpv6(InternetAddress a) =>
    a.type == InternetAddressType.IPv6 &&
    !a.isLinkLocal &&
    !a.isLoopback &&
    (a.rawAddress[0] & 0xE0) == 0x20;

Uint8List stunBindingRequest(Uint8List txid) {
  final b = ByteData(20)
    ..setUint16(0, 0x0001)
    ..setUint16(2, 0)
    ..setUint32(4, _stunCookie);
  final out = b.buffer.asUint8List();
  out.setRange(8, 20, txid);
  return out;
}

/// Endereço IPv4 e porta que o servidor viu (`XOR-MAPPED-ADDRESS`, ou `MAPPED-ADDRESS`).
/// `null` se a resposta não é a deste pedido.
({String ip, int port})? parseStunMapped(Uint8List data, Uint8List txid) {
  if (data.length < 20) return null;
  final b = ByteData.sublistView(data);
  if (b.getUint16(0) != 0x0101 || b.getUint32(4) != _stunCookie) return null;
  for (var i = 0; i < 12; i++) {
    if (data[8 + i] != txid[i]) return null;
  }
  final end = min(data.length, 20 + b.getUint16(2));
  var off = 20;
  ({String ip, int port})? plain;
  while (off + 4 <= end) {
    final type = b.getUint16(off);
    final len = b.getUint16(off + 2);
    final v = off + 4;
    if (v + len > end) break;
    if ((type == 0x0020 || type == 0x0001) && len >= 8 && data[v + 1] == 1) {
      final xor = type == 0x0020;
      final port = b.getUint16(v + 2) ^ (xor ? _stunCookie >> 16 : 0);
      final raw = b.getUint32(v + 4) ^ (xor ? _stunCookie : 0);
      final ip =
          '${raw >> 24 & 255}.${raw >> 16 & 255}.${raw >> 8 & 255}.${raw & 255}';
      if (xor) return (ip: ip, port: port);
      plain = (ip: ip, port: port);
    }
    off = v + ((len + 3) & ~3);
  }
  return plain;
}

/// Decide se a rede parece de operadora, a partir do que os servidores STUN viram.
/// `mapped`: um par (ip, porta) por servidor, da mesma porta local; `localIps`: IPv4 das
/// interfaces. Os IPs locais não entram na faixa 100.64/10 de propósito: o Tailscale usa essa
/// faixa e não diz nada sobre o NAT.
NetHint classifyNat({
  required List<({String ip, int port})> mapped,
  required Set<String> localIps,
  required bool v6,
}) {
  final hotspot = localIps.any(isHotspotRange);
  if (mapped.isEmpty) {
    return NetHint(
      cgnat: hotspot,
      v6: v6,
      why: hotspot ? 'hotspot, STUN sem resposta' : 'STUN sem resposta',
    );
  }
  final ip = mapped.first.ip;
  if (isCgnatRange(ip)) {
    return NetHint(cgnat: true, v6: v6, why: 'IP público $ip em 100.64/10');
  }
  if (mapped.map((m) => m.port).toSet().length > 1) {
    return NetHint(
      cgnat: true,
      v6: v6,
      why: 'NAT simétrico (a porta muda por destino)',
    );
  }
  if (localIps.contains(ip)) {
    return NetHint(v6: v6, why: 'IP público direto na interface');
  }
  if (hotspot) {
    return NetHint(cgnat: true, v6: v6, why: 'compartilhamento do celular');
  }
  return NetHint(v6: v6, why: 'NAT comum (roteador)');
}

/// Sonda de rede: dois pedidos STUN pelo mesmo socket UDP (sem libwebrtc) e as interfaces locais.
class NetProbe {
  NetProbe({this.timeout = const Duration(milliseconds: 1500)});

  final Duration timeout;
  NetHint? _cached;
  DateTime _at = DateTime.fromMillisecondsSinceEpoch(0);
  String _key = '';

  /// Resultado recente (mesmas interfaces, até 10 min) ou uma nova sonda. Nunca lança.
  Future<NetHint> probe() async {
    try {
      final ifaces = await NetworkInterface.list();
      final addrs = [for (final i in ifaces) ...i.addresses];
      final key = addrs.map((a) => a.address).toList().join(',');
      final cached = _cached;
      if (cached != null &&
          key == _key &&
          DateTime.now().difference(_at) < const Duration(minutes: 10)) {
        return cached;
      }
      final local = {
        for (final a in addrs)
          if (a.type == InternetAddressType.IPv4 && !a.isLoopback) a.address,
      };
      final mapped = await _stun();
      final hint = classifyNat(
        mapped: mapped,
        localIps: local,
        v6: addrs.any(isGlobalIpv6),
      );
      _cached = hint;
      _key = key;
      _at = DateTime.now();
      return hint;
    } catch (e) {
      return NetHint(why: 'sonda falhou: $e');
    }
  }

  Future<List<({String ip, int port})>> _stun() async {
    final socket = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    final rnd = Random.secure();
    final pending = <String, Uint8List>{}; // "ip:porta" do servidor -> txid
    final order = <String>[];
    final found = <String, ({String ip, int port})>{};
    final done = Completer<void>();
    final sub = socket.listen((e) {
      if (e != RawSocketEvent.read) return;
      final d = socket.receive();
      if (d == null) return;
      final key = '${d.address.address}:${d.port}';
      final txid = pending[key];
      final m = txid == null ? null : parseStunMapped(d.data, txid);
      if (m != null) found[key] = m;
      if (found.length == pending.length && !done.isCompleted) done.complete();
    });
    try {
      for (final (host, port) in _stunServers) {
        try {
          final addrs = await InternetAddress.lookup(
            host,
            type: InternetAddressType.IPv4,
          ).timeout(timeout);
          if (addrs.isEmpty) continue;
          final txid = Uint8List.fromList(
            List.generate(12, (_) => rnd.nextInt(256)),
          );
          final key = '${addrs.first.address}:$port';
          pending[key] = txid;
          order.add(key);
          socket.send(stunBindingRequest(txid), addrs.first, port);
        } catch (_) {}
      }
      if (pending.isEmpty) return const [];
      await done.future.timeout(timeout, onTimeout: () {});
    } finally {
      await sub.cancel();
      socket.close();
    }
    return [
      for (final k in order)
        if (found[k] != null) found[k]!,
    ];
  }
}
