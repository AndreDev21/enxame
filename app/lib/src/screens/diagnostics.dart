import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../rust/api/app.dart' as api;
import '../rust/api/app.dart';

/// Diagnóstico de rede: DNS, NAT, relay e, por contato, o caminho (direto/relay) e o RTT.
class DiagnosticsScreen extends StatefulWidget {
  const DiagnosticsScreen({super.key});

  @override
  State<DiagnosticsScreen> createState() => _DiagnosticsScreenState();
}

class _DiagnosticsScreenState extends State<DiagnosticsScreen> {
  DiagInfo? _info;
  bool _running = false;
  String? _error;

  Future<void> _run() async {
    setState(() {
      _running = true;
      _error = null;
    });
    try {
      final d = await api.diag();
      if (mounted) setState(() => _info = d);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _running = false);
    }
  }

  String _path(L l, String p) => switch (p) {
        'direto' => l.pathDirect,
        'relay' => l.pathRelay,
        'misto' => l.pathMixed,
        _ => l.pathUnknown,
      };

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    final d = _info;
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text(l.diagnosticsTitle, style: t.textTheme.headlineSmall),
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton.icon(
            onPressed: _running ? null : _run,
            icon: _running
                ? const SizedBox(
                    width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.network_check),
            label: Text(_running ? l.running : l.runDiagnostics),
          ),
        ),
        if (_error != null) ...[
          const SizedBox(height: 8),
          Text(l.errorGeneric(_error!), style: TextStyle(color: t.colorScheme.error)),
        ],
        if (d != null) ...[
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Text('${l.roleLabel}: ${d.role}'),
                Text('${l.dnsLabel}: ${d.dns}'),
                Text('${l.natLabel}: ${d.nat}'),
                if (d.relay != null) Text('${l.relayLabel}: ${d.relay}'),
              ]),
            ),
          ),
          const SizedBox(height: 8),
          if (d.peers.isEmpty)
            Padding(padding: const EdgeInsets.all(8), child: Text(l.noContactsToProbe))
          else
            for (final p in d.peers)
              ListTile(
                leading: Icon(p.error == null ? Icons.check_circle_outline : Icons.error_outline,
                    color: p.error == null ? t.colorScheme.primary : t.colorScheme.error),
                title: Text(p.name),
                subtitle: Text(p.error == null
                    ? '${_path(l, p.path)}${p.rttMs == null ? '' : ' · ${p.rttMs!.toStringAsFixed(0)} ms'}'
                        '${p.peerRole == null ? '' : ' · ${p.peerRole}'}'
                    : '${l.unreachable}: ${p.error}'),
              ),
        ],
      ],
    );
  }
}
