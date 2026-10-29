import 'dart:async';
import 'dart:convert';

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

import 'callmetrics.dart';
import 'netprobe.dart';

/// Tipo da chamada: o que o **chamador** envia. Em `screen` só o chamador transmite a tela.
enum CallKind { audio, video, screen }

enum CallPhase { idle, outgoing, incoming, connecting, connected }

/// Um sinal recebido de um contato.
class IncomingSignal {
  const IncomingSignal(this.peer, this.name, this.msg);
  final String peer; // fingerprint
  final String name;
  final Map<String, dynamic> msg;
}

/// Como os sinais viajam (no app, pelo núcleo; nos testes, em memória).
abstract class SignalTransport {
  Future<void> send(String peer, Map<String, dynamic> msg);
  Stream<IncomingSignal> get incoming;
}

/// De onde vêm microfone, câmera e tela. Só é chamado depois de a chamada ser aceita e de uma
/// ação explícita do usuário (privacidade, spec Fase 6).
abstract class MediaProvider {
  Future<MediaStream?> microphone();
  Future<MediaStream?> camera();

  /// O usuário escolhe a tela/janela; `null` = cancelou.
  Future<MediaStream?> screen();
}

const _stun = [
  {'urls': 'stun:stun.l.google.com:19302'},
  {'urls': 'stun:stun.cloudflare.com:3478'},
];

/// Servidor TURN local da chamada (ponte de mídia sobre o iroh, Fase 7.2).
class BridgeInfo {
  const BridgeInfo(this.url, this.username, this.credential);
  final String url;
  final String username;
  final String credential;
}

/// Medidas da ponte (sem endereços).
class BridgeStatus {
  const BridgeStatus({
    required this.linked,
    required this.direct,
    required this.maxDatagram,
    required this.sent,
    required this.received,
    required this.droppedTooLarge,
    required this.droppedOther,
  });
  final bool linked;

  /// `true` = o iroh usa caminho direto; `false` = relay do iroh; `null` = ainda sem conexão.
  final bool? direct;
  final int? maxDatagram;
  final int sent;
  final int received;
  final int droppedTooLarge;
  final int droppedOther;
}

/// Liga a chamada ao núcleo: sobe e derruba a ponte `enxame/media/1`. `null` no controlador =
/// sem ponte (testes).
abstract class MediaBridgeProvider {
  /// `caller`: quem ligou disca; quem atendeu espera e deve subir antes de responder.
  Future<BridgeInfo> start(String peerFp, {required bool caller});
  Future<void> stop();
  Future<BridgeStatus?> status();
}

const _ringTimeout = Duration(seconds: 45);
const _connectTimeout = Duration(seconds: 25);

/// Prazo da primeira tentativa: uma conexão que abre leva ~4 s; passando disso, inverte os papéis.
const _firstAttemptTimeout = Duration(seconds: 7);

/// Quanto o ICE pode ficar em `Failed` antes de a tentativa ser dada como perdida.
const _failedGrace = Duration(seconds: 3);

/// Uma chamada 1:1 por vez. Sinalização por mensagens JSON (`invite`, `accept`, `reject`, `busy`,
/// `offer`, `answer`, `ice`, `hangup`); mídia por WebRTC (DTLS-SRTP).
class CallController extends ChangeNotifier {
  CallController({
    required this.transport,
    required this.media,
    this.iceServers = _stun,
    this.screenMaxWidth = 1920,
    this.netProbe,
    this.bridge,
  }) {
    _sub = transport.incoming.listen(_onSignal);
  }

  /// Papéis que funcionaram com cada contato (`<dados>/call_roles.json`): se com ele só conecta
  /// quando quem atendeu oferece, as próximas chamadas já começam assim, sem esperar a falha.
  File? rolesFile;
  Map<String, bool>? _roles;

  Map<String, bool> _loadRoles() {
    final cached = _roles;
    if (cached != null) return cached;
    final out = <String, bool>{};
    try {
      final j =
          jsonDecode(rolesFile!.readAsStringSync()) as Map<String, dynamic>;
      for (final e in j.entries) {
        if (e.value is bool) out[e.key] = e.value as bool;
      }
    } catch (_) {}
    return _roles = out;
  }

  /// Com este contato, nesta direção, a chamada só conectou com quem atendeu oferecendo?
  /// `null` = ainda não se sabe (a estimativa de rede decide).
  bool? _rememberedCalleeOffers(String peerFp, {required bool iAmCaller}) =>
      rolesFile == null ? null : _loadRoles()['$peerFp|$iAmCaller'];

  bool _calleeOffersFor(String peerFp, {required bool iAmCaller}) =>
      _rememberedCalleeOffers(peerFp, iAmCaller: iAmCaller) ?? false;

  /// Registro local de cada conexão (Fase 7.1); `null` desliga a medição (testes).
  CallMetricsLog? metrics;
  Stopwatch? _connectWatch; // desde que a chamada foi aceita até conectar
  Stopwatch? _connectedWatch; // desde que conectou
  int? _msToConnect;
  PathSampler _sampler = PathSampler();

  List<StatItem> _items(List<StatsReport> s) => [
    for (final r in s) (id: r.id, type: r.type, values: r.values),
  ];

  /// Fecha a medição desta chamada e a grava. Só conta chamadas que chegaram a tentar conectar.
  Future<void> _recordMetrics(String reason) async {
    final log = metrics;
    final watch = _connectWatch;
    if (log == null || watch == null || kind == null) return;
    final connected = phase == CallPhase.connected;
    try {
      if (_pc != null) {
        _sampler.sample(
          _items(await stats_().timeout(const Duration(seconds: 1))),
        );
      }
    } catch (_) {}
    await _sampleBridge();
    await warmUp(wait: const Duration(milliseconds: 200));
    final outcome = connected
        ? 'connected'
        : (reason == 'encerrada' ? 'cancelled' : 'failed');
    log.append(
      CallRecord(
        hour: DateTime.now().toUtc().toIso8601String().substring(0, 13),
        os: Platform.operatingSystem,
        kind: kind!.name,
        caller: _caller,
        outcome: outcome,
        attempts: _attempt + 1,
        swapped: _attempt > 0,
        msToConnect: _msToConnect,
        seconds: (connected ? _connectedWatch?.elapsed.inSeconds : null) ?? 0,
        failClass: outcome == 'failed' ? failClass(reason) : null,
        sampler: _sampler,
        bridge: _bridgeStatus,
        net: netProbe == null ? const {} : _mine.toJson(),
        peer: netProbe == null
            ? const {}
            : {'cgnat': _peerCgnat, 'v6': _peerV6},
      ),
    );
  }

