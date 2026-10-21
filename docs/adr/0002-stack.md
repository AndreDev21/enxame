# 0002 — Stack e versões fixadas

Status: aceito

## Contexto

A spec (seções 0.5 e 5) exige verificar a versão estável atual de `iroh` e `iroh-blobs` na
documentação oficial antes de usar qualquer crate de rede, e fixar versões.

Verificação feita em 2026-10-21 (crates.io API e docs.rs):

| Crate | Versão fixada | Observações |
|---|---|---|
| `iroh` | `=1.4.1` | Última estável (publicada em 2026-10-21); MSRV 1.91. Série 1.x. |
| `iroh-blobs` | `=0.103.1` | Última não-yanked; depende de `iroh ^1.0.0`. As versões 0.99.0–0.103.0 estão yanked; 0.99.1–0.103.1 foram republicadas em 2026-10-06. |

Ressalvas:

- A página do `iroh-blobs` no docs.rs declara que a crate **ainda não é considerada de
  qualidade de produção** e recomenda a 0.35 para quem precisa disso. A API está em 0.x e
  pode quebrar entre minors. Mitigação: versão exata (`=`), uso isolado atrás do módulo
  `blobs` do core e revisão a cada atualização.
- A discovery mDNS vive numa crate separada (`iroh-mdns-address-lookup`, 0.6.1 na
  crates.io; o `iroh-blobs` 0.103.1 a usa em dev-dependencies na ^0.4). Compatibilidade com
  `iroh 1.4.1` será verificada na Fase 1, quando o `net` for implementado.
- MSRV do workspace: **Rust 1.91** (imposto pelo `iroh`); `rust-toolchain.toml` usa stable.

## Decisão

Seguir a tabela da spec (tokio, iroh, iroh-blobs, serde + postcard, rusqlite, keyring,
sysinfo, RustCrypto/dalek, thiserror/anyhow/tracing, Flutter + flutter_rust_bridge v2).
`iroh` e `iroh-blobs` ficam fixados em `[workspace.dependencies]` já na Fase 0, mas só são
consumidos a partir da Fase 1. Demais versões usam a última estável de 2026-10-21 com
requisito caret. `flutter_rust_bridge` será verificado na Fase 6.

## Consequências

- Atualizar `iroh`/`iroh-blobs` é uma decisão explícita (PR dedicado, relendo o changelog).
- `Cargo.lock` é versionado (workspace com binários).
