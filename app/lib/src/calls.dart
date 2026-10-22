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

  final SignalTransport transport;
  final MediaProvider media;
  final List<Map<String, dynamic>> iceServers;
  late final StreamSubscription<IncomingSignal> _sub;

  final RTCVideoRenderer remoteRenderer = RTCVideoRenderer();
  final RTCVideoRenderer localRenderer = RTCVideoRenderer();
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
  RTCPeerConnection? _pc;
  MediaStream? _mic; // áudio (e câmera, se houver)
  MediaStream? _screen;
  MediaStreamTrack? _cameraTrack;
  RTCRtpSender? _videoSender;
  final List<RTCIceCandidate> _pendingIce = [];
  bool _remoteSet = false;
  bool _makingOffer = false;
  Timer? _timer;
  int _seq = 0;

  Future<void> ensureRenderers() async {
    if (_renderersReady) return;
    await remoteRenderer.initialize();
    await localRenderer.initialize();
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
      () => _end('a conexão não abriu', notify: true),
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
      await sender.replaceTrack(track);
    } else {
      _videoSender = await pc.addTrack(track, stream);
      await _negotiate();
    }
    sharing = true;
    localRenderer.srcObject = stream;
    notifyListeners();
  }

  Future<void> stopShare() async {
    if (!sharing) return;
    sharing = false;
    final screen = _screen;
    _screen = null;
    try {
      await _videoSender?.replaceTrack(_cameraTrack);
    } catch (_) {}
    for (final t in screen?.getTracks() ?? const <MediaStreamTrack>[]) {
      await t.stop();
    }
    localRenderer.srcObject = _mic;
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
    switch (t) {
      case 'accept':
        if (_caller && phase == CallPhase.outgoing) {
          _timer?.cancel();
          phase = CallPhase.connecting;
          _timer = Timer(
            _connectTimeout,
            () => _end('a conexão não abriu', notify: true),
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
    final pc = await createPeerConnection({
      'iceServers': iceServers,
      'sdpSemantics': 'unified-plan',
    });
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
        phase = CallPhase.connected;
        notifyListeners();
        unawaited(_logSelectedPair());
        _startStatsLog();
      } else if (s == RTCPeerConnectionState.RTCPeerConnectionStateFailed) {
        _end('a conexão falhou', notify: true);
      }
    };
    return pc;
  }

  /// O que cada lado envia: áudio e/ou câmera; na chamada de tela o chamador também transmite a
  /// tela e os dois falam (microfone, se houver: sem ele a chamada segue só com a tela).
  Future<void> _attachLocalMedia(RTCPeerConnection pc) async {
    MediaStream? local;
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
    if (local != null) {
      _mic = local;
      for (final t in local.getTracks()) {
        final s = await pc.addTrack(t, local);
        if (t.kind == 'video') {
          _videoSender = s;
          _cameraTrack = t;
        }
      }
    }
    if (kind == CallKind.screen) {
      if (_caller) {
        final screen = await media.screen();
        if (screen == null) throw StateError('transmissão de tela cancelada');
        _screen = screen;
        sharing = true;
        localRenderer.srcObject = screen;
        _videoSender = await pc.addTrack(screen.getVideoTracks().first, screen);
      } else {
        // Quem só recebe a tela do outro precisa de um transceptor de vídeo para ela.
        await pc.addTransceiver(
          kind: RTCRtpMediaType.RTCRtpMediaTypeVideo,
          init: RTCRtpTransceiverInit(direction: TransceiverDirection.RecvOnly),
        );
      }
      return;
    }
    if (local != null) localRenderer.srcObject = local;
  }

  Future<void> _startAsCaller() async {
    _log('chamador: abrindo a mídia');
    // O outro lado vê "escolhendo o que transmitir" em vez de ficar sem informação.
    unawaited(_send({'t': 'preparing'}).catchError((_) {}));
    try {
      final pc = _pc = await _newPeerConnection();
      await _attachLocalMedia(pc);
      _log('chamador: mídia pronta, gerando oferta');
      await _negotiate();
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
      final offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      await _send({'t': 'offer', 'sdp': offer.sdp, 'type': offer.type});
    } finally {
      _makingOffer = false;
    }
  }

  Future<void> _onOffer(Map<String, dynamic> m) async {
    try {
      peerPreparing = false;
      var pc = _pc;
      if (pc == null) {
        // Primeira oferta: quem atendeu monta a conexão e a mídia agora.
        pc = _pc = await _newPeerConnection();
        await _attachLocalMedia(pc);
      }
      // `signalingState` é nulo numa conexão nova (nenhum evento ainda): conta como estável.
      final st = pc.signalingState;
      final collision =
          _makingOffer ||
          (st != null && st != RTCSignalingState.RTCSignalingStateStable);
      // Atendente é "educado": cede em caso de oferta simultânea; o chamador ignora a do outro.
      if (collision && _caller) return;
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
      final answer = await pc.createAnswer();
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
      await transport.send(p, {...msg, 'id': _id});
      _log('enviado ${msg['t']} em ${sw.elapsedMilliseconds} ms');
    } catch (e) {
      _log('FALHA ao enviar ${msg['t']} após ${sw.elapsedMilliseconds} ms: $e');
      rethrow;
    }
  }

  // ------------------------------------------------------------------ fim

  Future<void> _end(String reason, {bool notify = false}) async {
    if (phase == CallPhase.idle) return;
    if (notify) {
      // Falha local: avisa o outro lado para ele não ficar pendurado.
      unawaited(_send({'t': 'hangup'}).catchError((_) {}));
    }
    _timer?.cancel();
    _statsTimer?.cancel();
    final pc = _pc;
    _pc = null;
    for (final s in [_mic, _screen]) {
      for (final t in s?.getTracks() ?? const <MediaStreamTrack>[]) {
        await t.stop();
      }
      await s?.dispose();
    }
    _mic = _screen = null;
    _cameraTrack = _videoSender = null;
    await pc?.close();
    remoteRenderer.srcObject = null;
    localRenderer.srcObject = null;
    phase = CallPhase.idle;
    peerPreparing = false;
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
    super.dispose();
  }
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
