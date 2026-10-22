# Status do projeto — onde paramos

Atualizado em 2026-10-22 (fim da sessão). **Leia este arquivo primeiro ao retomar.**
Especificação: [`enxame-mvp.md`](../enxame-mvp.md) · convenções: [`CLAUDE.md`](../CLAUDE.md).

## Resumo em uma frase

As Fases 0–4 estão **implementadas e passam em todos os testes locais**; as Fases 1, 2 e 3 também
foram **validadas em campo** com máquinas reais. **A Fase 4 (arquivos) também foi validada em
campo** (2026-10-22: multi-fonte, retomada e arquivo com remetente offline; veja o relatório).
A **Fase 5 (grupos)** está implementada e passa nos testes locais; falta o teste de campo e a revisão. Próxima fase de código: **Fase 6**.

## Fases

| Fase | Entrega | Testes locais | Campo (máquinas reais) | Relatório |
|---|---|---|---|---|
| 0 | Workspace, CI, ADRs | ok | — | — |
| 1 | Identidade, tickets, conectividade, `diag`, netsim | ok | ok (Linux↔Linux, redes diferentes, 5/5 diretas) | `reports/fase1-conectividade.md` |
| 2 | `store`, `crypto`, `sync`, mensagens 1:1 | ok | ok (offline e retorno, ordem, acentos/emoji) | `reports/fase2-mensagens.md` |
| 3 | Papéis, carry com recibos, `enxame-node`, deploy, Android | ok | ok (A→carrier→B com A offline) | `reports/fase3-carry.md` |
| 4 | Arquivos: STREAM, `iroh-blobs`, multi-fonte, retomada, prefetch | ok (inclui 1 GiB em 2 fontes) | ok (multi-fonte, retomada, remetente offline; Windows+4G) | `reports/fase4-arquivos.md` |
| 5 | Grupos pequenos (≤ 20), retrato assinado pelo criador, co-membros como pares, carry entre membros | ok (5 nós in-process, add/remove, removido não lê, carry entre membros) | **PENDENTE** (`roteiros/fase5-grupos.md`) | `reports/fase5-grupos.md` |
| 6 | App Flutter desktop + setup simples | não iniciada | — | — |
| 7 | Call 1:1 (stretch) | não iniciada | — | — |

Último commit de código: `ea59a9d` (correções de campo da Fase 4).

## Amanhã, nesta ordem

1. ~~Teste de campo da Fase 4~~ **feito** (2026-10-22, com bob, notebook Windows no 4G e jean): ver
   `reports/fase4-arquivos.md`. Pendências menores: progresso da retomada começa em 0%, o `/enviar`
   do chat não informa carriers, e a andrecasa (outra cidade) ainda não rodou a Fase 4.
2. **Reconstruir e validar a imagem Docker do `enxame-node` com suporte a arquivos**
   (`deploy/`; a última validação foi na Fase 3, antes de `iroh-blobs`). Idem `docker compose`.
3. **Fase 5 — grupos pequenos (≤ 20)**, seção 13 da spec: `GroupOp` (só o criador adiciona/remove),
   fan-out por membro, membros como carriers entre si, aviso de IP ao entrar. Aceite: grupo de 5
   nós in-process, add/remove, removido não decifra mensagens novas, carry entre membros. Atenção
   aos arquivos em grupo (download de qualquer membro que tenha o blob) e ao `Heads`/`digest` por
   conversa de grupo (a sessão já aceita várias conversas em comum).
4. Depois: Fase 6 (app Flutter) — leia o princípio abaixo.

## Princípio de produto (decidido nesta sessão)

**Setup simples para o usuário básico**: instalar e usar, sem terminal, Docker, variáveis de ambiente
ou relay para configurar. Relay padrão = os públicos do iroh; relay/nó próprio (Docker) é **opt-in**
para quem quer garantia. Está na spec (1.1, 11, 13/Fase 6, 16.8) e no
[ADR 0008](adr/0008-simplicidade-de-setup-e-relay.md). **DNS com fallback automático** é item de
aceite da Fase 6 (hoje só existe a opção manual `dns_servers`).

