# Relatório da Fase 3 — papéis, carry e nó headless

Data: 2026-10-22. Validação automatizada e ponta a ponta local; o teste em máquinas reais está
descrito no fim e **ainda não foi feito**.

## Critérios de aceite

| Critério (spec 13) | Como é verificado | Resultado |
|---|---|---|
| Cenário A, B, C mútuos: B offline, A envia e fica offline, C online; B volta e recebe via C; o recibo remove a cópia em C | `b_offline_a_sends_and_leaves_c_carries_b_returns_and_receipt_purges_the_copy` (`tests/carry.rs`, nós in-process, QUIC real em localhost): A→C (oferta aceita) → A cai → B volta e fala com C → mensagem entregue, recibo volta a C na mesma sessão e a cópia some; depois A volta, aprende o recibo e a mensagem passa a "entregue" | ok |
| O mesmo cenário com `enxame-node` no lugar de C | (1) `same_scenario_with_a_headless_super_peer_node_as_carrier` (papel fixo `super_peer`, como o nó configura); (2) `scripts/e2e/carry-with-node.sh`: **binários reais** `enxame-cli` e `enxame-node` em processos separados, B derrubado antes do envio; o nó (`role=SuperPeer`) passa a `carry_items=1`, entrega quando B volta e termina em `carry_items=0` com 546 bytes contabilizados | ok |
| Teste de histerese (promoção atrasada, rebaixamento imediato) com relógio mockado | `promotion_is_delayed_and_demotion_is_immediate`, `unstable_condition_restarts_the_timer`, `changing_target_while_waiting_restarts_the_timer` (`RoleEngine`) e `roles_follow_signals_with_hysteresis_on_a_mocked_clock` (nó completo, `StaticSignals` + `tick_roles_at`) | ok |
| Check do target Android verde | `cargo check -p enxame-core --target aarch64-linux-android` com o NDK 28 (local) e job `android` no CI | ok local; CI a confirmar no push |

## Outros testes da fase

- **Regras de papel**: tabela com 21 casos (`decision_table`), mais interruptores do usuário e
  budget; `Leaf` e `LeafPlus` são exercitados nos testes de carry (recusa e quota pequena).
- **Sinais Linux** lidos de diretórios falsos: bateria/tomada, rota padrão, classificação de
  interface, valores do NetworkManager.
- **Carry (sessão)**: oferta aceita/recusada por papel, quota, destinatário desconhecido,
  envelope adulterado, autor errado; aceitar de novo é idempotente; recibo expurga todas as
  cópias redundantes; mensagem já confirmada não é carregada de novo; recibo forjado por terceiro
  não expurga nada; entrega de autor desconhecido é descartada sem recibo.
- **Seleção de carriers**: com 5 contatos em comum de papéis diferentes, só `SuperPeer`, `Relay` e
  `Peer` guardam cópia (K = 3) e `Leaf` nunca.
- **Budget/quota/limitador**: teto mensal estourado rebaixa a `LeafPlus`; quota 0 recusa; o limitador
  de banda espaça os envios (relógio pausado).
- **RoleUpdate e contatos em comum**: anúncio no meio da sessão; hashes salgados por conversa.
- **Política do nó headless**: só fingerprints permitidas entram, sem perguntar.

Total: **102 testes** passam (`cargo test --workspace`), mais `clippy -D warnings`, `fmt` e
`cargo deny check`.

## Deploy

Verificado localmente com Docker 29:

- `docker compose config` valida o `deploy/docker-compose.yml` (e o CI repete isso).
- **Imagem do nó** (`Dockerfile.node`, 113 MB): constrói; o contêiner sobe como `uid 10001` com 512 MB e
  2 CPUs, fica `role=SuperPeer`, `enxame-node health` retorna 0, `identity.key`, `health` e
  `node-ticket.txt` ficam com `0600`, e `docker stop` (SIGTERM) encerra com código 0.
- **Imagem do relay** (`Dockerfile.relay`, `iroh-relay` 1.4.1): constrói; com
  `RELAY_CERT_MODE=Manual` o `entrypoint` gera o `relay.toml`, `http://…/generate_204` responde 204,
  a porta 443 aceita conexão (healthcheck) e a 7842/udp é publicada.
- Não testado: Let's Encrypt (precisa de DNS e das portas 80/443 públicas) e `compose up` completo
  num servidor real.

## Validação em campo (2026-10-22)

Três máquinas Linux, relay público padrão do iroh, coordenadas pelo próprio Enxame (A = orquestrador):
**A** (fibra residencial), **B** (outra máquina, mesma LAN e mesmo IP público de A) e **C** (outra
cidade, outro ISP). C rodou com `--role peer`. B e C só viraram contatos durante o teste.

| Passo | Resultado |
|---|---|
| B e C viram contatos (ticket repassado por mensagem de A) | ok, fingerprints conferidas |
| Aquecimento: contatos em comum descobertos | ok: o banco de A listou B↔C nos dois sentidos; C como `Peer`, B como `Leaf` |
| Rodada 1: B sai; A escreve → carrier aceita (`offline; 1 carrier(s) vão entregar`) | ok |
| A sai; B volta e recebe **por C** (C: 562 bytes servidos, 0 guardadas depois) | ok; a sonda chegou sem A online |
| Recibo: A volta e vê ✓ nas mensagens | ok (0 pendentes) |
| Rodada 2 (com a correção abaixo): A escreve R2-1, R2-2, R2-3 com B offline | ok: as três aceitas por C |
| A sai; B volta e recebe R2-1, R2-2, R2-3 sozinho, na ordem, sem duplicatas | ok (relato de B) |
| A volta: R2-1..3 como entregues (✓), 0 pendentes | ok |

Leituras de `/contribuicao` em C durante a rodada 2: 2 guardadas (01:15), depois 1 guardada e
3886 bytes servidos (01:18). A "1" restante é provavelmente uma mensagem do próprio B para A que C
carregou enquanto A estava offline (o carry funciona nos dois sentidos); a leitura final pedida a
C (esperado: 0) não chegou antes do fechamento deste relatório.

### Achados do campo (todos corrigidos e publicados)

1. **DNS com EDNS descartado** (roteador de B, MagicDNS do Tailscale em A): o relay ficava
   inalcançável e o ticket saía só com endereços da LAN. Ocorreu duas vezes. Correção:
   `dns_servers` / `ENXAME_DNS_SERVERS` (validado: sem a opção `udp: false`; com
   `1.1.1.1,8.8.8.8`, NAT detectado e relay conectado).
2. **Bug de carry**: o carrier que já guardava a primeira mensagem pendente era tratado como se
   guardasse todas, então as seguintes não lhe eram oferecidas (na rodada 1 só a "sonda" foi
   carregada; as outras chegaram direto quando A voltou). Corrigido em `offer_to_carriers`, com
   teste de regressão (`every_message_to_an_offline_contact_is_carried_not_just_the_first`). A
   rodada 2 confirma em campo.
3. **`serve` não saía com Ctrl-C** (a leitura de stdin do tokio prendia o runtime). Corrigido.
4. `@nome` sem texto no `chat` era enviado como mensagem. Corrigido.

### Limites desta validação

- A e B compartilham LAN/IP público; só C está em outra rede.
- O nó headless (`enxame-node`) foi validado localmente (testes e script e2e), não em campo.
- Windows e rede móvel seguem sem teste.
