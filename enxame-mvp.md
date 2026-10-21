# Enxame — Especificação do MVP

> **Nome provisório:** `enxame` (troque antes de publicar o repositório).
> **Público deste documento:** Claude Code (implementação) e mantenedores (revisão futura).
> **Status:** MVP — escopo fechado abaixo. Qualquer coisa fora da seção "Escopo" é fora do MVP.
> **Plataforma:** **desktop-first** (Linux → Windows; macOS best-effort). Mobile é pós-MVP (seção 17), mas o core é desenhado desde já para não precisar de reescrita quando o mobile chegar.

---

## 0. Como o Claude Code deve usar este documento

1. Leia este arquivo inteiro antes de escrever qualquer código.
2. Gere um `CLAUDE.md` na raiz com: convenções do projeto, comandos de build/test, estrutura do monorepo e um link para este arquivo. Mantenha-o curto e atualize-o ao fim de cada fase.
3. Implemente **uma fase por vez** (seção 13). Ao final de cada fase:
   - `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` e `cargo test --workspace` passando;
   - critérios de aceitação da fase verificados e descritos no PR/commit;
   - **pare e peça revisão** antes de iniciar a próxima fase.
4. Toda decisão relevante não coberta aqui vira um ADR em `docs/adr/NNNN-titulo.md` (contexto, decisão, consequências).
5. **APIs mudam.** Antes de usar `iroh`, `iroh-blobs`, `flutter_rust_bridge` ou qualquer crate de rede, consulte a documentação da versão estável atual (docs.rs / repositório oficial). Não assuma assinaturas de memória. Fixe versões no `Cargo.toml`.
6. **Criptografia:** apenas composição de crates auditadas listadas na seção 5. Nunca implemente primitivas.
7. Protocolo de rede é **versionado desde o primeiro byte**. Nada de formato implícito.

---

## 1. Visão

Comunicador estilo Discord **sem servidor central obrigatório**. Cada usuário instala o app e ele:

- avalia automaticamente rede, energia e processamento do dispositivo;
- assume um **papel** na rede (de consumidor puro a super-peer) conforme o contexto, em tempo real;
- enquanto está em uso, **contribui com a sua rede de contatos**: roteia mensagens, semeia arquivos e guarda mensagens offline cifradas;
- conecta diretamente aos contatos (P2P, hole punching), como o RustDesk, com **fallback de relay mínimo e self-hostável**.

Quanto mais contatos ativos, melhor a entrega offline e a distribuição de arquivos. Os dados ficam nos dispositivos dos usuários, cifrados ponta a ponta.

### 1.1 Princípios de design (não negociáveis)

| Princípio | Implicação |
|---|---|
| Local-first | O app funciona offline; o banco local é a fonte da verdade do usuário. |
| Friend-to-friend | Só contatos aceitos mutuamente trocam tráfego e veem o IP um do outro. |
| Infra mínima, não zero | Rendezvous/relay leve (iroh-relay) é aceito e deve ser self-hostável via Docker. |
| Contribuição por contexto | O papel depende do estado do dispositivo (rede, energia, NAT), não do tipo do dispositivo. |
| Contribuição visível e limitada | Usuário vê e limita quanto contribui (banda, GB/mês). Nada escondido. |
| Bônus, nunca punição | Quem não pode contribuir (4G, bateria) mantém experiência básica completa. |
| E2E sempre | Carriers e relays só veem ciphertext. |
| Mesmo core em todo lugar | CLI, app Flutter e nó headless usam a mesma crate Rust. |
| Desktop-first, mobile-ready | MVP só desktop. O core não pode depender de nada exclusivo de desktop (ver seção 17.1). |

---

## 2. Escopo do MVP

### Dentro

