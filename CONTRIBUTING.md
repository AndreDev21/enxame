# Contribuindo

Obrigado por querer ajudar! O projeto avança **por fases** (seção 13 de
[`enxame-mvp.md`](enxame-mvp.md)); antes de abrir um PR grande, abra uma issue descrevendo a ideia
e a fase a que ela pertence. Coisas fora do escopo do MVP (seção 2) ficam para depois.

## Ambiente

- Rust 1.91+ (`rust-toolchain.toml` usa o `stable`).
- Linux: `libdbus-1-dev` e `pkg-config` (keystore do SO). Opcional: `cargo-deny`, Docker, o NDK do
  Android (guarda-corpo mobile) e `nft`/`ip`/`openssl` (netsim).

## Antes de enviar

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check                       # licenças e advisories
```

O CI roda isso, mais `cargo check -p enxame-core --target aarch64-linux-android` e a validação do
`docker compose` de `deploy/`.

## Convenções

- **Commits**: Conventional Commits (`feat(core): ...`, `fix(carry): ...`, `docs: ...`).
- **Sem `unwrap()`/`expect()`** fora de testes e `main`. Logs sem conteúdo de mensagem nem chaves,
  com ids truncados.
- **Cripto**: só composição de crates auditadas (ver `docs/adr/0002-stack.md` e `0005`). Nada de
  primitivas próprias.
- **Protocolo versionado desde o primeiro byte**. Todo tipo de wire vive em `proto` com teste de
  round-trip e entrada em `docs/protocol.md`.
- **O core compila para Android** (`aarch64-linux-android`): nada de dependência só de desktop;
  código específico de SO fica atrás de traits (`PlatformSignals`, keystore).
- **Decisão relevante não coberta pela spec** vira um ADR em `docs/adr/NNNN-titulo.md`
  (contexto, decisão, consequências).
- **APIs de rede mudam**: confira a documentação da versão fixada (`iroh`, etc.) antes de usar.
- Testes ao lado do código (`#[cfg(test)]`) para unidades; integração em `crates/*/tests/`.
  Teste de propriedade (`proptest`) para sincronização e ordenação. Nenhum teste depende de relay
  público da internet.

## Segurança

Não abra issue pública para vulnerabilidades: use o canal de [`SECURITY.md`](SECURITY.md).
