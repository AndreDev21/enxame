// Teste de ponta a ponta do app com o núcleo Rust de verdade (biblioteca nativa carregada):
// onboarding → home → contatos (convite) → novo grupo sem contatos → contribuição → diagnóstico.
// Uso: ENXAME_DATA_DIR=$(mktemp -d) ENXAME_MDNS=0 flutter test integration_test -d linux
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:enxame_app/src/app.dart';
import 'package:enxame_app/src/native_lib.dart';
import 'package:enxame_app/src/rust/api/app.dart' as api;
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
    for (
      var i = 0;
      i < 100 && find.byType(NavigationRail).evaluate().isEmpty;
      i++
    ) {
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
    for (
      var i = 0;
      i < 100 && find.byType(AlertDialog).evaluate().isEmpty;
      i++
    ) {
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
    final cli =
        Platform.environment['ENXAME_CLI'] ??
        '${Directory.current.path}/../target/release/enxame-cli';
    final peerDir = Directory.systemTemp.createTempSync('enxame-peer');
    final env = {
      'ENXAME_DATA_DIR': peerDir.path,
      'ENXAME_RELAY': 'disabled',
      'ENXAME_MDNS': '0',
    };
    await Process.run(cli, ['init', '--name', 'Beto'], environment: env);
    final serve = await Process.start(cli, [
      'serve',
      '--auto-accept',
    ], environment: env);
    addTearDown(() {
      serve.kill();
      peerDir.deleteSync(recursive: true);
    });
    final serveLines = <String>[];
    final ticketReady = Completer<String>();
    serve.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen(
      (line) {
        serveLines.add(line);
        if (line.startsWith('enxame1') && !ticketReady.isCompleted) {
          ticketReady.complete(line.trim());
        }
      },
    );
    serve.stderr.drain<void>();
    final ticket = await ticketReady.future.timeout(
      const Duration(seconds: 60),
    );

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
    await tester.enterText(
      find.byType(TextField).last,
      'oi do app, vinda do teste',
    );
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

    // Chamada: o sinal do app atravessa a ponte e o núcleo (E2E, em tempo real) até o outro nó.
    final peerFp = (await api.contacts())
        .firstWhere((c) => c.name == 'Beto')
        .fingerprint;
    await api.sendCallSignal(
      contact: peerFp,
      payload: '{"t":"invite","id":"teste-1","kind":"screen"}',
    );
    var gotSignal = false;
    for (var i = 0; i < 40 && !gotSignal; i++) {
      gotSignal = serveLines.any(
        (l) => l.contains('Sinal de chamada de Teste') && l.contains('teste-1'),
      );
      if (!gotSignal) await tester.pump(const Duration(milliseconds: 250));
    }
    expect(
      gotSignal,
      isTrue,
      reason: 'o sinal de chamada chegou ao outro nó: $serveLines',
    );

    // Arquivo: o app envia (mesma chamada do botão e do arrastar-e-soltar) e o CLI baixa dele.
    final src = File('${peerDir.path}/origem.bin')
      ..writeAsBytesSync(
        List<int>.generate(3 * 1024 * 1024 + 123, (i) => (i * 31 + 7) & 0xff),
      );
    await api.sendFile(
      contacts: [
        (await api.contacts()).firstWhere((c) => c.name == 'Beto').fingerprint,
      ],
      path: src.path,
    );
    serve.kill(); // um processo por identidade: o `get` abaixo sobe o nó do CLI
    await serve.exitCode;
    final out = Directory('${peerDir.path}/baixados')..createSync();
    ProcessResult? got;
    for (var i = 0; i < 20; i++) {
      got = await Process.run(cli, [
        'get',
        'Teste',
        '1',
        '--out',
        out.path,
      ], environment: env);
      if (got.exitCode == 0) break;
      await tester.pump(const Duration(seconds: 1));
    }
    expect(
      got?.exitCode,
      0,
      reason: 'o CLI baixou o arquivo: ${got?.stderr} ${got?.stdout}',
    );
    final dl = File('${out.path}/origem.bin');
    expect(dl.existsSync(), isTrue);
    expect(
      dl.readAsBytesSync(),
      equals(src.readAsBytesSync()),
      reason: 'conteúdo idêntico',
    );

    // Configurações: abre a tela e salva (relay/DNS manual só valem na próxima subida).
    await tester.tap(find.byIcon(Icons.settings_outlined));
    await tester.pumpAndSettle();
    expect(find.byType(SwitchListTile), findsWidgets);
    await tester.tap(find.byType(ExpansionTile));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(2), '1.1.1.1, 8.8.8.8');
    await tester.tap(find.byType(FilledButton).last);
    await tester.pumpAndSettle();
    final cfg = File('${Platform.environment['ENXAME_DATA_DIR']}/config.toml')
        .readAsStringSync();
    expect(cfg, contains('1.1.1.1'));
  });
}