- Identidade baseada em chave (sem conta, sem servidor de login).
- Adição de contato via **ticket** (link/QR), com aceitação mútua.
- Mensagens de texto 1:1 e em **grupos pequenos (≤ 20 membros)**, E2E.
- Persistência local (SQLite) e sincronização ao reconectar.
- **Avaliação de capacidade + papéis automáticos**, com histerese.
- **Store-and-forward** via contatos online (entrega offline).
- Transferência de arquivos cifrados, com **seeding multi-fonte** entre contatos.
- Descoberta em rede local (mDNS) para conexão direta na mesma LAN.
- Clientes: **CLI** (Linux), **nó headless** (Docker), **app Flutter desktop** (Linux primeiro, depois Windows; macOS best-effort).
- App desktop roda na **bandeja do sistema** e continua contribuindo com a rede quando a janela é fechada (opt-out, com limites).
- Painel de contribuição (contador de MB/contatos ajudados) e limites configuráveis.
- Diagnóstico de rede (`diag`): tipo de NAT, caminho direto vs relay, papel atual, score.

### Fora (pós-MVP — não implementar)

- Forward secrecy / MLS para grupos (fase pós-MVP; deixar o envelope preparado).
- Multi-dispositivo por identidade (estrutura preparada, não implementada).
- Comunidades grandes estilo "servidor do Discord", canais, roles, moderação.
- Calls em grupo, SFU, screen share.
- **Mobile (Android e iOS)**: roadmap na seção 17.
- Push notifications.
- Incentivos além do contador (nada de token/cripto, nunca).
- Wi-Fi Direct / Bluetooth mesh.

Call 1:1 é **stretch** (Fase 7) e só entra se as fases anteriores estiverem estáveis.

---

## 3. Arquitetura

```
┌──────────────────────── Dispositivo ────────────────────────┐
│  Flutter UI  ──FFI (flutter_rust_bridge)──┐                 │
│  CLI         ─────────────────────────────┤                 │
│  Nó headless ─────────────────────────────┤                 │
│                                           ▼                 │
│                     ┌──────── enxame-core ────────┐         │
│                     │ identity  crypto  store     │         │
│                     │ proto     sync    carry     │         │
│                     │ roles     blobs   net       │         │
│                     └──────────────┬──────────────┘         │
│                                    │ iroh (QUIC)            │
└────────────────────────────────────┼────────────────────────┘
                                     │
          direto (hole punching) ◄───┴───► fallback via relay
                                     │
                     ┌───────────────┴───────────────┐
                     │ iroh-relay (self-host, Docker) │
                     │ enxame-node (super-peer 24/7)  │  ← opcional, por comunidade
                     └────────────────────────────────┘
```

### 3.1 Componentes do core

| Módulo | Responsabilidade |
|---|---|
| `identity` | Chave de identidade (Ed25519), certificado de dispositivo, tickets de convite. |
| `crypto` | Selagem de envelopes por destinatário, assinatura, cifragem de arquivos. |
| `store` | SQLite: contatos, conversas, envelopes, fila de carry, contabilidade de contribuição. |
| `proto` | Tipos de wire, framing, versionamento, ALPNs. |
| `sync` | Reconciliação de mensagens por conversa entre dois peers. |
| `carry` | Store-and-forward: aceitar, guardar, entregar e expurgar envelopes de terceiros. |
| `roles` | Coleta de sinais, score, papel atual, histerese, budget. |
| `blobs` | Arquivos via `iroh-blobs` (BLAKE3 verificado, multi-fonte, resume). |
| `net` | Endpoint iroh, discovery (DNS/pkarr + mDNS), gerência de conexões, métricas de caminho. |

API pública do core: uma struct `Enxame` com métodos assíncronos e um **stream de eventos** (`MessageReceived`, `ContactOnline`, `RoleChanged`, `TransferProgress`, ...). Clientes não acessam módulos internos.

---

## 4. Estrutura do monorepo

```
enxame/
├── Cargo.toml                 # workspace
├── CLAUDE.md
├── LICENSE
├── crates/
│   ├── enxame-core/           # toda a lógica
│   ├── enxame-cli/            # binário de desenvolvimento/teste
│   ├── enxame-node/           # daemon headless, papel fixo SuperPeer
│   └── enxame-ffi/            # API exposta ao Flutter (flutter_rust_bridge v2)
├── app/                       # Flutter desktop (Linux → Windows); mobile pós-MVP
├── deploy/
│   ├── docker-compose.yml     # iroh-relay + enxame-node
│   └── README.md
├── scripts/
│   └── netsim/                # simulação de NAT com network namespaces
├── docs/
│   ├── adr/
│   ├── protocol.md            # spec de wire, mantida em sincronia com proto
│   └── threat-model.md
└── .github/workflows/ci.yml
```

