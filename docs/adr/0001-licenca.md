# 0001 — Licença

Status: aceito (provisório até a publicação do repositório)

## Contexto

O Enxame é um comunicador P2P cujo valor depende de a rede permanecer aberta. A spec
(seção 16.2) recomenda AGPL-3.0 para o projeto inteiro e cita como alternativa MIT/Apache-2.0
dual apenas no `enxame-core`, para maximizar adoção como biblioteca.

## Decisão

Adotar **AGPL-3.0-only** em todo o workspace. Impede que um fork fechado seja oferecido como
serviço de rede, como no caso de Signal e Element. O texto integral está em `LICENSE`.
(raiz do repositório).

## Consequências

- Dependências precisam ser compatíveis com AGPL; `cargo-deny` aplica a allowlist em `deny.toml`.
- Integradores que queiram usar o core em produto fechado não podem. Se a adoção como
  biblioteca virar objetivo, reabrir esta decisão e relicenciar `enxame-core` (exige
  concordância de todos os contribuidores até lá).
- O nome do projeto (`enxame`) continua provisório (spec 16.1) e o campo `repository` do
  `Cargo.toml` tem placeholder.
