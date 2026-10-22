import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../state.dart';

String _size(BigInt bytes) {
  final b = bytes.toDouble();
  if (b < 1024) return '${b.toInt()} B';
  if (b < 1024 * 1024) return '${(b / 1024).toStringAsFixed(1)} KB';
  if (b < 1024 * 1024 * 1024) {
    return '${(b / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
  return '${(b / (1024 * 1024 * 1024)).toStringAsFixed(2)} GB';
}

/// Painel de contribuição: papel atual, quanto foi servido, o que está guardado e os botões
/// de modo jogo e de contribuição.
class ContributionScreen extends StatelessWidget {
  const ContributionScreen({super.key, required this.state});
  final AppState state;

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    final c = state.contribution;
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text(l.contributionTitle, style: t.textTheme.headlineSmall),
        const SizedBox(height: 8),
        Text(l.contributionHelp),
        const SizedBox(height: 16),
        Card(
          child: Column(
            children: [
              SwitchListTile(
                title: Text(l.contribute),
                subtitle: Text(l.contributeHelp),
                value: state.contributing,
                onChanged: state.setContribution,
              ),
              SwitchListTile(
                title: Text(l.gameMode),
                subtitle: Text(l.gameModeHelp),
                value: state.gameMode,
                onChanged: state.setGameMode,
              ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        if (c != null) ...[
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _row(context, l.roleLabel, c.role),
                  const SizedBox(height: 12),
                  Text(l.servedThisMonth, style: t.textTheme.labelLarge),
                  const SizedBox(height: 4),
                  LinearProgressIndicator(
                    value: c.budgetBytes == BigInt.zero
                        ? 0
                        : (c.bytesServed.toDouble() / c.budgetBytes.toDouble())
                              .clamp(0.0, 1.0),
                  ),
                  const SizedBox(height: 4),
                  Text('${_size(c.bytesServed)} / ${_size(c.budgetBytes)}'),
                  const Divider(height: 28),
                  _row(
                    context,
                    l.carrying,
                    '${l.messagesUnit(c.carryItems.toInt())} · ${_size(c.carryBytes)}',
                  ),
                  _row(context, l.itemsDelivered, '${c.itemsDelivered}'),
                ],
              ),
            ),
          ),
          const SizedBox(height: 8),
          Text(l.byContact, style: t.textTheme.titleMedium),
          if (c.byContact.isEmpty)
            Padding(padding: const EdgeInsets.all(8), child: Text(l.nobodyYet))
          else
            for (final x in c.byContact)
              ListTile(
                dense: true,
                leading: const Icon(Icons.favorite_outline),
                title: Text(x.name),
                trailing: Text(_size(x.bytes)),
              ),
        ],
      ],
    );
  }

  Widget _row(BuildContext context, String k, String v) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 2),
    child: Row(
      children: [
        Expanded(child: Text(k)),
        Text(v, style: Theme.of(context).textTheme.titleSmall),
      ],
    ),
  );
}