---

## 5. Stack

| Área | Escolha | Motivo |
|---|---|---|
| Linguagem core | Rust stable, edition 2021+ | Performance, segurança de memória, ecossistema iroh. |
| Async | `tokio` | Padrão do iroh. |
| Rede P2P | `iroh` | QUIC, hole punching, relay fallback, roaming de IP (essencial no mobile). |
| Arquivos | `iroh-blobs` | BLAKE3 verificado em streaming, multi-fonte, resume. |
| Discovery local | feature de discovery em rede local do iroh (mDNS) | Conexão direta na LAN. |
| Relay | `iroh-relay` self-hosted | Mesmo papel do hbbs/hbbr no RustDesk. |
| Serialização | `serde` + `postcard` | Compacto, determinístico o bastante para wire. |
| Persistência | SQLite via `rusqlite` (bundled) em `spawn_blocking` | Simples, portável (inclusive Android, no futuro). |
| Keystore | `keyring` | Secret Service (Linux), Credential Manager (Windows), Keychain (macOS). |
| Sinais do sistema | `sysinfo` + implementações por SO atrás da trait `PlatformSignals` | Permite plugar Android/iOS depois sem mexer no core. |
| Migrations | `rusqlite_migration` ou SQL versionado próprio | Schema evolui por fase. |
| Assinatura | `ed25519-dalek` | — |
| Troca de chaves | `x25519-dalek` | — |
| AEAD | `chacha20poly1305` (XChaCha20-Poly1305) | Nonce de 192 bits, aleatório seguro. |
| Hash/KDF | `blake3`, `hkdf` | — |
| Erros/log | `thiserror` (core), `anyhow` (binários), `tracing` | — |
| UI | Flutter desktop + `flutter_rust_bridge` v2 | Stack já dominado; o mesmo código atende mobile depois. |
| Bandeja | `tray_manager` + `window_manager` (Flutter) | Rodar em segundo plano no desktop. |
| CI | GitHub Actions: fmt, clippy, test, `cargo-deny` | Licenças e advisories. |

---

## 6. Identidade e contatos

### 6.1 Chaves

- **Identity key** (Ed25519): identidade do usuário. Gerada na primeira execução. Guardada no keystore do SO via `keyring` (Linux: Secret Service; Windows: Credential Manager; macOS: Keychain). Fallback: arquivo com permissão restrita, documentado como limitação (comum em Linux sem Secret Service, como WMs puros sem gnome-keyring/kwallet).
- **Encryption key** (X25519): separada, assinada pela identity key.
- **Device key**: a `SecretKey` do iroh (o NodeId). A identity key emite um `DeviceCert { identity_pk, node_id, created_at, sig }`.
  - MVP: um dispositivo por identidade, mas **toda a lógica referencia identidade e dispositivo separadamente**, para habilitar multi-dispositivo sem quebrar o protocolo.

### 6.2 Ticket de convite

```rust
struct InviteTicket {
    version: u8,
    identity_pk: [u8; 32],
    enc_pk: [u8; 32],
    device_cert: DeviceCert,
    node_addr: NodeAddr,      // node_id + relay_url + endereços diretos conhecidos
    display_name: String,
    expires_at: u64,
    sig: [u8; 64],            // assinado pela identity key
}
```

Codificação: `postcard` → base32 com prefixo `enxame1`. Mesmo payload vira QR no app.

Fluxo: A gera o ticket → B importa → B conecta e envia `ContactRequest` → A aceita → ambos marcam `mutual = true`. **Nenhum tráfego de carry/seed antes de `mutual = true`.**

---

## 7. Mensagens

### 7.1 Envelope