  /// Mede a rede deste lado (CGNAT, IPv6). `null` desliga a estimativa (testes).
  final Future<NetHint> Function()? netProbe;
  NetHint _mine = const NetHint();
  Future<NetHint>? _probing;

  /// O outro lado parece estar atrás de CGNAT / tem IPv6 global (dito por ele na sinalização).
  bool _peerCgnat = false;
  bool _peerV6 = false;

  /// Mede a rede (resultado em cache por alguns minutos) e espera no máximo `wait`; a medição
  /// continua em segundo plano. Nunca lança.
  Future<void> warmUp({
    Duration wait = const Duration(milliseconds: 700),
  }) async {
    final probe = netProbe;
    if (probe == null) return;
    final f = _probing ??= probe()
        .then((h) {
          _mine = h;
          _log('rede deste lado: $h');
          return h;
        })
        .whenComplete(() => _probing = null);
    try {
      await f.timeout(wait);
    } catch (_) {}
  }

  Map<String, dynamic> get _netFields => netProbe == null
      ? const {}
      : {'cg': _mine.cgnat ? 1 : 0, 'v6': _mine.v6 ? 1 : 0};

  /// Por que não abriu caminho direto, e o que fazer.
  String _noPathMessage() {
    final name = peerName ?? 'o contato';
    if (_mine.cgnat && _peerCgnat) {
      return 'as duas pontas parecem estar em rede móvel (CGNAT), que não abre caminho direto '
          'entre si: conecte um dos dois a um Wi-Fi e tente de novo';
    }
    if (_mine.cgnat) {
      return 'a sua rede parece ser móvel (CGNAT) e não abriu caminho direto com $name: '
          'tente por um Wi-Fi';
    }
    if (_peerCgnat) {
      return 'a rede de $name parece ser móvel (CGNAT) e não abriu caminho direto: '
          'peça para $name usar um Wi-Fi';
    }
    return 'não foi possível abrir caminho direto com $name; alguns roteadores e redes '
        'móveis bloqueiam chamadas: tente por outra rede (Wi-Fi) ou ative o IPv6 no roteador';
  }

  void _rememberRole() {
    final p = peer;
    if (p == null || rolesFile == null) return;
    // Quem ofereceu na tentativa que funcionou: a que começou assim, a menos que tenha trocado.
    final calleeOffers = _calleeOfferedFirst != (_attempt == 1);
    final roles = _loadRoles();
    final key = '$p|$_caller';
    if (roles[key] == calleeOffers) return;
    roles[key] = calleeOffers;
    _log(
      'papéis que funcionam com este contato: quem atende oferece = $calleeOffers',
    );
    try {
      rolesFile!.writeAsStringSync(jsonEncode(roles));
    } catch (_) {}
  }

  /// A primeira tentativa já começou com quem atendeu oferecendo (lembrado ou pedido pelo outro).
  bool _calleeOfferedFirst = false;

  /// Arquivo de diagnóstico (`<dados>/calls.log`): tempos de cada etapa, para achar lentidão.
  File? logFile;
  final Stopwatch _clock = Stopwatch()..start();

  void _log(String msg) {
    final line =
        '[call +${_clock.elapsedMilliseconds}ms ${DateTime.now().toIso8601String()}] $msg';
    debugPrint(line);
    try {
      logFile?.writeAsStringSync('$line\n', mode: FileMode.append);
    } catch (_) {}
  }

  /// Largura máxima da tela enviada: telas maiores são reduzidas (menos pixels = mais quadros na
  /// mesma banda e no mesmo CPU). A 2560x1440 o X11 entregava só ~21 fps; texto segue legível a 1080p.
  final int screenMaxWidth;

  final SignalTransport transport;
  final MediaProvider media;
  final List<Map<String, dynamic>> iceServers;

  /// Ponte de mídia sobre o iroh (TURN local); a cada conexão WebRTC sobe uma nova.
  final MediaBridgeProvider? bridge;
  BridgeStatus? _bridgeStatus;
  bool _bridgeUp = false;

  /// O caminho em uso é o relay público do iroh: só cabe áudio (D4, degrau 6).
  bool audioOnlyPath = false;
  late final StreamSubscription<IncomingSignal> _sub;

  final RTCVideoRenderer remoteRenderer = RTCVideoRenderer();
  final RTCVideoRenderer localRenderer = RTCVideoRenderer();

  /// Prévia da tela que este lado transmite (separada da câmera: trocar o `srcObject` da câmera
  /// não atualizava a imagem no Windows).
  final RTCVideoRenderer screenRenderer = RTCVideoRenderer();
  bool _renderersReady = false;

  CallPhase phase = CallPhase.idle;
  CallKind? kind;
  String? peer;
  String? peerName;
  String? endedReason;
  bool get isCaller => _caller;
  bool micOn = true;
  bool camOn = true;
  bool sharing = false;
  bool remoteHasVideo = false;

  /// O outro lado está escolhendo/abrindo o que vai transmitir (antes da oferta).
  bool peerPreparing = false;

  String? _id;
  bool _caller = false;

  /// Quem manda as ofertas. Começa igual a `_caller`; se a primeira conexão não abre, os papéis
  /// se invertem (ver `_swapRoles`). `_caller` segue sendo quem ligou (na chamada de tela, quem
  /// transmite).
  bool _offerer = false;

  /// 0 = primeira tentativa de conexão; 1 = papéis invertidos. Todo sinal leva este número e os
  /// de outra tentativa são ignorados (candidatos atrasados da conexão anterior).
  int _attempt = 0;

  /// A primeira conexão falhou e a chamada tenta de novo com os papéis invertidos.
  bool retrying = false;
  RTCPeerConnection? _pc;
  MediaStream? _mic; // áudio (e câmera, se houver)
  MediaStream? _screen;
  MediaStreamTrack? _cameraTrack;
  RTCRtpSender? _videoSender;
  final Set<String> _reused = {};
  final List<RTCIceCandidate> _pendingIce = [];
  bool _remoteSet = false;
  bool _makingOffer = false;
  Timer? _timer;
  int _seq = 0;

