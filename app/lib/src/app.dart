import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../l10n/app_localizations.dart';
import 'rust/api/app.dart' as api;
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
          if (state.started) return HomeScreen(state: state);
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
              return const Scaffold(body: Center(child: CircularProgressIndicator()));
            },
          );
        },
      ),
    );
  }
}