```rust
struct Envelope {
    version: u8,
    id: [u8; 32],               // blake3 do envelope sem id/sig
    conversation_id: [u8; 32],
    author: [u8; 32],           // identity_pk
    author_device: NodeId,
    hlc: Hlc,                   // hybrid logical clock (ms físico + contador)
    parents: Vec<[u8; 32]>,     // heads conhecidos na conversa (ordenação causal)
    recipients: Vec<SealedKey>, // chave de conteúdo selada por destinatário
    kind: EnvelopeKind,         // Text | FileRef | Receipt | GroupOp | CallSignal
    ciphertext: Vec<u8>,        // XChaCha20-Poly1305 com a chave de conteúdo
    sig: [u8; 64],              // Ed25519 da identity key do autor
}

struct SealedKey {
    recipient: [u8; 32],        // identity_pk (visível ao carrier: necessário para roteamento)
    ephemeral_pk: [u8; 32],
    sealed: Vec<u8>,            // chave de conteúdo cifrada via X25519 efêmero-estático + HKDF
}
```

Regras:
- Validação ao receber: versão suportada → `id` recalculado → assinatura → autor é contato ou membro do grupo → dedup por `id`. Qualquer falha descarta e registra em `tracing`.
- Ordenação na UI: ordem topológica por `parents`, desempate por `hlc`, depois por `id`.
- **Sem forward secrecy no MVP** (documentado em `threat-model.md`). Campo `version` permite migrar para MLS (`openmls`).

### 7.2 Grupos pequenos (≤ 20)

- `conversation_id` = hash aleatório gerado pelo criador.
- Operações de grupo (`GroupOp::Create/Add/Remove/Rename`) são envelopes assinados. No MVP, **só o criador** pode adicionar ou remover membros, o que evita resolução de conflito de permissões.
- Conteúdo: uma chave de conteúdo por mensagem, selada para cada membro (fan-out). Aceitável até 20 membros.
- Membros do mesmo grupo podem atuar como carriers entre si mesmo sem serem contatos diretos. **Isso expõe IP entre membros**: o app avisa ao entrar no grupo.

### 7.3 Sincronização

ALPN `enxame/sync/1`. Ao conectar:
1. `Hello { device_cert, protocol_versions, role, capabilities }` trocado nos dois sentidos.
2. Para cada conversa em comum: `Heads { conversation_id, heads: Vec<id>, count, hlc_max }`.
3. Se divergir: `RangeQuery { conversation_id, since_hlc }` → `Ids { ... }` → `Want { ids }` → `Envelopes { ... }`.
4. MVP aceita reconciliação simples por faixa de HLC. Reconciliação baseada em set (RBSR) é pós-MVP. Registrar em ADR.

Frames: `u32` length-prefix + `postcard`. Tamanho máximo de frame configurável (padrão 1 MiB). Arquivos nunca passam por `sync`, só por `blobs`.

---

## 8. Papéis e avaliação de capacidade

### 8.1 Sinais coletados (`roles::Signals`)

| Sinal | Fonte |
|---|---|
| `nat_kind` | Resultado do iroh (conectividade direta vs relay) + sondagem STUN/QAD: `Open`, `Cone`, `Symmetric`, `Unknown` |
| `upload_kbps_est` | Medição **passiva** de throughput das transferências reais (EWMA). Nunca speedtest ativo. |
| `on_ac_power` | Linux: `/sys/class/power_supply`; Windows: `GetSystemPowerStatus`; macOS: IOKit (best-effort). Desktop sem bateria = `true`. |
| `battery_pct` | idem |
| `metered` | Linux: NetworkManager `Metered` (D-Bus); Windows: `NetworkCostType` |
| `link_kind` | `Ethernet`, `Wifi`, `Cellular`, `Unknown` (tethering via celular conta como `Cellular` quando detectável) |
| `thermal_ok` | Desktop: `true` no MVP |
| `session_uptime` | Tempo desde que o app ficou ativo (inclui tempo na bandeja) |
| `cpu_free`, `mem_free` | `sysinfo` |
| `foreground` | Reservado para mobile. No desktop é sempre `true` enquanto o processo roda, inclusive na bandeja. |
| `game_mode` | Toggle manual do usuário (detecção automática é pós-MVP) |

