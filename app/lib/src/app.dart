import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../l10n/app_localizations.dart';
import 'rust/api/app.dart' as api;
import 'desktop.dart';
import 'prefs.dart';
import 'screens/home.dart';
import 'screens/onboarding.dart';
import 'state.dart';

/// Amarelo-mel: cor-semente do tema (claro e escuro seguem o sistema).
const _seed = Color(0xFFE5A100);

class EnxameApp extends StatefulWidget {
  const EnxameApp({super.key});

  @override
  State<EnxameApp> createState() => _EnxameAppState();
}

class _EnxameAppState extends State<EnxameApp> {
  final AppState state = AppState();
  late Future<void> _boot;
  late final bool _firstRun;
  DesktopShell? _shell;
  bool _shellStarting = false;

  @override
  void initState() {
    super.initState();
    _firstRun = !api.hasIdentity();
    // Quem já tem identidade entra direto; a primeira execução passa pelo onboarding.
    _boot = _firstRun ? Future.value() : state.start();
  }

  @override
  void dispose() {
    state.dispose();
    super.dispose();
  }

  /// Integração com a área de trabalho: só depois que o nó subiu (o menu da bandeja usa o estado).
  Future<void> _startShell(L l) async {
    if (_shell != null || _shellStarting) return;
    _shellStarting = true;
    try {
      final prefs = await UiPrefs.load(api.dataDir());
      final shell = DesktopShell(state, prefs);
      await shell.init(l);
      if (mounted) setState(() => _shell = shell);
    } catch (e) {
      debugPrint('integração com a área de trabalho indisponível: $e');
    }
  }

  ThemeData _theme(Brightness b) => ThemeData(
    useMaterial3: true,
    colorScheme: ColorScheme.fromSeed(seedColor: _seed, brightness: b),
    visualDensity: VisualDensity.adaptivePlatformDensity,
  );

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      onGenerateTitle: (c) => L.of(c).appName,
      theme: _theme(Brightness.light),
      darkTheme: _theme(Brightness.dark),
      localizationsDelegates: const [
        L.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: L.supportedLocales,
      home: ListenableBuilder(
        listenable: state,
        builder: (context, _) {
          if (state.started) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (mounted) _startShell(L.of(context));
            });
            _shell?.updateLocale(L.of(context));
            return HomeScreen(state: state, shell: _shell);
          }
          if (_firstRun) return OnboardingScreen(state: state);
          return FutureBuilder<void>(
            future: _boot,
            builder: (context, snap) {
              if (snap.hasError) {
                return Scaffold(
                  body: Center(
                    child: Text(L.of(context).startFailed('${snap.error}')),
                  ),
                );
              }
              return const Scaffold(
                body: Center(child: CircularProgressIndicator()),
              );
            },
          );
        },
      ),
    );
  }
}
