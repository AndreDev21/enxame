import 'dart:async';
import 'dart:convert';

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

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

const _ringTimeout = Duration(seconds: 45);
const _connectTimeout = Duration(seconds: 30);

/// Uma chamada 1:1 por vez. Sinalização por mensagens JSON (`invite`, `accept`, `reject`, `busy`,
/// `offer`, `answer`, `ice`, `hangup`); mídia por WebRTC (DTLS-SRTP).
class CallController extends ChangeNotifier {
  CallController({
    required this.transport,
    required this.media,
    this.iceServers = _stun,
    this.screenMaxWidth = 1920,
  }) {
    _sub = transport.incoming.listen(_onSignal);
  }

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
    try {
      await transport.send(peerFp, {'t': 'invite', 'id': _id, 'kind': k.name});
    } catch (e) {
      await _end('não foi possível chamar: $e');
    }
  }

  /// Atende a chamada que está tocando.
  Future<void> accept() async {
    if (phase != CallPhase.incoming) return;
    _timer?.cancel();
    phase = CallPhase.connecting;
    notifyListeners();
    _timer = Timer(
      _connectTimeout,
      () => _onConnectFailed('a conexão não abriu'),
    );
    await _send({'t': 'accept'});
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
        final byId = {for (final r in stats) r.id: r};
        String cand(Object? id) {
          final c = byId[id];
          return c == null
              ? '?'
              : '${c.values['candidateType']} ${c.values['address'] ?? c.values['ip']}:${c.values['port']} (${c.values['protocol']})';
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
        for (final r in await stats_()) {
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
          _timer = Timer(
            _connectTimeout,
            () => _onConnectFailed('a conexão não abriu'),
          );
          notifyListeners();
          await _startAsCaller();
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
    final pc = await createPeerConnection({
      'iceServers': iceServers,
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
    pc.onConnectionState = (s) {
      _log('connection: $s');
      if (s == RTCPeerConnectionState.RTCPeerConnectionStateConnected &&
          phase == CallPhase.connecting) {
        _timer?.cancel();
        retrying = false;
        phase = CallPhase.connected;
        notifyListeners();
        unawaited(_logSelectedPair());
        _startStatsLog();
        if (sharing) unawaited(_tuneScreenSender());
      } else if (s == RTCPeerConnectionState.RTCPeerConnectionStateFailed) {
        _onConnectFailed('a conexão falhou');
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
    for (final c in _pendingIce) {
      await pc.addCandidate(c);
    }
    _pendingIce.clear();
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
  Future<void> _onConnectFailed(String reason) async {
    if (phase != CallPhase.connecting) {
      await _end(reason, notify: true);
      return;
    }
    if (_attempt > 0) {
      await _end(
        'sem caminho direto com este contato (rede móvel ou roteador restritivo): '
        'tente de outra rede',
        notify: true,
      );
      return;
    }
    if (!_offerer) {
      // Dá tempo ao outro lado para detectar a falha e mandar o `swap`.
      _timer?.cancel();
      _timer = Timer(
        const Duration(seconds: 20),
        () => _end('a conexão não abriu', notify: true),
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

  /// Fecha só a conexão WebRTC, mantendo as faixas locais (microfone, câmera, tela) para a
  /// próxima tentativa: a tela não pede o seletor de novo.
  Future<void> _dropConnection() async {
    final pc = _pc;
    _pc = null;
    _statsTimer?.cancel();
    _videoSender = null;
    _reused.clear();
    _pendingIce.clear();
    _remoteSet = false;
    _makingOffer = false;
    try {
      await pc?.dispose();
    } catch (e) {
      _log('dispose da conexão: $e');
    }
    await pc?.close();
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
    final pc = _pc;
    _pc = null;
    for (final s in [_mic, _screen]) {
      for (final t in s?.getTracks() ?? const <MediaStreamTrack>[]) {
        _log('parando a faixa ${t.kind}');
        await t.stop();
      }
      _log('liberando o fluxo local');
      await s?.dispose();
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

  @override
  void dispose() {
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
