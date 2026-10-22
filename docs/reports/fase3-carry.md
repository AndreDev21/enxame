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

## Pendente: teste em máquinas reais (A, B, C)

Pré-requisito: as três máquinas viram contatos mútuas **entre si** (hoje A é contato de B e de C,
mas B e C ainda não são contatos).

1. **B ↔ C**: B roda `serve` e copia o ticket; C faz `add '<ticket>'`; B aceita.
2. Em cada máquina, `chat <contato>` por ~10 s com cada um dos outros dois (troca a lista de
   contatos em comum).
3. **B fecha o chat.** Em A: `send <B> "teste de carry"`. Esperado:
   `Guardada; o contato está offline. 1 contato(s) em comum vão carregá-la ...`.
4. **A fecha o chat/serve.** C continua online (`chat` ou `serve`).
5. **B reabre** o chat com C (`chat <C>`): a mensagem de A deve aparecer sem A estar online.
6. **A reabre** o chat: `history <B>` passa de `…` a `✓` (recibo entregue por C).
7. Em C, `/contribuicao` no chat mostra os bytes servidos no mês.
