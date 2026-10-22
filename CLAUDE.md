# CLAUDE.md

Enxame: comunicador P2P friend-to-friend em Rust (iroh). Especificação completa e fases em
[`enxame-mvp.md`](enxame-mvp.md); leia a seção 0 antes de mexer em qualquer coisa.

## Princípio de produto

- **Setup simples para o usuário básico** (spec 1.1, ADR 0008): sem terminal, Docker, variáveis de
  ambiente ou relay para configurar. Relay padrão = públicos do iroh; relay/nó próprio é opt-in.
  Não empurre configuração técnica para o caminho do usuário comum; o DNS com fallback automático
  já existe (ADR 0010); falta o resto do setup simples da Fase 6.

## Regras de trabalho

- Uma fase por vez (seção 13 da spec). Ao fim de cada fase: fmt, clippy e test verdes, critérios
  de aceite descritos, e **parar para revisão**.
- Decisão relevante não coberta pela spec vira ADR em `docs/adr/NNNN-titulo.md`.
- Antes de usar `iroh`, `iroh-blobs`, `flutter_rust_bridge` ou crate de rede, consulte a doc da
  versão estável atual; não assuma APIs de memória. Versões fixadas em `Cargo.toml` (ADR 0002).
- Criptografia: só composição de crates auditadas da seção 5. Nunca implementar primitivas.
- Protocolo versionado desde o primeiro byte. Todo tipo de wire fica em `proto`, com teste de
  round-trip e entrada em `docs/protocol.md`.
- Sem `unwrap()`/`expect()` fora de testes e `main` (lints do workspace avisam).
- Logs sem conteúdo de mensagem nem chaves; ids truncados.
- Commits: Conventional Commits (`feat(core): ...`).
- O core deve compilar para `aarch64-linux-android` (guarda-corpo mobile, job de CI na Fase 3):
  nada de dependência exclusiva de desktop; código específico de SO atrás de traits.

## Comandos

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p enxame-cli -- --help
```

## Estrutura

- `crates/enxame-core` — toda a lógica; API pública: `Enxame` + stream de `Event`.
- `crates/enxame-cli` — binário de desenvolvimento/teste.
- `crates/enxame-node` — daemon headless (papel fixo SuperPeer).
- `crates/enxame-ffi` — fronteira para o Flutter (Fase 6).
- `app/` (Flutter, Fase 6), `deploy/` (Docker), `scripts/netsim/` (NAT), `docs/` (ADRs, protocolo).

## Fase 1 (identidade e conectividade)

- CLI: `init`, `id`, `ticket`, `add`, `serve`, `contacts`, `diag`. Sem relay, o ticket só vale
  enquanto o processo que o gerou roda: use o ticket impresso por `serve`.
- Config: `<data_dir>/config.toml` + `ENXAME_*` (`DATA_DIR`, `RELAY`, `RELAY_CA_FILE`, `MDNS`,
  `DNS_DISCOVERY`, `DISPLAY_NAME`). Detalhes de wire em `docs/protocol.md`; decisões no ADR 0004.
- Nomes do iroh 1.x: `EndpointId`/`EndpointAddr` (a spec ainda diz NodeId/NodeAddr).
- Testes de rede de integração: `cargo test -p enxame-core --test contacts` (sem internet).
- NAT simulado: `scripts/netsim/netsim.sh` (ver README ali); resultados em `docs/reports/`.

## Fase 2 (mensagens 1:1)

- CLI: `send <contato> <texto>`, `history <contato> [-n N]`, `chat <contato>` (interativo e online).
  `send`/`chat`/`serve` usam a identidade do diretório: não rode dois ao mesmo tempo no mesmo
  `--data-dir`.
- Dados em `<data_dir>/enxame.db` (SQLite, WAL). Wire e formato em `docs/protocol.md`; decisões no
  ADR 0005. O texto das mensagens fica em claro no banco local.
- Testes: unitários (cripto, HLC, ordem, store, sync sobre `tokio::io::duplex` com `proptest`) e
  `cargo test -p enxame-core --test messages` (nós in-process).
- Config nova: `sync_interval_secs` (0 desliga a sincronização automática), `dial_timeout_secs`.

## Fase 3 (papéis, carry, nó headless)

- Módulos: `roles` (sinais, `target_role`, `RoleEngine` com histerese, estimativa passiva de
  upload, `RateLimiter`, `CarryLimits`), `carry` (sessão `enxame/carry/1`, recibos), `node/runtime`
  (laço de papéis) e `node/carry_flow` (discar, escolher carriers, oferecer). Decisões no ADR 0006;
  wire em `docs/protocol.md`.
- Um nó novo nasce em `Leaf` e só sobe depois de estável (120 s; 600 s para `SuperPeer`). `Leaf`
  não carrega. Em testes use `role = "peer"` (papel fixo) ou `Enxame::tick_roles_at(Duration)` com
  `StaticSignals` para simular o relógio.
- Config nova: `role`, `contribute`, `game_mode`, `budget.*`, `promote_after_*`, `role_interval_secs`,
  `allowed_fingerprints`, `auto_accept_contacts`, `dns_servers` (e `ENXAME_*` correspondentes; `VAR=` vazio vale
  como ausente). CLI: `--role`, `--game-mode`, `--no-contribute`; no `chat`: `/papel`, `/jogo`,
  `/contribuir`, `/contribuicao`.
- `enxame-node`: `run` (padrão) e `health`; papel fixo `SuperPeer`; chave em arquivo no volume;
  ticket em `<data>/node-ticket.txt`; saúde em `<data>/health`. Deploy em `deploy/`.
- Guarda-corpo Android: `cargo check -p enxame-core --target aarch64-linux-android` (precisa do NDK:
  veja o job `android` do CI para as variáveis `CC_`/`AR_`/linker).
- Testes: `cargo test -p enxame-core --test carry` (cenário A, B, C, nó headless como C, quotas,
  histerese com relógio simulado, budget, política de contatos).

## Fase 4 (arquivos)

- Módulos: `filecrypt` (STREAM sobre XChaCha20-Poly1305), `files` (`FileRef`, nomes seguros),
  `blobs` (segmentos de 8 MiB + `HashSeq`, importar/baixar/exportar, `RoundRobin`) e `node/files.rs`
  (serviço de blobs, política de servir, envio, download, pré-replicação). Decisões no ADR 0007.
- CLI: `send-file <caminho> <contatos...>`, `files <contato>`, `get <contato> <n|id> [--out]`; no
  `chat`: `/enviar`, `/arquivos`, `/baixar`.
- API: `send_file`/`send_file_to`, `files`, `download_file`, `prefetched_blobs`, `file_bytes_sent`.
  Para semear o que baixou, o nó precisa de papel `>= LeafPlus` (em testes: `role = "peer"`).
- Config: `max_file_mb`, `prefetch_max_mb`, `download_dir`, `blob_gc_interval_secs`.
- O `iroh-blobs` 0.103.1 é pré-produção: mantenha o uso isolado e a versão fixada.
- Testes: `cargo test -p enxame-core --test files` (arquivos entre nós Enxame) e os testes de
  `blobs` (3 nós iroh). O teste de 1 GB é `#[ignore]`: `cargo test -p enxame-core --test files
  -- --ignored`.

## Fase 5 (grupos)

- Módulos: `groups` (retrato assinado `GroupSnapshot`, validação, pares por grupo) e
  `node/groups.rs` (criar, add/remover/renomear, enviar, sincronizar, histórico). Decisões no
  ADR 0009; wire em `docs/protocol.md`.
- Só o criador emite retratos (estado completo, `seq` crescente); viajam por mensagem 1:1 `GroupOp`.
  Mensagens do grupo: `conversation_id = group_id`, chave de conteúdo selada para cada membro.
  Co-membros que não são contatos viram **pares de rede** (`Inner::peers`) para sincronização e carry.
- CLI (no `chat`): `/grupos`, `/grupo criar|add|remover|nome|sync|historico`, `#grupo texto`.
  Eventos `GroupJoined` (com a lista de expostos ao IP), `GroupUpdated`, `GroupRemoved`.
