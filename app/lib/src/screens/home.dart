import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../state.dart';
import 'chat.dart';
import 'contacts.dart';
import 'contribution.dart';
import 'diagnostics.dart';
import '../desktop.dart';
import 'dialogs.dart';
import 'settings.dart';

enum _Section { chats, contacts, contribution, diagnostics, settings }

/// Janela principal: trilho de navegação à esquerda e a seção escolhida à direita.
class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, required this.state, this.shell});
  final AppState state;
  final DesktopShell? shell;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  _Section _section = _Section.chats;

  @override
  void initState() {
    super.initState();
    widget.state.addListener(_showNotices);
  }

  @override
  void dispose() {
    widget.state.removeListener(_showNotices);
    super.dispose();
  }

  /// Avisos do núcleo (entrei/saí de um grupo, IP visível) viram diálogo, um de cada vez.
  void _showNotices() {
    final n = widget.state.takeNotice();
    if (n == null || !mounted) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) showNoticeDialog(context, n);
    });
  }

  @override
  Widget build(BuildContext context) {
    final l = L.of(context);
    final s = widget.state;
    final badge = s.requests.length;
    return Scaffold(
      body: Row(
        children: [
          NavigationRail(
            selectedIndex: _section.index,
            labelType: NavigationRailLabelType.all,
            onDestinationSelected: (i) =>
                setState(() => _section = _Section.values[i]),
            leading: Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: Icon(
                Icons.hive_outlined,
                size: 32,
                color: Theme.of(context).colorScheme.primary,
              ),
            ),
            destinations: [
              NavigationRailDestination(
                icon: const Icon(Icons.chat_bubble_outline),
                selectedIcon: const Icon(Icons.chat_bubble),
                label: Text(l.navChats),
              ),
              NavigationRailDestination(
                icon: Badge(
                  isLabelVisible: badge > 0,
                  label: Text('$badge'),
                  child: const Icon(Icons.people_outline),
                ),
                selectedIcon: const Icon(Icons.people),
                label: Text(l.navContacts),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.volunteer_activism_outlined),
                selectedIcon: const Icon(Icons.volunteer_activism),
                label: Text(l.navContribution),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.network_check),
                selectedIcon: const Icon(Icons.network_check),
                label: Text(l.navDiagnostics),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.settings_outlined),
                selectedIcon: const Icon(Icons.settings),
                label: Text(l.navSettings),
              ),
            ],
          ),
          const VerticalDivider(width: 1),
          Expanded(
            child: ListenableBuilder(
              listenable: s,
              builder: (context, _) => switch (_section) {
                _Section.chats => ChatsView(
                  state: s,
                  onAddContact: () =>
                      setState(() => _section = _Section.contacts),
                ),
                _Section.contacts => ContactsScreen(
                  state: s,
                  onOpenChat: (c) {
                    s.select(c);
                    setState(() => _section = _Section.chats);
                  },
                ),
                _Section.contribution => ContributionScreen(state: s),
                _Section.diagnostics => const DiagnosticsScreen(),
                _Section.settings => SettingsScreen(shell: widget.shell),
              },
            ),
          ),
        ],
      ),
    );
  }
}
