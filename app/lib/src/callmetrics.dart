import 'dart:convert';
import 'dart:io';

/// Medição de conexões de chamada (Fase 7.1): um registro local por tentativa de conexão, sem IPs,
/// sem fingerprints e sem texto livre. Fica em `<dados>/call_metrics.jsonl`; sai do computador só
/// quando o usuário exporta (`CallMetricsLog.exportAnonymized`). Esquema em `docs/metricas-chamadas.md`.

/// Degrau da escada de conexão (roadmap da Fase 7, D4) que de fato carregou a mídia.
enum Ladder { lan, ipv6, ipv4Direct, relay, none }

/// Caminho escolhido pelo ICE, sem endereços.
class PathInfo {
  const PathInfo({
    required this.localType,
    required this.remoteType,
    required this.v6,
    required this.lan,
  });

  /// `host`, `srflx`, `prflx` ou `relay`.
  final String localType;
  final String remoteType;
  final bool v6;

  /// Os dois lados em endereço de rede local (mesma LAN).
  final bool lan;

  Ladder get ladder {
    if (localType == 'relay' || remoteType == 'relay') return Ladder.relay;
    if (lan) return Ladder.lan;
    return v6 ? Ladder.ipv6 : Ladder.ipv4Direct;
  }

  /// Identifica o par sem endereço: usado para contar trocas de caminho durante a chamada.
  String get key => '$localType/$remoteType/${v6 ? 6 : 4}/$lan';
}

/// RTT abaixo do qual um par sem endereço remoto conhecido é tratado como LAN.
const _lanRttMs = 30.0;

bool isPrivateAddress(String ip) {
  if (ip.contains(':')) {
    final a = ip.toLowerCase();
    return a.startsWith('fe80') || a.startsWith('fc') || a.startsWith('fd');
  }
  final p = ip.split('.').map(int.tryParse).toList();
  if (p.length != 4 || p.any((e) => e == null)) return false;
  final a = p[0]!, b = p[1]!;
  return a == 10 ||
      (a == 172 && b >= 16 && b <= 31) ||
      (a == 192 && b == 168) ||
      (a == 169 && b == 254);
}

/// Um item das estatísticas do WebRTC (`StatsReport`), sem depender do plugin.
typedef StatItem = ({String id, String type, Map<dynamic, dynamic> values});

/// O par em uso: bem-sucedido e nomeado/selecionado; entre vários, o que mais enviou.
({PathInfo path, double? rttMs})? selectedPath(List<StatItem> stats) {
  final byId = {for (final r in stats) r.id: r};
  StatItem? best;
  num bestBytes = -1;
  for (final r in stats.where((r) => r.type == 'candidate-pair')) {
    if (r.values['state'] != 'succeeded') continue;
    final used = r.values['nominated'] == true || r.values['selected'] == true;
    final bytes = (r.values['bytesSent'] as num?) ?? 0;
    if (!used && bytes <= 0) continue;
    if (bytes > bestBytes) {
      best = r;
      bestBytes = bytes;
    }
  }
  if (best == null) return null;
  String addr(Object? id) {
    final c = byId[id];
    return '${c?.values['address'] ?? c?.values['ip'] ?? ''}';
  }

  String type(Object? id) => '${byId[id]?.values['candidateType'] ?? '?'}';
  final l = addr(best.values['localCandidateId']);
  final r = addr(best.values['remoteCandidateId']);
  final rtt = best.values['currentRoundTripTime'];
  final rttMs = rtt is num ? rtt * 1000.0 : null;
  // O libwebrtc não informa o endereço de um candidato remoto `prflx` (campo vazio). Nesse caso é
  // LAN se este lado é um `host` em rede privada e o RTT é de rede local (campo, 2026-10-26: 5-9 ms).
  final localType = type(best.values['localCandidateId']);
  final remoteUnknown =
      r.isEmpty && type(best.values['remoteCandidateId']) == 'prflx';
  final lan =
      l.isNotEmpty &&
      isPrivateAddress(l) &&
      (r.isNotEmpty
          ? isPrivateAddress(r)
          : remoteUnknown &&
                localType == 'host' &&
                rttMs != null &&
                rttMs < _lanRttMs);
  return (
    path: PathInfo(
      localType: localType,
      remoteType: type(best.values['remoteCandidateId']),
      v6: l.contains(':') || r.contains(':'),
      lan: lan,
    ),
    rttMs: rttMs,
  );
}

/// Classe da falha, sem texto livre (o motivo mostrado ao usuário pode citar o nome do contato).
String failClass(String reason) {
  final r = reason.toLowerCase();
  if (r.contains('caminho direto') || r.contains('não abriu')) return 'timeout';
  if (r.contains('falhou')) return 'ice_failed';
  if (r.contains('pelo outro lado')) return 'remote_hangup';
  if (r.contains('negocia') || r.contains('mídia')) return 'negotiation';
  if (r.contains('chamar')) return 'signaling';
  return 'other';
}

