# Status do projeto — onde paramos

Atualizado em 2026-10-22 (fim da sessão). **Leia este arquivo primeiro ao retomar.**
Especificação: [`enxame-mvp.md`](../enxame-mvp.md) · convenções: [`CLAUDE.md`](../CLAUDE.md).

## Resumo em uma frase

As Fases 0–4 estão **implementadas e passam em todos os testes locais**; as Fases 1, 2 e 3 também
foram **validadas em campo** com máquinas reais. **A Fase 4 (arquivos) também foi validada em
campo** (2026-10-22: multi-fonte, retomada e arquivo com remetente offline; veja o relatório).
A **Fase 5 (grupos)** está implementada, passa nos testes locais e foi validada em campo (2026-10-22); aguarda revisão. Próxima fase de código: **Fase 6**.

## Fases

| Fase | Entrega | Testes locais | Campo (máquinas reais) | Relatório |
|---|---|---|---|---|
| 0 | Workspace, CI, ADRs | ok | — | — |
| 1 | Identidade, tickets, conectividade, `diag`, netsim | ok | ok (Linux↔Linux, redes diferentes, 5/5 diretas) | `reports/fase1-conectividade.md` |
| 2 | `store`, `crypto`, `sync`, mensagens 1:1 | ok | ok (offline e retorno, ordem, acentos/emoji) | `reports/fase2-mensagens.md` |
| 3 | Papéis, carry com recibos, `enxame-node`, deploy, Android | ok | ok (A→carrier→B com A offline) | `reports/fase3-carry.md` |
| 4 | Arquivos: STREAM, `iroh-blobs`, multi-fonte, retomada, prefetch | ok (inclui 1 GiB em 2 fontes) | ok (multi-fonte, retomada, remetente offline; Windows+4G) | `reports/fase4-arquivos.md` |
| 5 | Grupos pequenos (≤ 20), retrato assinado pelo criador, co-membros como pares, carry entre membros | ok (5 nós in-process, add/remove, removido não lê, carry entre membros) | ok (4 máquinas, inclui Windows no 4G; sem não-contatos) | `reports/fase5-grupos.md` |
| 6 | App Flutter desktop + setup simples | não iniciada | — | — |
| 7 | Call 1:1 (stretch) | não iniciada | — | — |

Último commit de código: `ea59a9d` (correções de campo da Fase 4).

## Próximos passos, nesta ordem

1. ~~Teste de campo da Fase 4~~ **feito** (2026-10-22, com bob, notebook Windows no 4G e jean): ver
   `reports/fase4-arquivos.md`. Pendências menores: progresso da retomada começa em 0%, o `/enviar`
   do chat não informa carriers, e a andrecasa (outra cidade) ainda não rodou a Fase 4.
2. ~~Reconstruir e validar a imagem Docker do `enxame-node`~~ **feito** (2026-10-22): imagem de 121 MB,
   `scripts/e2e/node-image.sh` passa (mensagem + arquivo de 6 MB servidos pelo contêiner com o remetente
   offline), healthcheck `healthy` em ~20 s, uso de ~7 MiB de RAM em repouso, `docker compose config` válido.
   Não testado: relay próprio com Let's Encrypt (precisa de domínio) e grupos via contêiner.
3. ~~Fase 5 — grupos pequenos~~ **feita e validada em campo** (`reports/fase5-grupos.md`); aguarda revisão.
4. **Fase 6 em andamento** (spec 13): blocos
   - 6.1 DNS automático — **feito** (ADR 0010);
   - 6.2 ponte `enxame-ffi` (frb 2.13.0) e app Linux básico: onboarding, contatos (ticket/QR), conversas
     1:1 e de grupo, chat, painel de contribuição, diagnóstico;
   - 6.3 arquivos (arrastar e soltar), bandeja, modo jogo, autostart;
   - **6.4 chamadas 1:1: áudio, vídeo e transmissão de tela** (`flutter_webrtc`, sinalização `CallSignal`
     E2E; captura Wayland/PipeWire e X11; ver spec Fase 6 e decisão 5 sobre TURN);
   - 6.5 empacotamento (AppImage) e binários pela CI;
   - 6.6 Windows (build, instalador, campo).
   Próxima fase depois desta: Fase 7 (chamadas em grupo e TURN, stretch). — leia o princípio abaixo.

## Princípio de produto (decidido nesta sessão)

**Setup simples para o usuário básico**: instalar e usar, sem terminal, Docker, variáveis de ambiente
ou relay para configurar. Relay padrão = os públicos do iroh; relay/nó próprio (Docker) é **opt-in**
para quem quer garantia. Está na spec (1.1, 11, 13/Fase 6, 16.8) e no
[ADR 0008](adr/0008-simplicidade-de-setup-e-relay.md). **DNS com fallback automático**: feito (ADR 0010; detecta o resolvedor do sistema que não resolve o
relay e passa a 1.1.1.1/8.8.8.8; `dns_servers` continua mandando).

## Pendências e dívidas conhecidas

| Item | Onde entra |
|---|---|
| ~~DNS com fallback automático~~ feito (ADR 0010); falta reavaliar ao trocar de rede | Fase 6 |
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
- **DNS**: esta máquina (Tailscale/MagicDNS) e o roteador do bob descartam EDNS. Desde o ADR 0010 o
  nó detecta isso sozinho e usa 1.1.1.1/8.8.8.8 (`diag` mostra "DNS: automático"); `ENXAME_DNS_SERVERS` só é necessário para forçar outros servidores.
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
