// Teste de ponta a ponta do app com o núcleo Rust de verdade (biblioteca nativa carregada):
// onboarding → home → contatos (convite) → novo grupo sem contatos → contribuição → diagnóstico.
// Uso: ENXAME_DATA_DIR=$(mktemp -d) ENXAME_MDNS=0 flutter test integration_test -d linux
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:enxame_app/src/app.dart';
import 'package:enxame_app/src/native_lib.dart';
import 'package:enxame_app/src/rust/frb_generated.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('primeira execução até as telas principais', (tester) async {
    await RustLib.init(externalLibrary: await openEnxameLib());
    await tester.pumpWidget(const EnxameApp());
    await tester.pumpAndSettle();

    // Onboarding: um único campo.
    expect(find.byType(TextField), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'Teste');
    await tester.tap(find.byType(FilledButton));
    // O núcleo sobe de verdade (identidade, banco, endpoint): dá tempo.
    for (var i = 0; i < 100 && find.byType(NavigationRail).evaluate().isEmpty; i++) {
      await tester.pump(const Duration(milliseconds: 200));
    }
    expect(find.byType(NavigationRail), findsOneWidget);

    // Sem contatos: a lista de conversas explica e oferece adicionar.
    expect(find.byIcon(Icons.person_add), findsWidgets);

    // Contatos: meu convite e o campo para colar o de outra pessoa.
    await tester.tap(find.byIcon(Icons.people_outline));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.copy), findsOneWidget);
    expect(find.byIcon(Icons.qr_code_2), findsOneWidget);
    await tester.tap(find.byIcon(Icons.qr_code_2));
    // O convite é gerado pelo núcleo (endereços e relay): pode levar alguns segundos.
    for (var i = 0; i < 100 && find.byType(AlertDialog).evaluate().isEmpty; i++) {
      await tester.pump(const Duration(milliseconds: 200));
    }
    expect(find.byType(AlertDialog), findsOneWidget);
    await tester.tap(find.byType(TextButton).last);
    await tester.pumpAndSettle();

    // Contribuição: papel e os dois interruptores.
    await tester.tap(find.byIcon(Icons.volunteer_activism_outlined));
    await tester.pumpAndSettle();
    expect(find.byType(SwitchListTile), findsNWidgets(2));
    await tester.tap(find.byType(SwitchListTile).last);
    await tester.pumpAndSettle();

    // Diagnóstico: roda de verdade (rede real, sem contatos).
    await tester.tap(find.byIcon(Icons.network_check).first);
    await tester.pumpAndSettle();
    await tester.tap(find.byType(FilledButton));
    for (var i = 0; i < 100 && find.byType(Card).evaluate().isEmpty; i++) {
      await tester.pump(const Duration(milliseconds: 300));
    }
    expect(find.byType(Card), findsWidgets);

    // Pareia com um `enxame-cli serve` de verdade (outra identidade, mesma máquina) e troca
    // uma mensagem: o app adiciona o convite, o CLI aceita sozinho, o app envia e o CLI guarda.
    final cli = Platform.environment['ENXAME_CLI'] ??
        '${Directory.current.path}/../target/release/enxame-cli';
    final peerDir = Directory.systemTemp.createTempSync('enxame-peer');
    final env = {
      'ENXAME_DATA_DIR': peerDir.path,
      'ENXAME_RELAY': 'disabled',
      'ENXAME_MDNS': '0',
    };
    await Process.run(cli, ['init', '--name', 'Beto'], environment: env);
    final serve = await Process.start(cli, ['serve', '--auto-accept'], environment: env);
    addTearDown(() {
      serve.kill();
      peerDir.deleteSync(recursive: true);
    });
    final ticketReady = Completer<String>();
    serve.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((line) {
      if (line.startsWith('enxame1') && !ticketReady.isCompleted) ticketReady.complete(line.trim());
    });
    serve.stderr.drain<void>();
    final ticket = await ticketReady.future.timeout(const Duration(seconds: 60));

    await tester.tap(find.byIcon(Icons.people_outline));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).last, ticket);
    await tester.tap(find.byIcon(Icons.person_add));
    for (var i = 0; i < 150 && find.text('Beto').evaluate().isEmpty; i++) {
      await tester.pump(const Duration(milliseconds: 200));
    }
    expect(find.text('Beto'), findsWidgets, reason: 'o CLI virou contato');

    // Abre a conversa e envia.
    await tester.tap(find.text('Beto').last);
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).last, 'oi do app, vinda do teste');
    await tester.tap(find.byIcon(Icons.send));
    for (var i = 0; i < 50; i++) {
      await tester.pump(const Duration(milliseconds: 200));
    }
    expect(find.text('oi do app, vinda do teste'), findsWidgets);

    // O CLI (lendo o banco, sem subir outro nó) recebeu a mensagem.
    var seen = false;
    for (var i = 0; i < 40 && !seen; i++) {
      final r = await Process.run(cli, ['history', 'Teste'], environment: env);
      seen = '${r.stdout}'.contains('oi do app, vinda do teste');
      if (!seen) await tester.pump(const Duration(milliseconds: 500));
    }
    expect(seen, isTrue, reason: 'a mensagem chegou ao CLI');
  });
}
