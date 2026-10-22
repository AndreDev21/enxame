import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../state.dart';

/// Primeira execução: um único campo (o nome). Identidade, relay, DNS e pastas são automáticos.
class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({super.key, required this.state});
  final AppState state;

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  final _name = TextEditingController();
  bool _busy = false;
  String? _error;

  Future<void> _go() async {
    final name = _name.text.trim();
    if (name.isEmpty || _busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.state.start(displayName: name);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final t = Theme.of(context);
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Icon(
                  Icons.hive_outlined,
                  size: 72,
                  color: t.colorScheme.primary,
                ),
                const SizedBox(height: 16),
                Text(
                  l.onboardingTitle,
                  textAlign: TextAlign.center,
                  style: t.textTheme.headlineMedium,
                ),
                const SizedBox(height: 8),
                Text(
                  l.onboardingSubtitle,
                  textAlign: TextAlign.center,
                  style: t.textTheme.bodyLarge,
                ),
                const SizedBox(height: 28),
                TextField(
                  controller: _name,
                  autofocus: true,
                  enabled: !_busy,
                  textInputAction: TextInputAction.done,
                  onSubmitted: (_) => _go(),
                  decoration: InputDecoration(
                    labelText: l.onboardingNameLabel,
                    border: const OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 16),
                FilledButton(
                  onPressed: _busy ? null : _go,
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 12),
                    child: Text(
                      _busy ? l.onboardingStarting : l.onboardingStart,
                    ),
                  ),
                ),
                if (_error != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    l.startFailed(_error!),
                    style: TextStyle(color: t.colorScheme.error),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}
