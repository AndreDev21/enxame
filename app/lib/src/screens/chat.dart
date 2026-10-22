import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../rust/api/app.dart' as api;
import '../rust/api/app.dart';
import '../state.dart';

String _clock(BigInt ms) {
  final d = DateTime.fromMillisecondsSinceEpoch(ms.toInt()).toLocal();
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(d.hour)}:${two(d.minute)}';
}

/// Lista de conversas à esquerda e a conversa aberta à direita.
class ChatsView extends StatelessWidget {
  const ChatsView({super.key, required this.state, required this.onAddContact});
  final AppState state;
  final VoidCallback onAddContact;

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final convs = state.conversations;
    final selected = state.selected;
    return Row(
      children: [
        SizedBox(
          width: 320,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
                child: Row(children: [
                  Expanded(
                      child: Text(l.navChats,
                          style: Theme.of(context).textTheme.titleLarge)),
                  IconButton(
                    tooltip: l.newGroup,
                    icon: const Icon(Icons.group_add_outlined),
                    onPressed: () => _newGroup(context),
                  ),
                ]),
              ),
              Expanded(
                child: convs.isEmpty
                    ? Center(
                        child: Padding(
                          padding: const EdgeInsets.all(24),
                          child: Column(mainAxisSize: MainAxisSize.min, children: [
                            Text(l.noChats, textAlign: TextAlign.center),
                            const SizedBox(height: 12),
                            FilledButton.icon(
                                onPressed: onAddContact,
                                icon: const Icon(Icons.person_add),
                                label: Text(l.addContact)),
                          ]),
                        ),
                      )
                    : ListView.builder(
                        itemCount: convs.length,
                        itemBuilder: (c, i) {
                          final conv = convs[i];
                          final last = state.lastOf(conv);
                          return ListTile(
                            selected: conv.key == state.selectedKey,
                            leading: CircleAvatar(
                              child: conv is GroupConversation
                                  ? const Icon(Icons.groups_2_outlined)
                                  : Text(conv.title.isEmpty ? '?' : conv.title[0].toUpperCase()),
                            ),
                            title: Text(conv.title, maxLines: 1, overflow: TextOverflow.ellipsis),
                            subtitle: last == null
                                ? null
                                : Text(
                                    last.text,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                  ),
                            trailing: last == null ? null : Text(_clock(last.timeMs)),
                            onTap: () => state.select(conv),
                          );
                        },
                      ),
              ),
            ],
          ),
        ),
        const VerticalDivider(width: 1),
        Expanded(
          child: selected == null
              ? Center(child: Text(l.pickChat))
              : ChatPane(key: ValueKey(selected.key), state: state, conversation: selected),
        ),
      ],
    );
  }

  Future<void> _newGroup(BuildContext context) async {
    final l = L.of(context);
    final contacts = state.contacts.where((c) => c.mutual).toList();
    if (contacts.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(l.groupNeedsContacts)));
      return;
    }
    final name = TextEditingController();
    final picked = <String>{};
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => StatefulBuilder(
        builder: (c, setS) => AlertDialog(
          title: Text(l.newGroup),
          content: SizedBox(
            width: 360,
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              TextField(
                controller: name,
                autofocus: true,
                decoration: InputDecoration(labelText: l.groupNameLabel),
                onChanged: (_) => setS(() {}),
              ),
              const SizedBox(height: 12),
              Align(
                  alignment: Alignment.centerLeft,
                  child: Text(l.groupMembersLabel, style: Theme.of(c).textTheme.titleSmall)),
              Flexible(
                child: ListView(shrinkWrap: true, children: [
                  for (final ct in contacts)
                    CheckboxListTile(
                      dense: true,
                      title: Text(ct.name),
                      value: picked.contains(ct.fingerprint),
                      onChanged: (v) => setS(() =>
                          v == true ? picked.add(ct.fingerprint) : picked.remove(ct.fingerprint)),
                    ),
                ]),
              ),
            ]),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(c, false), child: Text(l.cancel)),
            FilledButton(
              onPressed: name.text.trim().isEmpty || picked.isEmpty
                  ? null
                  : () => Navigator.pop(c, true),
              child: Text(l.create),
            ),
          ],
        ),
      ),
    );
    if (ok == true) {
      try {
        final g = await state.createGroup(name.text.trim(), picked.toList());
        await state.select(GroupConversation(g));
      } catch (e) {
        if (context.mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text(l.errorGeneric('$e'))));
        }
      }
    }
  }
}

/// Uma conversa: cabeçalho, mensagens (as mais novas embaixo) e o campo de texto.
class ChatPane extends StatefulWidget {
  const ChatPane({super.key, required this.state, required this.conversation});
  final AppState state;
  final Conversation conversation;

  @override
  State<ChatPane> createState() => _ChatPaneState();
}

class _ChatPaneState extends State<ChatPane> {
  final _input = TextEditingController();
  final _focus = FocusNode();
  bool _sending = false;

