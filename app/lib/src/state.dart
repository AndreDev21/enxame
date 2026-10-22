import 'dart:async';

import 'package:flutter/foundation.dart';

import 'calls.dart';
import 'device_media.dart';
import 'rust/api/app.dart' as api;

import 'package:flutter_webrtc/flutter_webrtc.dart' show DesktopCapturerSource;

import 'rust/api/app.dart';

/// Uma conversa na lista: um contato (1:1) ou um grupo.
sealed class Conversation {
  const Conversation();
  String get key;
  String get title;
}

class DmConversation extends Conversation {
  const DmConversation(this.contact);
  final ContactInfo contact;
  @override
  String get key => 'dm:${contact.fingerprint}';
  @override
  String get title => contact.name;
}

class GroupConversation extends Conversation {
  const GroupConversation(this.group);
  final GroupInfo group;
  @override
  String get key => 'g:${group.id}';
  @override
  String get title => group.name;
}

/// Aviso para a interface mostrar uma vez (ex.: entrei num grupo com não-contatos).
class Notice {
  Notice(this.kind, this.title, {this.items = const []});
  final String kind; // 'joined' | 'removed' | 'ip'
  final String title;
  final List<String> items;
}

/// Estado do app: o nó, os dados que a interface mostra e a assinatura dos eventos do núcleo.
class AppState extends ChangeNotifier {
  MyIdentity? me;
  List<ContactInfo> contacts = const [];
  List<ContactRequest> requests = const [];
  List<GroupInfo> groups = const [];
  Contribution? contribution;
  String? selectedKey;
  bool gameMode = false;
  bool contributing = true;

  final Map<String, List<MessageInfo>> _messages = {};
  final Map<String, MessageInfo?> _last = {};
  final Map<String, double> transfers = {};
  final Map<String, FileItem> fileItems = {}; // por id de mensagem
  final Map<String, String> savedAt = {}; // id de mensagem -> caminho baixado
  final Map<String, String> fileErrors = {};
  final List<Notice> notices = [];
  StreamSubscription<AppEvent>? _events;
  Timer? _timer;

  final _signals = StreamController<IncomingSignal>.broadcast();

  /// Chamadas 1:1 (áudio, vídeo e tela). O seletor de tela é fornecido pela interface.
  late final CallController calls = CallController(
    transport: _CoreSignals(_signals.stream),
    media: DeviceMedia(pickScreen: () async => pickScreen?.call()),
  );

  /// A interface registra aqui o diálogo que deixa o usuário escolher a tela ou janela.
  Future<DesktopCapturerSource?> Function()? pickScreen;

  bool get started => me != null;

  /// Conversas: grupos ativos primeiro e contatos mútuos, pelo nome.
  List<Conversation> get conversations => [
    ...groups.where((g) => g.active).map(GroupConversation.new),
    ...contacts.where((c) => c.mutual).map(DmConversation.new),
  ];

  Conversation? get selected {
    for (final c in conversations) {
      if (c.key == selectedKey) return c;
    }
    return null;
  }

  List<MessageInfo> messagesOf(Conversation c) => _messages[c.key] ?? const [];
  MessageInfo? lastOf(Conversation c) => _last[c.key];

  Future<void> start({String? displayName}) async {
    me = await api.startNode(displayName: displayName);
    await refreshAll();
    _events = api.events().listen(_onEvent, onError: (_) {});
    // Rede de segurança: o estado pendente (pedidos, entregas) é recarregado de tempos em tempos.
    _timer = Timer.periodic(const Duration(seconds: 10), (_) => refreshAll());
    notifyListeners();
  }

  Future<void> refreshAll() async {
    try {
      me = await api.myIdentity();
      contacts = await api.contacts();
      requests = await api.pendingRequests();
      groups = await api.groups();
      contribution = await api.contribution();
      for (final c in conversations) {
        await _loadMessages(c, notify: false);
      }
    } catch (_) {
      // O próximo ciclo tenta de novo; erros pontuais não derrubam a interface.
    }
    notifyListeners();
  }

  Future<void> _loadMessages(Conversation c, {bool notify = true}) async {
    final msgs = switch (c) {
      DmConversation(:final contact) => await api.history(
        contact: contact.fingerprint,
        limit: 200,
      ),
      GroupConversation(:final group) => await api.groupHistory(
        group: group.id,
        limit: 200,
      ),
    };
    _messages[c.key] = msgs;
    _last[c.key] = msgs.isEmpty ? null : msgs.last;
    if (c is DmConversation && msgs.any((m) => m.file != null)) {
      for (final f in await api.files(contact: c.contact.fingerprint)) {
        fileItems[f.messageId] = f;
      }
    }
    if (notify) notifyListeners();
  }

