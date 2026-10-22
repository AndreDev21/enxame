# ADR 0011 — App Flutter e ponte `flutter_rust_bridge`

Status: aceito (Fase 6, bloco 6.2).

## Decisões

1. **Ponte:** `flutter_rust_bridge` **2.13.0** fixado no crate (`=2.13.0`), no pacote Dart e no codegen
   (`cargo install flutter_rust_bridge_codegen --version '=2.13.0' --locked`). Os tipos da API são
   simples (strings, números, listas, `enum` de eventos); contatos são identificados pela fingerprint
   e grupos pelo id em hexadecimal. O código gerado (`frb_generated.rs` e `app/lib/src/rust/`) é
   versionado; `scripts/app/gen-bridge.sh` o regenera.
2. **`unsafe` só no código gerado.** O workspace proíbe `unsafe` (`forbid`); o crate `enxame-ffi`
   rebaixa para `deny` e o módulo `frb_generated` recebe `#[allow(unsafe_code)]`. A API escrita à mão
   (`src/api/`) não usa `unsafe`.
3. **Biblioteca nativa sem cargokit:** o CMake do Linux (`app/linux/CMakeLists.txt`) roda
   `cargo build --release -p enxame-ffi` e instala o `libenxame_ffi.so` em `lib/`, ao lado do
   executável. Em desenvolvimento a biblioteca também é achada em `target/release` ou em
   `ENXAME_FFI_LIB`. (Windows e empacotamento: blocos 6.5 e 6.6.)
4. **Estado e telas:** `ChangeNotifier` único (`AppState`) alimentado pelo stream de eventos do
   núcleo, sem pacote de gerência de estado. Material 3 com cor-semente âmbar, claro e escuro pelo
   sistema. Primeira execução = um campo (o nome); relay, DNS e pastas são automáticos.
5. **Idiomas:** `intl` com ARB (`app_pt.arb` modelo e `app_en.arb`); o app abre no idioma do sistema.
6. **Pacotes:** `file_selector` (oficial) e `desktop_drop` 0.8.x (a 0.9 exige `cross_file` 0.4,
   incompatível com o resto do ecossistema hoje), `qr_flutter`, `freezed` (exigido pelo codegen para
   o enum de eventos). `file_picker` foi descartado por conflito de versões.
7. **Bandeja do sistema** (`tray_manager`), arrastar e soltar e autostart ficam no bloco 6.3;
   chamadas no 6.4.

## Consequências

- Mudar a API em `crates/enxame-ffi/src/api/` exige rodar `scripts/app/gen-bridge.sh`.
- O teste `app/integration_test/app_test.dart` sobe o app com o núcleo de verdade, percorre as telas
  e pareia com um `enxame-cli serve` real trocando uma mensagem
  (`flutter test integration_test -d linux`, com `ENXAME_DATA_DIR` temporário).
- `libdbus` continua sendo dependência de execução do keystore do SO (cai para arquivo se não houver
  Secret Service); relevante para o AppImage (6.5).
