import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../l10n/app_localizations.dart';
import '../rust/api/app.dart' as api;
import '../state.dart';
import 'dialogs.dart';

/// Contatos: meu convite, adicionar por convite, pedidos recebidos e a lista.
class ContactsScreen extends StatefulWidget {
  const ContactsScreen({super.key, required this.state, required this.onOpenChat});
  final AppState state;
  final void Function(Conversation) onOpenChat;

  @override
  State<ContactsScreen> createState() => _ContactsScreenState();
}

class _ContactsScreenState extends State<ContactsScreen> {
  final _ticket = TextEditingController();
  bool _adding = false;
  String? _message;

  Future<String> _loadTicket() => api.myTicket();

  Future<void> _copy() async {
    final l = L.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final t = await _loadTicket();
    await Clipboard.setData(ClipboardData(text: t));
    messenger.showSnackBar(SnackBar(content: Text(l.inviteCopied)));
  }

  Future<void> _qr() async {
    final t = await _loadTicket();
    if (mounted) await showQrDialog(context, t);
  }

  Future<void> _add() async {
    final l = L.of(context);
    final text = _ticket.text.trim();
    if (text.isEmpty || _adding) return;
    setState(() {
      _adding = true;
      _message = l.waitingAcceptance;
    });
    String msg;
    try {
      final r = await api.addContact(ticket: text);
      msg = switch (r) {
        'aceito' => l.contactAdded(_nameFor(text)),
        'recusado' => l.contactRejected,
        _ => l.contactAlready,
      };
      if (r == 'aceito') _ticket.clear();
    } catch (e) {
      msg = l.errorGeneric('$e');
    }
    await widget.state.refreshAll();
    if (mounted) {
      setState(() {
        _adding = false;
        _message = msg;
      });
    }
  }

  /// O nome do contato novo: o mais recente da lista.
  String _nameFor(String _) {
    final c = widget.state.contacts;
    return c.isEmpty ? '' : c.last.name;
  }

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    final s = widget.state;
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text(l.contactsTitle, style: t.textTheme.headlineSmall),
        const SizedBox(height: 16),
        if (s.requests.isNotEmpty) ...[
          Text(l.requestsTitle, style: t.textTheme.titleMedium),
          const SizedBox(height: 8),
          for (final r in s.requests)
            Card(
              color: t.colorScheme.secondaryContainer,
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(l.requestFrom(r.name), style: t.textTheme.titleSmall),
                    const SizedBox(height: 4),
                    Text(l.requestVerify),
                    SelectableText(r.fingerprint,
                        style: t.textTheme.titleMedium?.copyWith(fontFamily: 'monospace')),
                    const SizedBox(height: 8),
                    Row(children: [
                      FilledButton(
                          onPressed: () => s.respond(r, true), child: Text(l.accept)),
                      const SizedBox(width: 8),
                      OutlinedButton(
                          onPressed: () => s.respond(r, false), child: Text(l.reject)),
                    ]),
                  ],
                ),
              ),
            ),
          const SizedBox(height: 16),
        ],
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(l.myInvite, style: t.textTheme.titleMedium),
                const SizedBox(height: 4),
                Text(l.myInviteHelp),
                const SizedBox(height: 12),
                Wrap(spacing: 8, children: [
                  FilledButton.icon(
                      onPressed: _copy,
                      icon: const Icon(Icons.copy),
                      label: Text(l.copyInvite)),
                  OutlinedButton.icon(
                      onPressed: _qr,
                      icon: const Icon(Icons.qr_code_2),
                      label: Text(l.showQr)),
                ]),
              ],
            ),
          ),
        ),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(l.addContact, style: t.textTheme.titleMedium),
                const SizedBox(height: 4),
                Text(l.addContactHelp),
                const SizedBox(height: 12),
                TextField(
                  controller: _ticket,
                  minLines: 2,
                  maxLines: 4,
                  enabled: !_adding,
                  decoration: InputDecoration(
                    labelText: l.inviteLabel,
                    border: const OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                FilledButton.icon(
                  onPressed: _adding ? null : _add,
                  icon: _adding
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.person_add),
                  label: Text(l.sendRequest),
                ),
                if (_message != null) ...[const SizedBox(height: 8), Text(_message!)],
              ],
            ),
          ),
        ),
        const SizedBox(height: 8),
        if (s.contacts.isEmpty)
          Padding(padding: const EdgeInsets.all(16), child: Text(l.noContacts))
        else
          for (final c in s.contacts)
            ListTile(
              leading: CircleAvatar(child: Text(c.name.isEmpty ? '?' : c.name[0].toUpperCase())),
              title: Text(c.name),
              subtitle: Text('${l.fingerprintLabel}: ${c.fingerprint}'),
              trailing: c.mutual ? null : Chip(label: Text(l.contactPending)),
              onTap: c.mutual ? () => widget.onOpenChat(DmConversation(c)) : null,
            ),
      ],
    );
  }
}
