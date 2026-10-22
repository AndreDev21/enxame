import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../desktop.dart';
import '../rust/api/app.dart' as api;
import '../rust/api/app.dart';

/// Comportamento do app e, separado, as opções avançadas (servidor próprio, DNS manual).
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, required this.shell});
  final DesktopShell? shell;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final _relay = TextEditingController();
  final _ca = TextEditingController();
  final _dns = TextEditingController();
  final _downloads = TextEditingController();
  bool _mdns = true;
  bool _autostart = false;
  bool _loaded = false;
  String? _message;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final s = await api.getSettings();
    final auto = await widget.shell?.autostartEnabled ?? false;
    if (!mounted) return;
    setState(() {
      _relay.text = s.relay;
      _ca.text = s.relayCaFile ?? '';
      _dns.text = s.dnsServers.join(', ');
      _downloads.text = s.downloadDir ?? '';
      _mdns = s.mdns;
      _autostart = auto;
      _loaded = true;
    });
  }

  Future<void> _save() async {
    final l = L.of(context);
    try {
      await api.saveSettings(
        s: Settings(
          relay: _relay.text.trim().isEmpty ? 'default' : _relay.text.trim(),
          relayCaFile: _ca.text.trim().isEmpty ? null : _ca.text.trim(),
          dnsServers: _dns.text
              .split(RegExp(r'[,\s]+'))
              .where((e) => e.isNotEmpty)
              .toList(),
          downloadDir: _downloads.text.trim().isEmpty
              ? null
              : _downloads.text.trim(),
          mdns: _mdns,
        ),
      );
      setState(() => _message = l.settingsSaved);
    } catch (e) {
      setState(() => _message = l.errorGeneric('$e'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    final shell = widget.shell;
    if (!_loaded) return const Center(child: CircularProgressIndicator());
    Widget field(TextEditingController c, String label) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: TextField(
        controller: c,
        decoration: InputDecoration(
          labelText: label,
          border: const OutlineInputBorder(),
        ),
      ),
    );
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text(l.settingsTitle, style: t.textTheme.headlineSmall),
        const SizedBox(height: 16),
        Text(l.settingsBehavior, style: t.textTheme.titleMedium),
        if (shell != null)
          Card(
            child: Column(
              children: [
                SwitchListTile(
                  title: Text(l.closeToTray),
                  subtitle: Text(
                    shell.hasTray ? l.closeToTrayHelp : l.trayUnavailable,
                  ),
                  value: shell.prefs.closeToTray,
                  onChanged: (v) async {
                    shell.prefs.closeToTray = v;
                    await shell.prefs.save();
                    setState(() {});
                  },
                ),
                SwitchListTile(
                  title: Text(l.autostart),
                  subtitle: Text(l.autostartHelp),
                  value: _autostart,
                  onChanged: (v) async {
                    await shell.setAutostart(v);
                    setState(() => _autostart = v);
                  },
                ),
              ],
            ),
          ),
        const SizedBox(height: 24),
        ExpansionTile(
          title: Text(l.advancedTitle, style: t.textTheme.titleMedium),
          childrenPadding: const EdgeInsets.all(8),
          expandedCrossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.advancedHelp),
            const SizedBox(height: 12),
            field(_relay, l.relayField),
            field(_ca, l.relayCaField),
            field(_dns, l.dnsField),
            field(_downloads, l.downloadDirField),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(l.mdnsField),
              value: _mdns,
              onChanged: (v) => setState(() => _mdns = v),
            ),
            const SizedBox(height: 8),
            FilledButton(onPressed: _save, child: Text(l.saveSettings)),
            if (_message != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(_message!),
              ),
          ],
        ),
      ],
    );
  }
}