Todos os sinais vêm da trait `PlatformSignals`. O MVP implementa Linux e Windows (macOS best-effort). Sinais que não podem ser lidos numa plataforma assumem o valor **conservador**.

### 8.2 Papéis

```rust
enum Role { Leaf, LeafPlus, Peer, Relay, SuperPeer }
```

| Papel | Pode fazer |
|---|---|
| `Leaf` | Só consome. |
| `LeafPlus` | Semeia blobs que já tem localmente; entrega envelopes de carry que já guarda; aceita carry **só de texto**, com quota pequena. |
| `Peer` | + aceita carry de texto com quota normal; serve blobs sob demanda. |
| `Relay` | + aceita carry de envelopes `FileRef`; replica blobs pequenos de contatos (≤ limite). |
| `SuperPeer` | + quota alta de carry, guarda mensagens offline por TTL longo e é preferido como carrier. |

### 8.3 Regras de decisão (iniciais, ajustáveis via config)

```
se game_mode                                  → Leaf
se contribuição desativada pelo usuário       → Leaf
se link_kind == Cellular ou metered           → LeafPlus
se battery_pct < 20 e !on_ac_power            → Leaf
se !thermal_ok                                → Leaf
se link_kind == Wifi e !on_ac_power           → Peer     (notebook na bateria)
se on_ac_power e nat_kind in {Open, Cone}
   e upload_kbps_est >= 5000
   e session_uptime >= 10min                  → SuperPeer
se on_ac_power                                → Relay
senão                                         → Peer
```

**Histerese:**
- Promoção exige a condição estável por `promote_after` (padrão: 2 min para Peer/Relay, 10 min para SuperPeer).
- Rebaixamento é **imediato**.
- Mudança de papel emite `RoleChanged` e é anunciada aos contatos conectados (`RoleUpdate`).

**Budget do usuário** (sempre aplicado, acima do papel):
- `max_contrib_kbps` (padrão: 30% do `upload_kbps_est`);
- `max_contrib_gb_month` (padrão 20 GB no desktop);
- `carry_quota_mb` por contato e total.

Estourou o budget → rebaixa para `LeafPlus` até o próximo ciclo.

**Resiliência (lição do Skype 2010):** o papel é decisão local, sem coordenação global, e um bug que derrube o processo não pode propagar estado para outros nós. Promoções são lentas e a versão de protocolo é negociada no `Hello`.

---

## 9. Store-and-forward (carry)

ALPN `enxame/carry/1`.

1. A envia para B. Se B estiver online, entrega direto via `sync`.
2. Se B estiver offline, A escolhe até **K = 3 carriers** entre contatos em comum com B (ou membros do grupo) que estejam online, priorizando `SuperPeer > Relay > Peer > LeafPlus` e, dentro do papel, o maior uptime histórico.
3. A envia `CarryOffer { for_recipient: B, envelopes }`. O carrier aceita ou recusa conforme papel e quota (`CarryAccept`/`CarryReject`).
4. O carrier guarda com TTL (padrão 7 dias; `SuperPeer` 30 dias).
5. Quando o carrier vê B online: `CarryDeliver`. B valida normalmente.
6. B emite um `Receipt` assinado (envelope `kind = Receipt` contendo os ids), que se propaga pelos mesmos caminhos. Carriers que o recebem **expurgam** as cópias.
7. Qualquer nó com papel ≥ `LeafPlus` atua como "mensageiro": ao sincronizar com qualquer contato, entrega o que carrega para ele. Isso já deixa pronto o comportamento que o mobile vai usar (seção 17).

Contabilidade: cada byte servido para terceiros incrementa `contribution_stats` (por contato e por mês), que alimenta o painel e o budget.

---

## 10. Arquivos

- O remetente cifra o arquivo com uma chave aleatória (XChaCha20-Poly1305 em streaming por chunks; documentar o formato em ADR) e importa o **ciphertext** no `iroh-blobs`.
- A mensagem `FileRef { blob_hash, size, name, mime, file_key }` vai dentro do envelope E2E. Só os destinatários têm a `file_key`.
- O download usa **todos os contatos que anunciarem ter o blob** (multi-fonte), com verificação BLAKE3 e resume nativos.
- Quem baixou passa a semear automaticamente, respeitando papel e budget.
- `Relay`/`SuperPeer` podem pré-replicar blobs ≤ `prefetch_max_mb` (padrão 50 MB) dos contatos para melhorar a disponibilidade.
- Garbage collection: blobs sem referência local e fora de TTL de seeding são removidos.