  Future<void> ensureRenderers() async {
    if (_renderersReady) return;
    await remoteRenderer.initialize();
    await localRenderer.initialize();
    await screenRenderer.initialize();
    _renderersReady = true;
  }

  // ------------------------------------------------------------------ ações do usuário

  /// Liga para `peerFp`. Nada é capturado ainda: primeiro o outro lado precisa aceitar.
  Future<void> call(String peerFp, String name, CallKind k) async {
    if (phase != CallPhase.idle) return;
    await ensureRenderers();
    _begin(
      peerFp,
      name,
      k,
      caller: true,
      id: '${DateTime.now().microsecondsSinceEpoch}-${_seq++}',
    );
    phase = CallPhase.outgoing;
    notifyListeners();
    _timer = Timer(_ringTimeout, () => _end('sem resposta', notify: true));
    final callId = _id;
    await warmUp();
    // Cancelou enquanto media.
    if (phase != CallPhase.outgoing || _id != callId) return;
    // Já se sabe que com este contato só conecta se quem atende oferecer: pede isso no convite.
    if (_calleeOffersFor(peerFp, iAmCaller: true)) {
      _calleeOfferedFirst = true;
      _offerer = false;
    }
    try {
      await transport.send(peerFp, {
        't': 'invite',
        'id': _id,
        'kind': k.name,
        if (_calleeOfferedFirst) 'so': 1,
        ..._netFields,
      });
    } catch (e) {
      await _end('não foi possível chamar: $e');
    }
  }

  /// Atende a chamada que está tocando.
  Future<void> accept() async {
    if (phase != CallPhase.incoming) return;
    _timer?.cancel();
    phase = CallPhase.connecting;
    _connectWatch = Stopwatch()..start();
    notifyListeners();
    _timer = Timer(
      _attemptTimeout(),
      () => _onConnectFailed('a conexão não abriu'),
    );
    await warmUp();
    // O outro lado desligou enquanto media.
    if (phase != CallPhase.connecting) return;
    // Memória do que funcionou com este contato; sem ela, quem está atrás de CGNAT envia
    // primeiro (o CGNAT só deixa passar o que responde a algo que ele mesmo enviou). Com IPv6
    // global nas duas pontas não há NAT no caminho, e a ordem padrão serve.
    final remembered = _rememberedCalleeOffers(peer!, iAmCaller: false);
    final bothV6 = _mine.v6 && _peerV6;
    final byNet = _mine.cgnat && !_peerCgnat && !bothV6;
    if (remembered ?? byNet) {
      if (remembered == null) {
        _log(
          'este lado está atrás de CGNAT e o outro não: atendente oferece primeiro',
        );
      }
      _calleeOfferedFirst = true;
    }
    if (_calleeOfferedFirst) _offerer = true;
    await _send({
      't': 'accept',
      if (_calleeOfferedFirst) 'o': 1,
      ..._netFields,
    });
    if (_offerer) await _startAsCaller();
  }

  Future<void> reject() async {
    if (phase != CallPhase.incoming) return;
    await _send({'t': 'reject'});
    await _end('recusada', notify: false);
  }

  /// Desliga (ou cancela, se ainda está chamando).
  Future<void> hangup() async {
    if (phase == CallPhase.idle) return;
    await _send({'t': 'hangup'});
    await _end('encerrada', notify: false);
  }

  void toggleMic() {
    micOn = !micOn;
    for (final t in _mic?.getAudioTracks() ?? const <MediaStreamTrack>[]) {
      t.enabled = micOn;
    }
    notifyListeners();
  }

  void toggleCamera() {
    camOn = !camOn;
    for (final t in _mic?.getVideoTracks() ?? const <MediaStreamTrack>[]) {
      t.enabled = camOn;
    }
    notifyListeners();
  }

  /// Começa a transmitir a tela (o usuário escolhe a fonte). Numa chamada com câmera troca a
  /// imagem; numa só de áudio acrescenta um vídeo e renegocia.
  Future<void> startShare() async {
    if (audioOnlyPath) {
      _log('compartilhar a tela: caminho por relay público, só áudio');
      return;
    }
    final pc = _pc;
    if (pc == null || sharing || phase != CallPhase.connected) return;
    final stream = await media.screen();
    if (stream == null || stream.getVideoTracks().isEmpty) return;
    _screen = stream;
    final track = stream.getVideoTracks().first;
    track.onEnded = () => stopShare();
    final sender = _videoSender;
    if (sender != null) {
      _log('tela no meio da chamada: trocando a faixa do envio de vídeo');
      await sender.replaceTrack(track);
      _log(
        'troca da faixa concluída: envio com a faixa ${sender.track?.id} '
        '(tela ${track.id}, câmera ${_cameraTrack?.id})',
      );
    } else {
      _log('tela no meio da chamada: acrescentando vídeo e renegociando');
      _videoSender = await _addScreenTrack(pc, stream);
      await _negotiate();
      _log('renegociação enviada');
    }
    sharing = true;
    screenRenderer.srcObject = stream;
    notifyListeners();
    unawaited(_tuneScreenSender());
    unawaited(_logOutboundVideo('depois de iniciar a tela'));
  }

  /// Registra o tamanho dos quadros de vídeo que de fato saem (a tela é bem maior que a câmera):
  /// mostra se a troca de faixa valeu.
  Future<void> _logOutboundVideo(String when) async {
    await Future<void>.delayed(const Duration(seconds: 3));
    try {
      for (final r in await stats_()) {
        if (r.type == 'outbound-rtp' && r.values['kind'] == 'video') {
          _log(
            '$when: enviando ${r.values['frameWidth']}x${r.values['frameHeight']}, '
            '${r.values['framesSent']} quadros, ${r.values['framesPerSecond']} fps',
          );
        }
      }
    } catch (e) {
      _log('estatísticas de envio: $e');
    }
  }

