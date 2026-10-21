# Relatório da Fase 2 — mensagens 1:1

Testes automatizados (resumo) e validação em campo com máquinas reais. Data: 2026-10-21.

## Testes automatizados

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` (60 testes) e `cargo deny check` passam.

| Critério de aceite | Como é verificado |
|---|---|
| Mensagens persistem e sincronizam após reconexão | `offline_messages_persist_and_sync_after_reconnect`, `history_and_pending_messages_survive_a_restart` (`tests/messages.rs`) |
| Envelope adulterado é rejeitado | `every_tampered_field_is_rejected` (cripto, 10 mutações) e `tampered_envelope_is_rejected_and_not_stored`, `unsolicited_envelope_is_not_stored`, `envelope_from_a_stranger_author_is_rejected`, `envelope_from_the_far_future_is_rejected` (sync) |
| Deduplicação | `insert_dedups_and_survives_reopen`; segunda sessão não troca nada (`one_sided_history_is_pulled`) |
| N nós in-process | `conversations_are_isolated_between_three_nodes` (3 nós; não-contatos não conversam) |
| Convergência (propriedade) | `any_split_of_a_thread_converges` (`proptest`): qualquer divisão de um histórico entre dois lados converge ao mesmo conjunto e à mesma ordem; `same_set_same_order_regardless_of_arrival` para a ordenação |

O teste de propriedade encontrou duas falhas reais do critério inicial de "já em dia" (comparar
só pontas e contagem): ter a ponta sem os ancestrais, e dois lados cada um sem um dos pais da
mesma ponta. Corrigido com um resumo (`digest`) do conjunto de ids no `Heads` (ADR 0005).

## Validação em campo

Cenário: máquina A (Linux, fibra residencial, relay público padrão do iroh), contatos mútuos já
estabelecidos na Fase 1 com a máquina B e a máquina C, cada uma em rede e cidade diferentes.
Todas rodando `enxame-cli chat` com o binário desta versão (o banco da Fase 1 foi migrado do
`contacts.bin` na primeira execução, sem perda).

| Teste | C | B |
|---|---|---|
| Mensagem pendente em A entregue quando o contato entra no chat | ok | — (nada pendente) |
| Contato → A | ok | ok |
| A → contato (resposta, ida e volta) | ok | — |
| Texto longo (~500 caracteres), acentos e emoji | ok | emoji e símbolo (`<3`) ok |
| Contato offline: A escreve 2 mensagens, ficam guardadas (`guardada; contato inacessível`) | ok | ok |
| Contato volta ao chat e recebe as 2 mensagens sozinho, sem reenvio | **ok** (confirmado pelo usuário) | **ok** (confirmado pelo `history` dele: as duas na ordem A → B, sem duplicatas) |
| Isolamento entre conversas (as conversas de A com B e com C não se misturam) | observado: o chat com um contato só mostrou mensagens dele | idem |

O `history` de B (enviado por ele pelo próprio chat) lista, nesta ordem: 3 mensagens dele, as duas
mensagens OFFLINE de A (A, depois B), mais duas dele e o pedido de A. A mesma conversa no banco
de A tem as mesmas 9 mensagens na mesma ordem: ordem idêntica nos dois lados e sem duplicatas.

Pendente: o `history` de C (a ordem foi confirmada só pelo relato dele de que recebeu as duas
mensagens). Windows e rede móvel (4G/CGNAT) continuam sem teste.

## Observações

- Em todas as trocas a sessão começou pelo relay e a mensagem chegou sem o contato precisar discar
  A; o `diag` da Fase 1 mostra se o caminho virou direto.
- O DNS do iroh não funciona com o MagicDNS do Tailscale (descarta consultas EDNS), o que impedia o
  relay em uma das máquinas até se usar um `resolv.conf` próprio para o processo. Um fallback de
  DNS configurável fica como melhoria.
- O endereço de um contato vem do ticket e envelhece: sem relay/DNS, só o lado que reiniciou
  consegue discar. Com relay normal isso não é um problema (visto em campo).