---

## 11. Rede

- Endpoint iroh com relay configurável: em dev usar os relays públicos padrão do iroh; em produção, o relay próprio do `deploy/`. A URL do relay vai no ticket.
- Discovery: discovery padrão do iroh (DNS/pkarr) + discovery em rede local (mDNS).
- Métricas por conexão, expostas em `diag` e logadas: tipo de caminho (`Direct`/`Relay`/`Mixed`), RTT, tempo até a conexão direta, trocas de caminho.
- **Essas métricas são o principal entregável de validação técnica da Fase 1.**

### 11.1 Nó headless e deploy

`enxame-node`: mesmo core, sem UI, papel fixo `SuperPeer`, budget configurável por env/arquivo. Uso: quem tem VPS ou homelab sobe um nó 24/7 para a sua rede de contatos.

`deploy/docker-compose.yml`:
- `iroh-relay` com TLS (Let's Encrypt ou certificado manual);
- `enxame-node` com volume persistente;
- healthchecks, `restart: unless-stopped` e limites de recurso.

---

## 12. Segurança (resumo; detalhar em `docs/threat-model.md`)

| Ameaça | Mitigação no MVP | Limitação conhecida |
|---|---|---|
| Carrier lê mensagem | E2E por destinatário | Carrier vê `recipient` e tamanho (metadado) |
| Mensagem forjada ou alterada | Assinatura Ed25519 + id por hash | — |
| Desconhecido injeta tráfego | Só contatos mútuos ou membros de grupo | — |
| Exposição de IP | Só contatos veem IP | Membros de grupo veem IP entre si (aviso na UI) |
| Comprometimento de chave | — | Sem forward secrecy (MLS pós-MVP) |
| Flood de carry | Quotas por contato e total, TTL | — |
| Chave em disco | Keystore do SO via `keyring` | Fallback em arquivo com permissão restrita |
| Bug derruba a rede | Papéis locais, sem estado global, protocolo versionado | — |

---

## 13. Fases e critérios de aceitação

### Fase 0 — Fundação
- Workspace Cargo, crates vazias com a API pública esboçada, CI (fmt, clippy, test, cargo-deny), `CLAUDE.md`.
- ADRs: `0001-licenca`, `0002-stack`, `0003-formato-envelope`.
- **Aceite:** CI verde; `cargo run -p enxame-cli -- --help` funciona.

### Fase 1 — Identidade e conectividade
- Geração e persistência de chaves, `DeviceCert`, `InviteTicket` (gerar/importar/QR em texto no CLI).
- Endpoint iroh, `Hello`, aceitação mútua de contato.
- `enxame-cli diag` mostrando NAT, caminho direto/relay, RTT.
- `scripts/netsim/`: network namespaces + nftables simulando NAT cone e simétrico em uma única máquina Linux.
- **Aceite:** duas máquinas em redes diferentes viram contatos via ticket; `diag` mostra o caminho; no netsim, o cenário simétrico cai em relay e o cone faz hole punching. Relatório com as métricas em `docs/reports/fase1-conectividade.md`.

### Fase 2 — Mensagens 1:1
- `store` com migrations, `crypto` (selagem e assinatura), `sync`.
- CLI: `send`, `chat` (modo interativo), `history`.
- **Aceite:** mensagens persistem e sincronizam após reconexão; envelope adulterado é rejeitado (teste); dedup funciona; testes de integração com N nós in-process.

### Fase 3 — Papéis, carry e nó headless
- `roles` completo com sinais Linux; `RoleUpdate` entre contatos.
- `carry` completo com receipts e expurgo.
- `enxame-node` + `deploy/docker-compose.yml`.
- CI ganha o job `cargo check -p enxame-core --target aarch64-linux-android` (guarda-corpo mobile-ready, seção 17.1).
- **Aceite:** cenário A, B, C mútuos: B offline, A envia e fica offline, C online; B volta e recebe via C; o receipt remove a cópia em C. O mesmo cenário com `enxame-node` no lugar de C. Teste de histerese (promoção atrasada, rebaixamento imediato) com relógio mockado. Check do target Android verde.

### Fase 4 — Arquivos
- Cifragem em streaming, `iroh-blobs`, `FileRef`, seeding automático, multi-fonte, GC, budget.
- **Aceite:** arquivo de 1 GB baixado de 2 fontes simultâneas; interrupção e resume; hash verificado; o budget limita a taxa servida.

### Fase 5 — Grupos pequenos
- `GroupOp`, fan-out, membros como carriers, aviso de IP.
- **Aceite:** grupo de 5 nós in-process; add/remove pelo criador; removido não descriptografa mensagens novas; carry entre membros funciona.

### Fase 6 — App Flutter desktop (Linux → Windows)
- `enxame-ffi` com flutter_rust_bridge v2; stream de eventos → estado da UI.
- Telas: onboarding (gera identidade), contatos (ticket como link copiável + QR exibido), lista de conversas, chat, envio de arquivo com drag-and-drop, painel de contribuição (papel atual, MB servidos, contatos ajudados, limites, toggle de modo jogo), diagnóstico.
- Bandeja do sistema: fechar a janela mantém o nó ativo; menu da bandeja com status, modo jogo, pausar contribuição e sair.
- Autostart opcional com o sistema (desligado por padrão).
- Discovery mDNS habilitado.
- Ordem: Linux (X11 e Wayland; testar em Hyprland e BSPWM) → Windows. macOS só se não exigir trabalho específico relevante.
- Empacotamento: AppImage (Linux) e instalador MSIX ou Inno Setup (Windows). Registrar a escolha em ADR.
- **Aceite:** Linux ↔ Windows trocam mensagens e arquivos em redes diferentes; com a janela fechada, o app na bandeja continua atuando como carrier; o papel muda ao ligar o modo jogo ou desconectar o carregador do notebook; duas máquinas na mesma LAN conectam direto sem relay.

### Fase 7 — (Stretch) Call 1:1
- Mídia via `flutter_webrtc`; sinalização (SDP/ICE) transportada como envelopes `CallSignal` sobre a conexão iroh já existente.
- **Aceite:** chamada de voz e vídeo 1:1 entre dois desktops na mesma rede e em redes diferentes com NAT cone. Documentar o comportamento com NAT simétrico (provável falha sem TURN). A solução fica em ADR para pós-MVP.

---

## 14. Testes

- **Unitários:** crypto (round-trip, adulteração), parsing de ticket, regras de papel (tabela de casos), HLC e ordenação.
- **Integração:** harness que sobe N instâncias do core no mesmo processo, com relay local de teste quando necessário; cenários das fases 2–5 automatizados.
- **Rede real/simulada:** `scripts/netsim` para NAT; roteiro manual documentado para teste de desktops em fibra, 4G (via tethering do celular) e Starlink, registrando taxa direto vs relay.
- **Propriedade:** `proptest` para sync (qualquer ordem de entrega converge para o mesmo conjunto e a mesma ordenação).
- Nenhum teste depende de relay público da internet no CI.

---

## 15. Convenções

- Commits: Conventional Commits (`feat(core): ...`, `fix(carry): ...`).
- Sem `unwrap()`/`expect()` fora de testes e `main`.
- Todo tipo de wire em `proto`, com teste de round-trip e entrada em `docs/protocol.md`.
- Configuração: arquivo TOML + override por env (`ENXAME_*`); padrões seguros.
- Logs sem conteúdo de mensagem nem chaves; ids truncados.
- Público open source: README com proposta, arquitetura em 1 diagrama, como rodar CLI e nó, como contribuir; `CONTRIBUTING.md` e `SECURITY.md` ao fim da Fase 3.

---

## 16. Decisões em aberto (registrar em ADR quando decididas)

1. **Nome definitivo** do projeto.
2. **Licença:** recomendação AGPL-3.0 para o projeto inteiro (impede fork fechado oferecido como serviço, como em Signal e Element). Alternativa: MIT/Apache-2.0 dual no `enxame-core` para maximizar adoção como biblioteca, AGPL no app.
3. Reconciliação por HLC (MVP) vs RBSR.
4. Formato de cifragem em streaming de arquivos.
5. Estratégia para calls com NAT simétrico (TURN próprio vs mídia sobre o relay do iroh).
6. Caminho para MLS (`openmls`) e multi-dispositivo.
7. Empacotamento e distribuição desktop (AppImage/Flatpak/AUR no Linux; MSIX vs Inno Setup no Windows).

---

## 17. Roadmap mobile (pós-MVP)

O mobile **não é implementado no MVP**, mas o desenho já o considera. O objetivo é que levar o app para Android seja trabalho de cliente e de sinais da plataforma, não de protocolo.

### 17.1 Guarda-corpos durante o MVP (obrigatórios)

- `enxame-core` compila para `aarch64-linux-android` (job de CI a partir da Fase 3). Nada de dependência exclusiva de desktop no core.
- Tudo que é específico de SO fica atrás de traits (`PlatformSignals`, keystore), implementadas fora da lógica central.
- O protocolo assume **sessões curtas e intermitentes**: sync e carry funcionam com conexões que caem a qualquer momento e retomam do ponto certo.
- Roaming de IP (troca de rede) não pode quebrar sessão lógica. Testar trocando Wi-Fi ↔ cabo no desktop.
- Os papéis `Leaf` e `LeafPlus` são implementados e testados no MVP, mesmo que o desktop raramente os use, porque serão o padrão do mobile.
- O stream de eventos do core não presume UI sempre ativa (o mobile vai suspender a UI).

### 17.2 Fase M1 — Android

- Implementação de `PlatformSignals` para Android via platform channels: `BatteryManager`, `ConnectivityManager` (metered, celular), thermal status, foreground/background.
- Regras de papel do mobile:
  - app em foreground + 4G/5G → `LeafPlus` (semeia o que já tem, entrega carry, aceita carry só de texto com quota pequena);
  - app em foreground + Wi-Fi → `Peer`;
  - Wi-Fi + carregando + **foreground service opt-in** ("Ajudar meus contatos enquanto carrega no Wi-Fi", com notificação persistente) → `Relay`;
  - background sem o serviço, bateria baixa ou aquecimento → `Leaf`.
- Mobile **nunca** é `SuperPeer`, nunca atua como SFU e nunca faz relay pesado em dados móveis.
- Budget mobile padrão: 1 GB/mês, ajustável.
- Mobile como "mensageiro": como abre o app várias vezes ao dia, ele é o principal portador de mensagens offline entre contatos.
- Telas adaptadas: scanner de QR para adicionar contato, compartilhamento de arquivos pelo share sheet do Android.
- **Aceite:** desktop ↔ Android trocam mensagens e arquivos; o Android muda de papel ao alternar Wi-Fi/4G/carregador; dois celulares na mesma Wi-Fi conectam por LAN sem relay; mensagem offline chega via celular atuando como carrier.

### 17.3 Fase M2 — iOS (depois do Android estável)

- Contribuição só em foreground (o iOS não permite serviço contínuo em background); call ativa via CallKit.
- Papel máximo: `Peer` em Wi-Fi com o app aberto.
- Sem push notification via servidor próprio no início. Avaliar em ADR o trade-off de privacidade de usar APNs.

---

## 18. Prompt inicial sugerido para o Claude Code

```
Leia o arquivo enxame-mvp.md inteiro. Ele é a especificação do MVP deste repositório.
Siga a seção 0 à risca. Comece pela Fase 0: crie o workspace, o CLAUDE.md, a CI e os
três ADRs iniciais. Antes de adicionar dependências de rede, verifique na documentação
oficial a versão estável atual do iroh e do iroh-blobs e fixe as versões. Ao terminar a
Fase 0, rode fmt/clippy/test, descreva o que foi feito e pare para revisão.
```