  Future<void> stopShare() async {
    if (!sharing) return;
    sharing = false;
    final screen = _screen;
    _screen = null;
    try {
      _log('parando a tela: devolvendo a câmera (${_cameraTrack != null})');
      await _videoSender?.replaceTrack(_cameraTrack);
    } catch (e) {
      _log('falha ao devolver a câmera: $e');
    }
    for (final t in screen?.getTracks() ?? const <MediaStreamTrack>[]) {
      await t.stop();
    }
    screenRenderer.srcObject = null;
    notifyListeners();
  }

  /// Registra por onde a mídia está indo (host/srflx/prflx/relay). Espera um instante: logo após
  /// "connected" as estatísticas do par ainda podem não ter o par escolhido.
  Future<void> _logSelectedPair() async {
    for (var attempt = 0; attempt < 4; attempt++) {
      await Future<void>.delayed(
        Duration(milliseconds: attempt == 0 ? 800 : 1500),
      );
      try {
        final stats = await stats_();
        _sampler.sample(_items(stats));
        final byId = {for (final r in stats) r.id: r};
        String cand(Object? id) {
          final c = byId[id];
          return c == null
              ? '?'
              : '${c.values['candidateType']} ${c.values['address'] ?? c.values['ip']}:${c.values['port']} '
                    '(${c.values['protocol']}, ${'${c.values['address'] ?? c.values['ip']}'.contains(':') ? 'IPv6' : 'IPv4'})';
        }

        final pairs = stats.where((r) => r.type == 'candidate-pair').toList();
        // O par em uso: nomeado/selecionado, ou o que de fato enviou bytes.
        final used = pairs.where(
          (r) =>
              r.values['state'] == 'succeeded' &&
              (r.values['nominated'] == true ||
                  r.values['selected'] == true ||
                  ((r.values['bytesSent'] as num?) ?? 0) > 0),
        );
        if (used.isNotEmpty) {
          for (final r in used) {
            _log(
              'par escolhido: local=${cand(r.values['localCandidateId'])} '
              'remoto=${cand(r.values['remoteCandidateId'])} '
              'rtt=${r.values['currentRoundTripTime']}',
            );
          }
          return;
        }
        if (attempt == 3) {
          _log(
            'sem par escolhido nas estatísticas; pares: ${pairs.map((p) => '${p.values['state']}').toList()}',
          );
        }
      } catch (e) {
        _log('não consegui ler o par escolhido: $e');
      }
    }
  }

  Timer? _statsTimer;

  /// Tela: mira em 30 quadros por segundo, com texto legível. Captura a 30 fps e pede "manter
  /// resolução" (sem isso o WebRTC começa em 640x360, texto borrado). No Linux o plugin ignora
  /// `maxBitrate`/`scaleResolutionDownBy` do sender (medido no campo): o limite de bitrate que
  /// vale é o do SDP (`boostVideoBitrate`). Não há como garantir 30 fps numa rede lenta.
  Future<void> _tuneScreenSender() async {
    final sender = _videoSender;
    if (sender == null) return;
    try {
      // Largura real capturada, para reduzir só o que passar de 1920.
      var width = 1920.0;
      final settings = _screen?.getVideoTracks().firstOrNull?.getSettings();
      final w = settings?['width'];
      if (w is num) width = w.toDouble();
      final p = sender.parameters;
      p.degradationPreference = RTCDegradationPreference.MAINTAIN_RESOLUTION;
      final enc = (p.encodings != null && p.encodings!.isNotEmpty)
          ? p.encodings!.first
          : RTCRtpEncoding();
      enc
        ..maxBitrate = 8000000
        ..maxFramerate = 30
        ..scaleResolutionDownBy = width > 1920 ? width / 1920 : 1.0;
      p.encodings = [enc];
      final ok = await sender.setParameters(p);
      _log(
        'afinando a tela (30 fps, ${width.round()}→≤1920 px, até 8 Mbit/s, manter resolução): '
        '${ok ? 'ok' : 'recusado'}',
      );
    } catch (e) {
      _log('não consegui afinar o envio da tela: $e');
    }
  }

  /// A cada poucos segundos anota resolução, quadros por segundo e taxa do vídeo (diagnóstico).
  void _startStatsLog() {
    _statsTimer?.cancel();
    _statsTimer = Timer.periodic(const Duration(seconds: 4), (_) async {
      try {
        final all = await stats_();
        _sampler.sample(_items(all));
        await _sampleBridge();
        for (final r in all) {
          final kindV = r.values['kind'] ?? r.values['mediaType'];
          if (kindV != 'video') continue;
          if (r.type == 'outbound-rtp') {
            _log(
              'vídeo enviado: ${r.values['frameWidth']}x${r.values['frameHeight']} '
              '${r.values['framesPerSecond']} fps, ${r.values['bytesSent']} B, '
              'alvo ${r.values['targetBitrate']} bps, '
              'limitação: ${r.values['qualityLimitationReason']}',
            );
          } else if (r.type == 'inbound-rtp') {
            _log(
              'vídeo recebido: ${r.values['frameWidth']}x${r.values['frameHeight']} '
              '${r.values['framesPerSecond']} fps, ${r.values['bytesReceived']} B, '
              'decodificados ${r.values['framesDecoded']}',
            );
          }
        }
      } catch (_) {}
    });
  }

  /// Lê as medidas da ponte e aplica o limite de áudio quando o iroh está no relay público.
  Future<void> _sampleBridge() async {
    final b = bridge;
    if (b == null || !_bridgeUp) return;
    try {
      final st = await b.status();
      if (st == null) return;
      _bridgeStatus = st;
      final onBridge = _sampler.last?.ladder == Ladder.relay;
      final limit = onBridge && st.direct == false && kind != CallKind.audio;
      if (limit != audioOnlyPath) {
        audioOnlyPath = limit;
        _log(
          limit
              ? 'caminho por relay público: só áudio (vídeo e tela pausados)'
              : 'caminho direto: vídeo liberado',
        );
        for (final t in _mic?.getVideoTracks() ?? const <MediaStreamTrack>[]) {
          t.enabled = !limit && camOn;
        }
        notifyListeners();
      }
    } catch (_) {}
  }

  Future<List<StatsReport>> stats_() async => await _pc?.getStats() ?? const [];

  /// Estatísticas WebRTC (testes e diagnóstico).
  Future<List<StatsReport>> stats() async => await _pc?.getStats() ?? const [];

  // ------------------------------------------------------------------ sinais recebidos