  Future<void> _send() async {
    final l = L.of(context);
    final text = _input.text.trim();
    if (text.isEmpty || _sending) return;
    setState(() => _sending = true);
    final error = await widget.state.sendText(widget.conversation, text);
    if (!mounted) return;
    setState(() => _sending = false);
    if (error == null) {
      _input.clear();
    } else {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(l.sendFailed(error))));
    }
    _focus.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    final conv = widget.conversation;
    final msgs = widget.state.messagesOf(conv);
    final isGroup = conv is GroupConversation;
    final inactive = isGroup && !conv.group.active;
    return Column(
      children: [
        ListTile(
          title: Text(conv.title, style: t.textTheme.titleMedium),
          subtitle: isGroup
              ? Text(conv.group.members.map((m) => m.isMe ? l.you : m.name).join(', '),
                  maxLines: 1, overflow: TextOverflow.ellipsis)
              : Text(conv is DmConversation ? conv.contact.fingerprint : ''),
          trailing: isGroup
              ? IconButton(
                  tooltip: l.groupInfo,
                  icon: const Icon(Icons.info_outline),
                  onPressed: () => _groupInfo(context, conv.group),
                )
              : null,
        ),
        const Divider(height: 1),
        Expanded(
          child: msgs.isEmpty
              ? Center(child: Text(l.noMessages))
              : ListView.builder(
                  reverse: true,
                  padding: const EdgeInsets.all(12),
                  itemCount: msgs.length,
                  itemBuilder: (c, i) {
                    final m = msgs[msgs.length - 1 - i];
                    return _Bubble(message: m, showAuthor: isGroup && !m.fromMe);
                  },
                ),
        ),
        if (widget.state.transfers.isNotEmpty)
          LinearProgressIndicator(value: widget.state.transfers.values.last),
        if (inactive)
          Padding(padding: const EdgeInsets.all(16), child: Text(l.notInGroup))
        else
          Padding(
            padding: const EdgeInsets.all(12),
            child: Row(children: [
              Expanded(
                child: TextField(
                  controller: _input,
                  focusNode: _focus,
                  autofocus: true,
                  minLines: 1,
                  maxLines: 5,
                  textInputAction: TextInputAction.send,
                  onSubmitted: (_) => _send(),
                  decoration: InputDecoration(
                    hintText: l.typeMessage,
                    border: const OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              IconButton.filled(
                tooltip: l.send,
                onPressed: _sending ? null : _send,
                icon: const Icon(Icons.send),
              ),
            ]),
          ),
      ],
    );
  }

  Future<void> _groupInfo(BuildContext context, GroupInfo group) async {
    final l = L.of(context);
    await showDialog<void>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(group.name),
        content: SizedBox(
          width: 380,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(l.groupCreator(group.creator)),
            const SizedBox(height: 12),
            for (final m in group.members)
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: CircleAvatar(
                    radius: 14, child: Text(m.name.isEmpty ? '?' : m.name[0].toUpperCase())),
                title: Text(m.isMe ? '${m.name} (${l.you})' : m.name),
                subtitle: m.exposed ? Text(l.seesYourIp) : null,
                trailing: group.iAmCreator && !m.isMe
                    ? IconButton(
                        tooltip: l.removeMember,
                        icon: const Icon(Icons.person_remove_outlined),
                        onPressed: () async {
                          Navigator.pop(c);
                          await api.removeGroupMember(group: group.id, member: m.fingerprint);
                          await widget.state.refreshAll();
                        },
                      )
                    : null,
              ),
          ]),
        ),
        actions: [
          if (group.iAmCreator)
            TextButton(
              onPressed: () async {
                Navigator.pop(c);
                await _addMember(context, group);
              },
              child: Text(l.addMember),
            ),
          TextButton(onPressed: () => Navigator.pop(c), child: Text(l.close)),
        ],
      ),
    );
  }

  Future<void> _addMember(BuildContext context, GroupInfo group) async {
    final l = L.of(context);
    final inGroup = group.members.map((m) => m.fingerprint).toSet();
    final options =
        widget.state.contacts.where((c) => c.mutual && !inGroup.contains(c.fingerprint)).toList();
    if (options.isEmpty) return;
    final pick = await showDialog<ContactInfo>(
      context: context,
      builder: (c) => SimpleDialog(
        title: Text(l.addMember),
        children: [
          for (final o in options)
            SimpleDialogOption(onPressed: () => Navigator.pop(c, o), child: Text(o.name)),
        ],
      ),
    );
    if (pick != null) {
      await api.addGroupMember(group: group.id, contact: pick.fingerprint);
      await widget.state.refreshAll();
    }
  }
}

class _Bubble extends StatelessWidget {
  const _Bubble({required this.message, required this.showAuthor});
  final MessageInfo message;
  final bool showAuthor;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final l = L.of(context);
    final mine = message.fromMe;
    final bg = mine ? t.colorScheme.primaryContainer : t.colorScheme.surfaceContainerHighest;
    final fg = mine ? t.colorScheme.onPrimaryContainer : t.colorScheme.onSurface;
    return Align(
      alignment: mine ? Alignment.centerRight : Alignment.centerLeft,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Card(
          color: bg,
          margin: const EdgeInsets.symmetric(vertical: 3),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                if (showAuthor)
                  Text(message.author,
                      style: t.textTheme.labelMedium?.copyWith(color: t.colorScheme.primary)),
                SelectableText(message.text, style: TextStyle(color: fg)),
                const SizedBox(height: 2),
                Row(mainAxisSize: MainAxisSize.min, children: [
                  Text(_clock(message.timeMs),
                      style: t.textTheme.labelSmall?.copyWith(color: fg.withValues(alpha: 0.7))),
                  if (mine && message.delivered != null) ...[
                    const SizedBox(width: 4),
                    Tooltip(
                      message: message.delivered! ? l.deliveredHint : l.pendingHint,
                      child: Icon(message.delivered! ? Icons.done_all : Icons.schedule,
                          size: 14, color: fg.withValues(alpha: 0.7)),
                    ),
                  ],
                ]),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