## Pendências e dívidas conhecidas

| Item | Onde entra |
|---|---|
| DNS com fallback automático (roteador e MagicDNS do Tailscale descartam EDNS) | Fase 6 (aceite) |
| Instaladores e binários prontos pela CI (hoje é preciso compilar com Rust) | Fase 6 / CI de releases |
| Windows e 4G (CGNAT) testados só na Fase 4 (notebook via relay, ~1 MB/s); sinais do Windows (bateria, rede medida) e Ctrl-C sem console não implementados | Fase 6 |
| `enxame-node` em Docker com arquivos | amanhã (item 2) |
| Endereço de contato vem do ticket e envelhece sem relay/DNS (só o lado que reiniciou consegue discar) | resolvido na prática com relay + DNS |
| `RoleUpdate` só alcança contatos com sessão aberta (sessões são curtas); os demais leem o papel no `Hello` | aceito (ADR 0006) |
| `iroh-blobs` 0.103.1 é pré-produção (aviso da própria doc): uso isolado em `blobs.rs`/`node/files.rs` | revisar a cada atualização |
| Avisos do `cargo-deny` ignorados com justificativa: `RUSTSEC-2024-0436`, `-2024-0370`, `-2023-0089` | `deny.toml` |
| Nome do projeto e `repository` provisórios; texto da licença AGPL-3.0 já está em `LICENSE` | spec 16 |

## Como validar tudo (rápido)

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                    # ~133 testes; a suíte leva alguns minutos
cargo test --release -p enxame-core --test files -- --ignored --nocapture   # 1 GiB, 2 fontes
cargo deny check                          # cargo install cargo-deny --locked
scripts/e2e/carry-with-node.sh            # A → enxame-node → B com binários reais
```

Guarda-corpo Android (precisa do NDK; aqui está em `~/Android/Sdk/ndk/28.2.13676358`):

```bash
NDK=~/Android/Sdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/linux-x86_64/bin
CC_aarch64_linux_android=$NDK/aarch64-linux-android24-clang AR_aarch64_linux_android=$NDK/llvm-ar \
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK/aarch64-linux-android24-clang \
cargo check -p enxame-core --target aarch64-linux-android
```

Netsim (NAT simulado, sem root): precisa do `iroh-relay`
(`cargo install iroh-relay --version '=1.4.1' --features server --locked`);
`scripts/netsim/netsim.sh cone symmetric` (ver o README ali).

## Armadilhas já encontradas

- **Um processo por identidade**: `chat`, `serve` e `send` no mesmo diretório de dados conflitam.
- **Não abrir o banco com um binário mais antigo** depois de migrar (o schema só avança).
- **DNS**: esta máquina (Tailscale/MagicDNS) e o roteador do bob descartam EDNS → o relay fica
  inalcançável. Use `ENXAME_DNS_SERVERS=1.1.1.1,8.8.8.8`.
- Nó novo nasce em `Leaf`, que não carrega mensagens nem semeia: em testes use `--role peer`
  (ou `role = "peer"` na configuração).
- Mandar o mesmo arquivo a duas pessoas em comandos separados gera blobs diferentes: use
  `send-file <arquivo> bob andrecasa` / `/enviar <arquivo> bob andrecasa`.
- Zsh não separa variáveis sem aspas em argumentos (`$VAR` com espaços) e `pkill -f` pode matar o
  próprio shell: use `ps | awk | xargs kill`.
- Nos testes de campo, não commitar IPs públicos nem dados pessoais nos relatórios.

## Estado do ambiente desta máquina (não versionado)

- Nenhum processo `enxame-*` rodando; imagens Docker de teste removidas.
- Dados reais em `~/.local/share/enxame` (identidade "andre"; contatos bob e andrecasa mútuos).
- Ferramentas só na pasta temporária da sessão (se sumirem, reinstale como acima): `cargo-deny`,
  `iroh-relay`.
