# Relatório da Fase 5 — grupos pequenos

Data: 2026-10-22. Validação automatizada (nós in-process) e em campo (seção abaixo). Roteiro:
`docs/roteiros/fase5-grupos.md`. Decisões em `docs/adr/0009-grupos-pequenos.md`; wire em
`docs/protocol.md`.

## Critérios de aceite (spec 13)

| Critério | Como foi verificado | Resultado |
|---|---|---|
| Grupo de **5 nós** in-process | `five_nodes_create_add_message_remove_and_the_removed_cannot_read_new_messages`: ana cria com beto, cris e dani; edu entra depois | ok |
| Add/remove **pelo criador** | O mesmo teste; `add`/`remove` por quem não é criador falham ("só o criador…"); o criador não pode ser removido | ok |
| **Removido não descriptografa** mensagens novas | Dani é removida; recebe o retrato (evento `GroupRemoved`), não obtém "sem o dani" em nenhuma sincronização e não consegue mais enviar | ok |
| **Carry entre membros** funciona | `a_member_who_is_not_the_recipients_contact_carries_for_the_group`: dani cai, ana escreve (dani é pendente), ana sai; beto (que **não** é contato de dani) carrega, dani volta e recebe pelo `carry_now("beto")`; o recibo esvazia a fila de beto | ok |
| Aviso de IP | `GroupJoined.exposed` lista os membros que não são contatos (testado: beto vê cris e dani); a CLI imprime o aviso | ok |

Outros testes: `a_member_catches_up_from_another_member_while_the_author_is_offline` (dani volta e
puxa de beto e cris, que não são seus contatos, sem o autor), `group_limits_and_validation`, e os
unitários de `groups` (retrato válido/inválido: membro repetido, criador fora, > 20, nome,
certificado trocado, lixo; `group_only_peers`).

## Validação em campo (2026-10-22)

Máquinas: **André** (criador, Linux, LAN), **bob** (Linux, LAN, `Relay`), **jean** (Linux, LAN,
`Peer`) e **notebook** (Windows, hotspot 4G, `Peer`), todos no commit `de8d7a5`. Todos já eram
contatos entre si, então o aviso de IP saiu vazio (o caso com não-contatos fica nos testes).

| Passo | Resultado |
|---|---|
| Criar `#teste` com bob, jean e notebook | ok: entregue direto aos 3; cada um viu "você entrou no grupo" e `/grupos` correto |
| Mensagens dos 4 autores | ok: histórico idêntico (4 mensagens) nas 4 máquinas, inclusive do 4G; `entregue a 3` |
| Notebook offline (2 min), André envia 2 mensagens | ok: `entregue a 2; offline: notebook; 2 carrier(s)`; ao voltar, as 2 chegaram sozinhas em 3 s |
| Remover jean | ok: jean viu "você foi removido", `/grupo sync` não trouxe a mensagem nova, `#teste ...` deu "você não está mais neste grupo"; bob e notebook seguiram recebendo |
| Bob (não criador) tenta add/remover | ok: "só o criador do grupo pode mudar a lista de membros" |

### Achados de campo

1. **Exibição no chat (corrigido):** duas mensagens chegando juntas imprimiam a última duas vezes
   e perdiam a primeira ao vivo (o chat lia só a última do histórico a cada evento). Vale também
   para o 1:1. Agora o chat guarda os ids já impressos e imprime só o que é novo.
2. `/grupo sync` num grupo de que fui removido ainda disca os ex-colegas (3 "responderam");
   inofensivo (nenhuma conversa em comum é negociada), mas deveria recusar.
3. O aviso de IP com não-contatos não foi exercitado em máquinas reais (todos eram contatos).

## O que foi construído

- `groups` (retratos, validação, pares por grupo), `node/groups.rs` (`create_group`,
  `add_group_member`, `remove_group_member`, `rename_group`, `groups`, `send_group_text`,
  `sync_group`, `group_history`), eventos `GroupJoined/Updated/Removed`.
- Sincronização e carry aceitam co-membros como pares; `ingest`/`ingest_carried` autorizam autores
  pelo retrato.
- Banco v4 (`groups`) e v5 (`pending_delivery`/`carry_queue` por `(envelope, destinatário)`).
- CLI (no `chat`): `/grupos`, `/grupo criar|add|remover|nome|sync|historico`, `#grupo texto`.

## Achados durante a implementação

1. A sincronização esperava um `Heads` de cada conversa que **ela** negociou, mas o par podia
   anunciar um grupo que o outro ainda não conhecia: a sessão ficava parada até o timeout de 30 s.
   Agora responde com `Heads` vazio.
2. `pending_delivery`/`carry_queue` tinham chave só em `envelope_id`: uma mensagem de grupo (vários
   destinatários) ficava entregue para todos assim que um recebesse. Migração v5.
3. O retrato completo (e não operações encadeadas) é o que permite a um membro novo entrar sem
   decifrar o passado.

## Limites desta validação

- O campo cobriu 4 máquinas reais (inclui Windows no 4G), mas só com contatos entre si: o caso de
  co-membros que não são contatos, o carry por eles e o aviso de IP preenchido só foram testados
  in-process.
- Sem sair do grupo por conta própria, sem troca de criador, sem arquivos em grupo validados em
  campo (o `FileRef` é aceito na conversa de grupo, mas servir blobs a co-membros só foi coberto
  pela política de pares, não por um teste de ponta a ponta de grupo).
- Nomes de grupo e membros sem espaços nos comandos da CLI.