  Future<void> _onSignal(IncomingSignal s) async {
    final m = s.msg;
    final t = m['t'];
    final id = m['id'];
    _log('recebido $t (fase $phase)');
    if (t == 'invite') {
      if (phase != CallPhase.idle) {
        unawaited(
          transport.send(s.peer, {'t': 'busy', 'id': id}).catchError((_) {}),
        );
        return;
      }
      final k = CallKind.values.where((k) => k.name == m['kind']).firstOrNull;
      if (k == null || id is! String) return;
      await ensureRenderers();
      _begin(s.peer, s.name, k, caller: false, id: id);
      _calleeOfferedFirst = m['so'] == 1;
      _peerCgnat = m['cg'] == 1;
      _peerV6 = m['v6'] == 1;
      unawaited(warmUp());
      phase = CallPhase.incoming;
      _timer = Timer(_ringTimeout, () => _end('perdida', notify: false));
      notifyListeners();
      return;
    }
    // Todo o resto só vale para a chamada atual, do contato certo.
    if (id != _id || s.peer != peer || phase == CallPhase.idle) return;
    if (t != 'swap' && ((m['a'] as int?) ?? 0) != _attempt) {
      _log('ignorado: $t da tentativa ${m['a']} (agora $_attempt)');
      return;
    }
    switch (t) {
      case 'accept':
        if (_caller && phase == CallPhase.outgoing) {
          _timer?.cancel();
          phase = CallPhase.connecting;
          _connectWatch = Stopwatch()..start();
          _timer = Timer(
            _attemptTimeout(),
            () => _onConnectFailed('a conexão não abriu'),
          );
          notifyListeners();
          _peerCgnat = m['cg'] == 1;
          _peerV6 = m['v6'] == 1;
          if (m['o'] == 1) {
            // Quem atendeu vai oferecer: este lado espera a oferta.
            _calleeOfferedFirst = true;
            _offerer = false;
          } else {
            _calleeOfferedFirst = false;
            _offerer = true;
            await _startAsCaller();
          }
        }
      case 'reject':
        await _end('recusada pelo outro lado', notify: false);
      case 'preparing':
        peerPreparing = true;
        notifyListeners();
      case 'busy':
        await _end('ocupado', notify: false);
      case 'hangup':
        await _end('encerrada pelo outro lado', notify: false);
      case 'swap':
        await _onSwap(m);
      case 'offer':
        await _onOffer(m);
      case 'answer':
        await _onAnswer(m);
      case 'ice':
        await _onIce(m);
    }
  }

  // ------------------------------------------------------------------ WebRTC

  void _begin(
    String peerFp,
    String name,
    CallKind k, {
    required bool caller,
    required String id,
  }) {
    peer = peerFp;
    peerName = name;
    kind = k;
    _caller = caller;
    _offerer = caller;
    _calleeOfferedFirst = false;
    _peerCgnat = false;
    _peerV6 = false;
    _connectWatch = _connectedWatch = null;
    _msToConnect = null;
    _sampler = PathSampler();
    _attempt = 0;
    retrying = false;
    _id = id;
    endedReason = null;
    micOn = true;
    camOn = true;
    sharing = false;
    remoteHasVideo = false;
    peerPreparing = false;
    _remoteSet = false;
    _pendingIce.clear();
  }

  Future<RTCPeerConnection> _newPeerConnection() async {
    _log('criando a conexão WebRTC');
    final servers = [...iceServers];
    final b = bridge;
    if (b != null) {
      // Falhar aqui não impede a chamada: segue só com o ICE direto (comportamento anterior).
      try {
        final info = await b.start(peer!, caller: _caller);
        servers.add({
          'urls': info.url,
          'username': info.username,
          'credential': info.credential,
        });
        _bridgeUp = true;
        _log('ponte de mídia pronta');
      } catch (e) {
        _bridgeUp = false;
        _log('ponte de mídia indisponível: $e');
      }
    }
    final pc = await createPeerConnection({
      'iceServers': servers,
      'sdpSemantics': 'unified-plan',
    });
    _log('conexão WebRTC criada');
    pc.onIceCandidate = (c) {
      if (c.candidate == null || c.candidate!.isEmpty) return;
      unawaited(
        _send({
          't': 'ice',
          'c': c.candidate,
          'mid': c.sdpMid,
          'line': c.sdpMLineIndex,
        }).catchError((_) {}),
      );
    };
    pc.onTrack = (e) async {
      _log(
        'faixa remota recebida: ${e.track.kind}, fluxos ${e.streams.map((x) => x.id).toList()}',
      );
      final stream = e.streams.isNotEmpty ? e.streams.first : null;
      if (e.track.kind == 'video') {
        remoteHasVideo = true;
        remoteRenderer.srcObject =
            stream ?? await createLocalMediaStream('remote')
              ..addTrack(e.track);
      } else if (remoteRenderer.srcObject == null && stream != null) {
        remoteRenderer.srcObject = stream;
      }
      notifyListeners();
    };
    pc.onIceGatheringState = (s) => _log('ICE gathering: $s');
    pc.onIceConnectionState = (s) => _log('ICE connection: $s');
    pc.onSignalingState = (s) => _log('signaling: $s');
    RTCPeerConnectionState?
    latest; // o plugin nem sempre preenche `pc.connectionState`
    pc.onConnectionState = (s) {
      latest = s;
      _log('connection: $s');
      if (s == RTCPeerConnectionState.RTCPeerConnectionStateConnected &&
          phase == CallPhase.connecting) {
        _timer?.cancel();
        retrying = false;
        _rememberRole();
        _msToConnect = _connectWatch?.elapsedMilliseconds;
        _connectedWatch = Stopwatch()..start();
        phase = CallPhase.connected;
        notifyListeners();
        unawaited(_logSelectedPair());
        _startStatsLog();
        if (sharing) unawaited(_tuneScreenSender());
      } else if (s == RTCPeerConnectionState.RTCPeerConnectionStateFailed) {
        // `Failed` logo depois do `Checking` é comum enquanto os candidatos do outro lado ainda
        // chegam (campo, 2026-10-26: 6 ms após a resposta, e o `swap` saiu antes do 1º candidato);
        // o ICE se recupera quando chegam. Só vale se continuar falho depois de uma folga.
        Timer(_failedGrace, () {
          if (_pc == pc && phase == CallPhase.connecting) {
            if (latest == RTCPeerConnectionState.RTCPeerConnectionStateFailed) {
              _onConnectFailed('a conexão falhou');
            }
          }
        });
      }
    };
    return pc;
  }

