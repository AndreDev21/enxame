# CLAUDE.md

Enxame: comunicador P2P friend-to-friend em Rust (iroh). Especificação completa e fases em
[`enxame-mvp.md`](enxame-mvp.md); leia a seção 0 antes de mexer em qualquer coisa.

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

## Estado

- Fases 0 e 1 implementadas, aguardando revisão. Pendente da Fase 1: teste real entre duas
  máquinas (roteiro em `docs/reports/fase1-conectividade.md`). Próxima: Fase 2 (mensagens 1:1).
- Nome do projeto e `repository` no `Cargo.toml` são provisórios.
