import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:enxame_app/l10n/app_localizations.dart';

void main() {
  testWidgets('textos de pt e en cobrem as mesmas chaves', (tester) async {
    for (final locale in L.supportedLocales) {
      late L l;
      await tester.pumpWidget(
        MaterialApp(
          locale: locale,
          localizationsDelegates: const [
            L.delegate,
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          supportedLocales: L.supportedLocales,
          home: Builder(
            builder: (c) {
              l = L.of(c);
              return const SizedBox();
            },
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(l.appName, 'Enxame');
      expect(l.contactAdded('Bia'), contains('Bia'));
      expect(l.messagesUnit(3), contains('3'));
    }
  });
}
