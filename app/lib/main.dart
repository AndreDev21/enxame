import 'package:flutter/material.dart';

import 'src/app.dart';
import 'src/native_lib.dart';
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init(externalLibrary: await openEnxameLib());
  runApp(const EnxameApp());
}
