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
7. **Bloco 6.3 (arquivos, bandeja, configurações):**
   - Arquivos: botão de anexar (`file_selector`) e arrastar e soltar (`desktop_drop`) na conversa 1:1; o
     balão mostra nome/tamanho, botão de baixar e onde foi salvo; progresso pelos eventos. **Grupos
     ainda não enviam arquivos** (o núcleo só tem `send_file_to` para contatos): o botão fica
     desligado nos grupos.
   - Bandeja: `tray_manager` 0.8 (API nativa, StatusNotifierItem sobre D-Bus) + `window_manager`
     (`setPreventClose`). Fechar a janela esconde e mantém o nó ativo. **No Linux só se cria a
     bandeja se houver um host (`org.kde.StatusNotifierWatcher` no D-Bus)**; sem ele esconder a
     janela a deixaria sem volta, então fechar encerra o app e a tela de configurações avisa.
   - Menu da bandeja: papel atual, abrir, modo jogo, contribuir, sair.
   - Autostart opcional (`launch_at_startup`; arquivo `.desktop` no Linux), desligado por padrão.
   - Configurações avançadas (relay próprio, CA, DNS manual, pasta de downloads, mDNS) gravadas em
     `config.toml` pelo `save_settings` (preserva as outras chaves; valem na próxima subida). A
     preferência "fechar mantém ativo" fica em `<dados>/ui.json`.
   - Ícone da bandeja: hexágono âmbar gerado (`app/assets/tray_icon.png`), provisório.
   Chamadas ficam no 6.4.

## Consequências

- Mudar a API em `crates/enxame-ffi/src/api/` exige rodar `scripts/app/gen-bridge.sh`.
- O teste `app/integration_test/app_test.dart` sobe o app com o núcleo de verdade, percorre as telas
  e pareia com um `enxame-cli serve` real trocando uma mensagem
  (`flutter test integration_test -d linux`, com `ENXAME_DATA_DIR` temporário).
- `libdbus` continua sendo dependência de execução do keystore do SO (cai para arquivo se não houver
  Secret Service); relevante para o AppImage (6.5).
