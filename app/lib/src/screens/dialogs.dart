import 'package:flutter/material.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../../l10n/app_localizations.dart';
import '../state.dart';

/// Mostra um aviso do núcleo (entrada/saída de grupo, IP visível).
Future<void> showNoticeDialog(BuildContext context, Notice n) {
  final l = L.of(context);
  final (title, body) = switch (n.kind) {
    'removed' => (l.removedFromGroup(n.title), null),
    _ when n.items.isNotEmpty => (l.joinedGroup(n.title), l.ipWarningBody),
    _ => (l.joinedGroup(n.title), null),
  };
  return showDialog<void>(
    context: context,
    builder: (c) => AlertDialog(
      icon: n.items.isNotEmpty ? const Icon(Icons.warning_amber_rounded) : null,
      title: Text(n.items.isNotEmpty ? l.ipWarningTitle : title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (n.items.isNotEmpty)
            Text(title, style: Theme.of(c).textTheme.titleSmall),
          if (body != null) ...[const SizedBox(height: 8), Text(body)],
          for (final i in n.items)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text('• $i'),
            ),
        ],
      ),
      actions: [
        FilledButton(onPressed: () => Navigator.pop(c), child: Text(l.ok)),
      ],
    ),
  );
}

/// QR do convite.
Future<void> showQrDialog(BuildContext context, String ticket) {
  final l = L.of(context);
  return showDialog<void>(
    context: context,
    builder: (c) => AlertDialog(
      title: Text(l.myInvite),
      content: SizedBox(
        width: 320,
        height: 320,
        child: QrImageView(
          data: ticket,
          backgroundColor: Colors.white,
          errorCorrectionLevel: QrErrorCorrectLevel.L,
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(c), child: Text(l.close)),
      ],
    ),
  );
}