- Banco v4 (`groups`) e v5 (`pending_delivery`/`carry_queue` por `(envelope, destinatário)`).
- Testes: `cargo test -p enxame-core --test groups`.

## Fase 6 (app Flutter; em andamento)

- `app/` (Flutter 3.47, Linux; Windows no bloco 6.6) e `crates/enxame-ffi` (ponte `flutter_rust_bridge`
  **2.13.0**, versão fixada; ADR 0011). Regenerar a ponte: `scripts/app/gen-bridge.sh`. O código gerado
  é versionado. A API escrita à mão está em `crates/enxame-ffi/src/api/app.rs`.
- Rodar: `cd app && flutter run -d linux` (o CMake compila a biblioteca Rust). Testes:
  `flutter analyze`, `flutter test` e, com núcleo real, `ENXAME_DATA_DIR=$(mktemp -d) ENXAME_RELAY=disabled
  ENXAME_MDNS=0 flutter test integration_test -d linux`.
- Chamadas 1:1 (áudio/vídeo/tela): sinalização `enxame/call/1` no núcleo (`calls`, `node/calls.rs`) e
  `CallController` no app (`app/lib/src/calls.dart`, `flutter_webrtc`); ADR 0012. Testes:
  `flutter test integration_test/call_test.dart -d linux` (captura a tela de verdade).
- DNS automático no núcleo (ADR 0010). Textos em `app/lib/l10n/*.arb` (pt modelo, en).
- **Não use automação de teclado/mouse (xdotool) na tela do usuário** para testar a interface: use
  `integration_test`.

## Estado

- **Leia [`docs/STATUS.md`](docs/STATUS.md) primeiro**: onde paramos, o que falta e a ordem de amanhã.
- Fases 0 a 5 implementadas (a 5 foi validada em campo: `docs/reports/fase5-grupos.md`); a 3 foi validada em campo com três máquinas (`docs/reports/fase3-carry.md`);
  aguarda revisão. Fases 0–2 revisadas. A Fase 1 foi validada em campo (Linux ↔ Linux,
  redes diferentes); Windows e 4G seguem sem teste. A Fase 2 foi validada em campo com duas
  máquinas remotas (`docs/reports/fase2-mensagens.md`); falta Windows e 4G. Próxima: Fase 6. A Fase 4 passou nos testes locais (inclui 1 GiB em 2 fontes) e foi validada em campo
  (multi-fonte, retomada, remetente offline; Linux e Windows no 4G; `docs/reports/fase4-arquivos.md`).
- Nome do projeto e `repository` no `Cargo.toml` são provisórios.