  /// O que cada lado envia: áudio e/ou câmera; na chamada de tela o chamador também transmite a
  /// tela e os dois falam (microfone, se houver: sem ele a chamada segue só com a tela).
  /// Com `answering`, a oferta do outro lado já foi aplicada: as faixas locais entram nos
  /// transceptores que ela criou (um `addTransceiver` antes da oferta, no Windows, ficava num
  /// transceptor novo e a resposta saía sem enviar o microfone até a renegociação seguinte).
  Future<void> _attachLocalMedia(
    RTCPeerConnection pc, {
    bool answering = false,
  }) async {
    MediaStream? local = _mic; // na segunda tentativa as faixas já existem
    _log('capturando a mídia local (${kind!.name})');
    if (local != null) {
      _log('reaproveitando a mídia local da tentativa anterior');
    } else {
      switch (kind!) {
        case CallKind.audio:
          local = await media.microphone();
        case CallKind.video:
          local = await media.camera();
        case CallKind.screen:
          try {
            local = await media.microphone();
          } catch (e) {
            _log('sem microfone na chamada de tela: $e');
          }
      }
    }
    _log(
      'mídia local capturada: ${local?.getTracks().map((t) => t.kind).toList()}',
    );
    if (local != null) {
      _mic = local;
      for (final t in local.getTracks()) {
        _log('adicionando a faixa ${t.kind} à conexão');
        // `addTrack` derruba o app no Windows (queda nativa, vista no campo); o transceptor com a
        // faixa faz o mesmo e é o caminho que a tela já usa.
        RTCRtpTransceiver? tr;
        if (answering) {
          for (final c in await pc.getTransceivers()) {
            if (c.sender.track == null &&
                c.receiver.track?.kind == t.kind &&
                !_reused.contains(c.mid)) {
              tr = c;
              break;
            }
          }
          if (tr != null) {
            _reused.add(tr.mid);
            await tr.sender.replaceTrack(t);
            await tr.setDirection(TransceiverDirection.SendRecv);
            _log('faixa ${t.kind} no transceptor ${tr.mid} da oferta');
          }
        }
        tr ??= await pc.addTransceiver(
          track: t,
          kind: t.kind == 'video'
              ? RTCRtpMediaType.RTCRtpMediaTypeVideo
              : RTCRtpMediaType.RTCRtpMediaTypeAudio,
          init: RTCRtpTransceiverInit(
            direction: TransceiverDirection.SendRecv,
            streams: [local],
          ),
        );
        final s = tr.sender;
        _log('faixa ${t.kind} adicionada');
        if (t.kind == 'video') {
          _videoSender = s;
          _cameraTrack = t;
        }
      }
    }
    if (kind == CallKind.screen) {
      if (_caller) {
        final screen = _screen ?? await media.screen();
        if (screen == null) throw StateError('transmissão de tela cancelada');
        _screen = screen;
        sharing = true;
        screenRenderer.srcObject = screen;
        RTCRtpTransceiver? shared;
        if (answering) {
          // Papéis invertidos: quem transmite agora responde, e a oferta já tem o m-line de vídeo.
          for (final c in await pc.getTransceivers()) {
            if (c.sender.track == null && c.receiver.track?.kind == 'video') {
              shared = c;
              break;
            }
          }
        }
        if (shared != null) {
          await shared.sender.replaceTrack(screen.getVideoTracks().first);
          await shared.setDirection(TransceiverDirection.SendRecv);
          _videoSender = shared.sender;
          _log('tela no transceptor ${shared.mid} da oferta');
        } else {
          _videoSender = await _addScreenTrack(pc, screen);
        }
      } else if (!answering) {
        // Quem só recebe a tela do outro precisa de um transceptor de vídeo para ela.
        await pc.addTransceiver(
          kind: RTCRtpMediaType.RTCRtpMediaTypeVideo,
          init: RTCRtpTransceiverInit(direction: TransceiverDirection.RecvOnly),
        );
      }
      return;
    }
    // O renderizador é de vídeo: com só áudio não há o que mostrar (e no Windows isso é um risco).
    if (local != null && local.getVideoTracks().isNotEmpty) {
      _log('ligando a prévia local');
      localRenderer.srcObject = local;
      _log('prévia local ligada');
    }
  }