/// Acumula amostras de uma conexão (RTT, perda, trocas de caminho).
class PathSampler {
  PathInfo? first;
  PathInfo? last;
  int changes = 0;
  final List<double> rtts = [];
  num _lost = 0, _received = 0;
  double? _jitterMs;

  void sample(List<StatItem> stats) {
    final sel = selectedPath(stats);
    if (sel != null) {
      first ??= sel.path;
      if (last != null && last!.key != sel.path.key) changes++;
      last = sel.path;
      if (sel.rttMs != null) rtts.add(sel.rttMs!);
    }
    for (final r in stats) {
      if (r.type == 'inbound-rtp' &&
          (r.values['kind'] ?? r.values['mediaType']) == 'audio') {
        _lost = (r.values['packetsLost'] as num?) ?? _lost;
        _received = (r.values['packetsReceived'] as num?) ?? _received;
        final j = r.values['jitter'];
        if (j is num) _jitterMs = j * 1000.0;
      }
    }
  }

  double? get rttAvg =>
      rtts.isEmpty ? null : rtts.reduce((a, b) => a + b) / rtts.length;
  double? get rttMax =>
      rtts.isEmpty ? null : rtts.reduce((a, b) => a > b ? a : b);
  double? get lossPct {
    final total = _lost + _received;
    return total <= 0 ? null : 100.0 * _lost / total;
  }

  double? get jitterMs => _jitterMs;
}

double? _r1(double? v) => v == null ? null : (v * 10).roundToDouble() / 10;

/// Um registro (uma conexão). `toJson` não leva IP, fingerprint nem texto livre.
class CallRecord {
  CallRecord({
    required this.hour,
    required this.os,
    required this.kind,
    required this.caller,
    required this.outcome,
    required this.attempts,
    required this.swapped,
    required this.msToConnect,
    required this.seconds,
    this.failClass,
    this.sampler,
    this.net = const {},
    this.peer = const {},
  });

  /// Hora cheia (`2026-10-26T15`): resolução suficiente para a análise e menos rastreável.
  final String hour;
  final String os;
  final String kind;
  final bool caller;

  /// `connected` ou `failed`.
  final String outcome;
  final int attempts;
  final bool swapped;
  final int? msToConnect;
  final int seconds;
  final String? failClass;
  final PathSampler? sampler;
  final Map<String, Object?> net;
  final Map<String, Object?> peer;

  Map<String, Object?> toJson() {
    final s = sampler;
    final path = s?.last;
    return {
      'v': 1,
      'hour': hour,
      'os': os,
      'kind': kind,
      'role': caller ? 'caller' : 'callee',
      'outcome': outcome,
      'attempts': attempts,
      'swapped': swapped,
      'ms_to_connect': msToConnect,
      'seconds': seconds,
      if (failClass != null) 'fail': failClass,
      'ladder': (path?.ladder ?? Ladder.none).name,
      if (path != null) ...{
        'local_type': path.localType,
        'remote_type': path.remoteType,
        'family': path.v6 ? 'v6' : 'v4',
      },
      'path_changes': s?.changes ?? 0,
      'rtt_ms_avg': _r1(s?.rttAvg),
      'rtt_ms_max': _r1(s?.rttMax),
      'loss_pct': _r1(s?.lossPct),
      'jitter_ms': _r1(s?.jitterMs),
      'net': net,
      'peer': peer,
    };
  }
}

/// Arquivo de registros (uma linha JSON por conexão).
class CallMetricsLog {
  CallMetricsLog(this.file);
  final File file;

  void append(CallRecord r) {
    try {
      file.writeAsStringSync(
        '${jsonEncode(r.toJson())}\n',
        mode: FileMode.append,
      );
    } catch (_) {}
  }

  List<Map<String, dynamic>> read() {
    try {
      return [
        for (final line in file.readAsLinesSync())
          if (line.trim().isNotEmpty)
            if (jsonDecode(line) case final Map<String, dynamic> m) m,
      ];
    } catch (_) {
      return const [];
    }
  }

  /// Escreve o arquivo para enviar a quem coordena a medição. Os registros já são anônimos; aqui
  /// só se confirma que nenhum campo fora do esquema vai junto. Devolve quantos registros foram.
  int exportAnonymized(File out) {
    const allowed = {
      'v', 'hour', 'os', 'kind', 'role', 'outcome', 'attempts', 'swapped',
      'ms_to_connect', 'seconds', 'fail', 'ladder', 'local_type', 'remote_type',
      'family', 'path_changes', 'rtt_ms_avg', 'rtt_ms_max', 'loss_pct',
      'jitter_ms', 'net', 'peer', //
    };
    final rows = [
      for (final m in read())
        {
          for (final e in m.entries)
            if (allowed.contains(e.key)) e.key: e.value,
        },
    ];
    out.writeAsStringSync(
      const JsonEncoder.withIndent('  ').convert({'v': 1, 'calls': rows}),
    );
    return rows.length;
  }
}