  Future<void> select(Conversation? c) async {
    selectedKey = c?.key;
    notifyListeners();
    if (c != null) await _loadMessages(c);
  }

  void _onEvent(AppEvent e) {
    switch (e) {
      case AppEvent_MessageReceived():
        refreshAll();
      case AppEvent_ContactRequested() || AppEvent_ContactAdded():
        refreshAll();
      case AppEvent_RoleChanged():
        refreshAll();
      case AppEvent_Transfer(:final name, :final done, :final total):
        if (total > BigInt.zero) {
          transfers[name] = done.toDouble() / total.toDouble();
          if (done >= total) transfers.remove(name);
        }
        notifyListeners();
      case AppEvent_GroupJoined(:final name, :final exposed):
        notices.add(Notice('joined', name, items: exposed));
        refreshAll();
      case AppEvent_GroupUpdated():
        refreshAll();
      case AppEvent_GroupRemoved(:final name):
        notices.add(Notice('removed', name));
        refreshAll();
      case AppEvent_CallSignal(
        :final fromFingerprint,
        :final fromName,
        :final payload,
      ):
        final m = decodeSignal(payload);
        if (m != null) {
          _signals.add(IncomingSignal(fromFingerprint, fromName, m));
        }
      case AppEvent_Lagged():
        refreshAll();
    }
  }

  Notice? takeNotice() => notices.isEmpty ? null : notices.removeAt(0);

  // ------------------------------------------------------------------ ações

  Future<String?> sendText(Conversation c, String text) async {
    try {
      switch (c) {
        case DmConversation(:final contact):
          await api.sendText(contact: contact.fingerprint, text: text);
        case GroupConversation(:final group):
          await api.sendGroupText(group: group.id, text: text);
      }
      await _loadMessages(c);
      return null;
    } catch (e) {
      return e.toString();
    }
  }

  /// Envia um arquivo ao contato da conversa 1:1 (o grupo ainda não suporta arquivos).
  Future<String?> sendFile(Conversation c, String path) async {
    if (c is! DmConversation) return null;
    try {
      await api.sendFile(contacts: [c.contact.fingerprint], path: path);
      await _loadMessages(c);
      return null;
    } catch (e) {
      return e.toString();
    }
  }

  /// Baixa o arquivo anunciado por `messageId` e guarda onde foi salvo.
  Future<void> download(DmConversation c, String messageId) async {
    final item = fileItems[messageId];
    if (item == null) return;
    fileErrors.remove(messageId);
    try {
      savedAt[messageId] = await api.downloadFile(
        contact: c.contact.fingerprint,
        index: item.index,
      );
    } catch (e) {
      fileErrors[messageId] = e.toString();
    }
    await _loadMessages(c);
  }

  Future<GroupInfo> createGroup(String name, List<String> fingerprints) async {
    final g = await api.createGroup(name: name, members: fingerprints);
    await refreshAll();
    return g;
  }

  Future<void> respond(ContactRequest r, bool accept) async {
    await api.respondRequest(id: r.id, accept: accept);
    await refreshAll();
  }

  Future<void> setGameMode(bool on) async {
    await api.setGameMode(on_: on);
    gameMode = on;
    await refreshAll();
  }

  Future<void> setContribution(bool on) async {
    await api.setContribution(on_: on);
    contributing = on;
    await refreshAll();
  }

  /// Para o nó e a assinatura de eventos (usado ao sair do app).
  Future<void> stop() async {
    await _events?.cancel();
    _timer?.cancel();
    await api.stopNode();
  }

  @override
  void dispose() {
    _events?.cancel();
    _timer?.cancel();
    api.stopNode();
    super.dispose();
  }
}

/// Sinais pelo núcleo: enviados com `send_call_signal`, recebidos pelo stream de eventos.
class _CoreSignals implements SignalTransport {
  _CoreSignals(this.incoming);
  @override
  final Stream<IncomingSignal> incoming;

  @override
  Future<void> send(String peer, Map<String, dynamic> msg) =>
      api.sendCallSignal(contact: peer, payload: encodeSignal(msg));
}