  /// Cria o envio da tela já com as codificações desejadas: o ajuste depois da negociação
  /// (`setParameters`) é ignorado pelo plugin no Linux, o do transceptor é respeitado.
  Future<RTCRtpSender> _addScreenTrack(
    RTCPeerConnection pc,
    MediaStream screen,
  ) async {
    final track = screen.getVideoTracks().first;
    // O plugin não informa o tamanho da captura em `getSettings()`: mede pelo primeiro quadro
    // desenhado no renderer local.
    screenRenderer.srcObject = screen;
    var w = 0;
    for (var i = 0; i < 30 && w == 0; i++) {
      w = screenRenderer.videoWidth;
      if (w == 0) await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    final scale = (w > screenMaxWidth) ? w / screenMaxWidth : 1.0;
    _log(
      'tela: $w px de largura capturada, reduzindo por ${scale.toStringAsFixed(2)}',
    );
    final t = await pc.addTransceiver(
      track: track,
      kind: RTCRtpMediaType.RTCRtpMediaTypeVideo,
      init: RTCRtpTransceiverInit(
        direction: TransceiverDirection.SendRecv,
        streams: [screen],
        sendEncodings: [
          RTCRtpEncoding(
            maxBitrate: 8000000,
            maxFramerate: 30,
            scaleResolutionDownBy: scale,
          ),
        ],
      ),
    );
    return t.sender;
  }

  Future<void> _startAsCaller() async {
    _log('chamador: abrindo a mídia');
    // O outro lado vê "escolhendo o que transmitir" em vez de ficar sem informação.
    if (_attempt == 0) unawaited(_send({'t': 'preparing'}).catchError((_) {}));
    try {
      final pc = _pc = await _newPeerConnection();
      await _attachLocalMedia(pc);
      _log('chamador: mídia pronta, gerando oferta');
      await _negotiate();
      _log('chamador: oferta enviada');
    } catch (e) {
      await _send({'t': 'hangup'}).catchError((_) {});
      await _end('falha ao iniciar a mídia: $e', notify: false);
    }
  }

  /// Gera uma oferta e a envia (inicial ou renegociação).
  Future<void> _negotiate() async {
    final pc = _pc;
    if (pc == null) return;
    try {
      _makingOffer = true;
      _log('criando a oferta');
      final created = await pc.createOffer();
      _log('oferta criada');
      final offer = RTCSessionDescription(
        boostVideoBitrate(created.sdp ?? ''),
        created.type,
      );
      await pc.setLocalDescription(offer);
      _log('oferta aplicada localmente');
      await _send({'t': 'offer', 'sdp': offer.sdp, 'type': offer.type});
    } finally {
      _makingOffer = false;
    }
  }

  Future<void> _onOffer(Map<String, dynamic> m) async {
    try {
      peerPreparing = false;
      var pc = _pc;
      var fresh = false;
      if (pc == null) {
        // Primeira oferta: quem atendeu monta a conexão e a mídia agora.
        pc = _pc = await _newPeerConnection();
        fresh = true;
      }
      // `signalingState` é nulo numa conexão nova (nenhum evento ainda): conta como estável.
      final st = pc.signalingState;
      final collision =
          _makingOffer ||
          (st != null && st != RTCSignalingState.RTCSignalingStateStable);
      // Atendente é "educado": cede em caso de oferta simultânea; o chamador ignora a do outro.
      if (collision && _offerer) return;
      if (collision) {
        await pc.setLocalDescription(RTCSessionDescription('', 'rollback'));
      }
      await pc.setRemoteDescription(
        RTCSessionDescription(m['sdp'] as String, m['type'] as String),
      );
      _remoteSet = true;
      _log(
        'atendente: oferta aplicada, ${_pendingIce.length} candidatos pendentes',
      );
      await _flushIce(pc);
      if (fresh) await _attachLocalMedia(pc, answering: true);
      final created = await pc.createAnswer();
      final answer = RTCSessionDescription(
        boostVideoBitrate(created.sdp ?? ''),
        created.type,
      );
      await pc.setLocalDescription(answer);
      await _send({'t': 'answer', 'sdp': answer.sdp, 'type': answer.type});
    } catch (e, st) {
      debugPrint('[call] falha na oferta: $e\n$st');
      await _send({'t': 'hangup'}).catchError((_) {});
      await _end('falha na negociação: $e', notify: false);
    }
  }

  Future<void> _onAnswer(Map<String, dynamic> m) async {
    final pc = _pc;
    if (pc == null) return;
    try {
      await pc.setRemoteDescription(
        RTCSessionDescription(m['sdp'] as String, m['type'] as String),
      );
      _remoteSet = true;
      await _flushIce(pc);
    } catch (e) {
      await _end('falha na negociação: $e', notify: true);
    }
  }

  Future<void> _onIce(Map<String, dynamic> m) async {
    final c = RTCIceCandidate(
      m['c'] as String?,
      m['mid'] as String?,
      m['line'] as int?,
    );
    final pc = _pc;
    if (pc == null || !_remoteSet) {
      _pendingIce.add(c);
    } else {
      await pc.addCandidate(c);
    }
  }

  Future<void> _flushIce(RTCPeerConnection pc) async {
    // Cópia: adicionar candidatos pode derrubar a conexão (e `_dropConnection` limpa a lista).
    final pending = List.of(_pendingIce);
    _pendingIce.clear();
    for (final c in pending) {
      if (_pc != pc) return;
      await pc.addCandidate(c);
    }
  }

  Future<void> _send(Map<String, dynamic> msg) async {
    final p = peer;
    if (p == null) return;
    final sw = Stopwatch()..start();
    try {
      await transport.send(p, {...msg, 'id': _id, 'a': _attempt});
      _log('enviado ${msg['t']} em ${sw.elapsedMilliseconds} ms');
    } catch (e) {
      _log('FALHA ao enviar ${msg['t']} após ${sw.elapsedMilliseconds} ms: $e');
      rethrow;
    }
  }

  // ------------------------------------------------------------------ inversão de papéis

  /// A conexão não abriu (ICE falhou ou o prazo estourou). Na primeira tentativa, quem ofereceu
  /// manda `swap` e os papéis se invertem: com NAT de rede móvel (CGNAT) o resultado depende de
  /// quem abre primeiro (no campo, Linux→Windows conectava 2/2 e Windows→Linux 0/2). Quem
  /// respondeu espera o `swap` em vez de desligar por conta própria.
  Duration _attemptTimeout() =>
      _attempt == 0 ? _firstAttemptTimeout : _connectTimeout;

  Future<void> _onConnectFailed(String reason) async {
    if (phase != CallPhase.connecting) {
      await _end(reason, notify: true);
      return;
    }
    if (_attempt > 0) {
      await _end(_noPathMessage(), notify: true);
      return;
    }
    if (!_offerer) {
      // Dá tempo ao outro lado para detectar a falha e mandar o `swap`.
      _timer?.cancel();
      _timer = Timer(
        const Duration(seconds: 20),
        () => _end(_noPathMessage(), notify: true),
      );
      return;
    }
    _log('a conexão não abriu: invertendo os papéis (tentativa 2)');
    _attempt = 1;
    _offerer = false;
    retrying = true;
    _timer?.cancel();
    _timer = Timer(
      _connectTimeout,
      () => _onConnectFailed('a conexão não abriu'),
    );
    await _dropConnection();
    notifyListeners();
    try {
      await _send({'t': 'swap'});
    } catch (e) {
      await _end('a conexão não abriu', notify: false);
    }
  }

  /// O outro lado pediu a inversão: este lado passa a mandar a oferta.
  Future<void> _onSwap(Map<String, dynamic> m) async {
    if (phase != CallPhase.connecting || _attempt != 0 || _offerer) return;
    _log('recebido swap: passando a oferecer (tentativa 2)');
    _attempt = (m['a'] as int?) ?? 1;
    _offerer = true;
    retrying = true;
    _timer?.cancel();
    _timer = Timer(
      _connectTimeout,
      () => _onConnectFailed('a conexão não abriu'),
    );
    await _dropConnection();
    notifyListeners();
    await _startAsCaller();
  }

  /// Fecha a conexão WebRTC da tentativa anterior e libera as faixas locais: a próxima tentativa
  /// captura de novo, como uma chamada nova. Reaproveitar o microfone entre conexões derrubou o
  /// app no Windows (`libwebrtc.dll` 0xc0000005, ~1 s depois de conectar). Na chamada de tela quem
  /// transmite escolhe a tela outra vez.
  Future<void> _stopBridge() async {
    final b = bridge;
    if (b == null || !_bridgeUp) return;
    _bridgeUp = false;
    _bridgeStatus = null;
    audioOnlyPath = false;
    try {
      await b.stop();
    } catch (e) {
      _log('fechar a ponte de mídia: $e');
    }
  }

  Future<void> _dropConnection() async {
    final pc = _pc;
    _pc = null;
    _statsTimer?.cancel();
    _videoSender = null;
    _cameraTrack = null;
    _reused.clear();
    _pendingIce.clear();
    _remoteSet = false;
    _makingOffer = false;
    for (final stream in [_mic, _screen]) {
      for (final t in stream?.getTracks() ?? const <MediaStreamTrack>[]) {
        try {
          await t.stop();
        } catch (e) {
          _log('parar a faixa: $e');
        }
      }
      try {
        await stream?.dispose();
      } catch (e) {
        _log('liberar o fluxo: $e');
      }
    }
    _mic = _screen = null;
    sharing = false;
    localRenderer.srcObject = null;
    screenRenderer.srcObject = null;
    try {
      await pc?.dispose();
    } catch (e) {
      _log('dispose da conexão: $e');
    }
    await pc?.close();
    await _stopBridge();
    _log('conexão da tentativa anterior fechada');
  }

  // ------------------------------------------------------------------ fim

  Future<void> _end(String reason, {bool notify = false}) async {
    if (phase == CallPhase.idle) return;
    if (notify) {
      // Falha local: avisa o outro lado para ele não ficar pendurado.
      unawaited(_send({'t': 'hangup'}).catchError((_) {}));
    }
    _log('encerrando a chamada: $reason');
    _timer?.cancel();
    _statsTimer?.cancel();
    await _recordMetrics(reason);
    final pc = _pc;
    _pc = null;
    for (final s in [_mic, _screen]) {
      // Uma falha ao liberar não pode deixar a chamada pendurada: o resto do encerramento segue.
      for (final t in s?.getTracks() ?? const <MediaStreamTrack>[]) {
        _log('parando a faixa ${t.kind}');
        try {
          await t.stop();
        } catch (e) {
          _log('parar a faixa: $e');
        }
      }
      _log('liberando o fluxo local');
      try {
        await s?.dispose();
      } catch (e) {
        _log('liberar o fluxo: $e');
      }
    }
    _mic = _screen = null;
    _cameraTrack = _videoSender = null;
    _reused.clear();
    _log('fechando a conexão');
    // O plugin só solta o observador nativo (e seu canal de eventos) em `dispose()`, e só enquanto
    // a conexão ainda está no mapa: o `close()` a tira de lá. Sem isto cada chamada deixa um
    // observador para trás; no Windows a 2ª ou 3ª chamada do processo caía (corrupção de heap).
    try {
      await pc?.dispose();
    } catch (e) {
      _log('dispose da conexão: $e');
    }
    await pc?.close();
    await _stopBridge();
    _log('conexão fechada');
    remoteRenderer.srcObject = null;
    localRenderer.srcObject = null;
    screenRenderer.srcObject = null;
    phase = CallPhase.idle;
    peerPreparing = false;
    retrying = false;
    endedReason = reason;
    sharing = false;
    remoteHasVideo = false;
    _id = null;
    notifyListeners();
  }

  bool _disposed = false;

  /// Um sinal atrasado pode encerrar a chamada depois do `dispose()`: não notifica ninguém.
  @override
  void notifyListeners() {
    if (!_disposed) super.notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _sub.cancel();
    _timer?.cancel();
    remoteRenderer.dispose();
    localRenderer.dispose();
    screenRenderer.dispose();
    super.dispose();
  }
}

/// Pede ao libwebrtc um limite de bitrate de vídeo maior na negociação. O padrão (~2,5 Mbit/s
/// para 1440p) faz o encoder descartar quadros mesmo com banda de sobra: no campo, 21 fps na
/// captura viravam 4 fps codificados com 5 Mbit/s estimados livres. Cada lado aplica isto ao seu
/// SDP local, então quem envia vê o `b=AS` e o `x-google-*` no SDP remoto.
String boostVideoBitrate(
  String sdp, {
  int minKbps = 2000,
  int startKbps = 4000,
  int maxKbps = 8000,
}) {
  final lines = sdp.split(RegExp(r'\r?\n'));
  final out = <String>[];
  var inVideo = false;
  var wroteBandwidth = false;
  final boost =
      'x-google-min-bitrate=$minKbps;x-google-start-bitrate=$startKbps;'
      'x-google-max-bitrate=$maxKbps';
  for (final line in lines) {
    if (line.startsWith('m=')) {
      inVideo = line.startsWith('m=video');
      wroteBandwidth = false;
    }
    if (inVideo && line.startsWith('b=AS:')) continue; // refeito logo após o c=
    if (inVideo &&
        line.startsWith('a=fmtp:') &&
        !line.contains('apt=') &&
        !line.contains('x-google-')) {
      out.add('$line;$boost');
      continue;
    }
    out.add(line);
    // `b=` vem logo depois de `c=` na seção de vídeo.
    if (inVideo && !wroteBandwidth && line.startsWith('c=')) {
      out.add('b=AS:$maxKbps');
      wroteBandwidth = true;
    }
  }
  return out.join('\r\n');
}

/// Transporte pelo núcleo: JSON dentro de um sinal E2E (`send_call_signal`).
String encodeSignal(Map<String, dynamic> m) => jsonEncode(m);
Map<String, dynamic>? decodeSignal(String s) {
  try {
    final v = jsonDecode(s);
    return v is Map<String, dynamic> ? v : null;
  } catch (_) {
    return null;
  }
}
