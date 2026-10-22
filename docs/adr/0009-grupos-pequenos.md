# ADR 0009 — Grupos pequenos (≤ 20)

Status: aceito (Fase 5). Spec: seções 7.2, 7.3, 9 e 15 de `enxame-mvp.md`.

## Contexto

A spec pede grupos de até 20 membros com E2E, só o criador mexendo na lista, membros como carriers
uns dos outros mesmo sem serem contatos e um aviso de que o IP fica exposto entre membros. O
envelope já aceita vários destinatários (até 32, `recipients`), e a sincronização, o carry e os
recibos hoje só conhecem **contatos mútuos**.

## Decisões

### 1. O estado do grupo é um retrato completo, assinado pelo criador

Cada operação (`Create`, `Add`, `Remove`, `Rename`) produz um **`GroupSnapshot`** com a lista
inteira de membros e o nome, numerado por `seq` (1, 2, 3, …). O estado atual é o retrato válido de
maior `seq`. Não há dobra de operações, nem resolução de conflito: só o criador cria retratos, e
eles são totalmente ordenados por `seq`.

Motivo: um membro novo não consegue decifrar as operações anteriores (seladas só para quem era
membro então). Com retratos completos ele precisa apenas do último.

`group_id = BLAKE3("enxame/group/1" ‖ creator_pk ‖ nonce)`. Quem recebe um retrato recalcula o id
e recusa se o criador do retrato não for o que gerou o id: ninguém consegue reivindicar o grupo
de outro.

Cada membro no retrato traz o necessário para falar com ele: `identity_pk`, `enc_pk`,
`device_cert` e `node_addr` (os mesmos campos de um contato) e o nome. O certificado é verificado
(`identity` assina o dispositivo) e o `node_addr` tem de ser o do certificado.

### 2. O retrato viaja por mensagem direta (1:1), não pela conversa do grupo

O envelope é `kind = GroupOp`, na **conversa 1:1 entre o criador e cada membro**, selado só para o
membro e para o criador. Toda a maquinaria existente (sincronização, pendência, carry, recibo)
entrega sem mudança, e o convite funciona antes de existir qualquer conversa de grupo.

Consequências boas: o criador é sempre contato mútuo de todos (só adiciona contatos seus), então o
primeiro retrato nunca precisa chegar por um desconhecido; e o **removido também recebe** o retrato
em que não consta (fica sabendo que saiu).

Consequência a aceitar: se o criador estiver offline, os outros membros só recebem uma mudança de
lista por carriers ou quando ele voltar.

### 3. Mensagens do grupo: uma conversa, um envelope, fan-out por selagem

`conversation_id = group_id`. Texto e `FileRef` são envelopes comuns selados para **todos os
membros do retrato que o remetente conhece** (inclusive ele). O removido deixa de ser destinatário
das mensagens novas e, portanto, não as decifra (spec). Não há re-cifragem do histórico: quem
entra depois lê só o que foi enviado depois que o remetente soube da entrada (sem forward
secrecy, como no 1:1).

### 4. Membros viram pares de rede (sem virar contatos)

Um membro que não é contato meu entra em `peers()` = contatos mútuos ∪ co-membros de grupo. Tudo
que fala com a rede passa a usar `peers()`: laço de sincronização, aceitação de conexões
(`enxame/sync/1`, `enxame/carry/1`), escolha de carriers, servir blobs. As operações de consentimento
(adicionar, listar contatos, enviar 1:1, a lista de "contatos em comum") seguem só com contatos.

Uma sessão com um par negocia a **interseção** das conversas: a conversa 1:1 (só se ele for
contato) mais os grupos ativos que os dois compartilham. Um envelope numa conversa de grupo é
aceito se o **autor for membro** do grupo no retrato atual (não precisa ser o par que entrega: a
sincronização reconcilia o conjunto inteiro, então um membro repassa o que tem).

Como a reconciliação já propaga mensagens entre quaisquer dois membros, "carry entre membros"
acontece naturalmente (D offline volta e sincroniza com C, que tem a mensagem de A). O carry
explícito também aceita autores/destinatários que sejam co-membros e envelopes de conversa de grupo.

### 5. Quem pode o quê

- Só o criador emite retratos: um `GroupOp` cujo autor não seja o criador do retrato é descartado.
- Máximo de 20 membros (inclui o criador); nome ≤ 64 caracteres.
- Mensagem de quem não está no retrato atual é descartada. Isto também descarta, numa corrida
  rara, uma mensagem legítima que chegue depois da remoção do autor.
- Retrato com `seq` menor ou igual ao atual é ignorado.

### 6. Aviso de IP

Ao receber um retrato que **me inclui**, o núcleo emite `Event::GroupJoined` com a lista dos
membros que **não são meus contatos**: eles passam a conhecer meu IP (e eu o deles). A CLI mostra o
aviso; o app da Fase 6 deve pedir confirmação antes de aceitar.

## Fora do escopo (documentado)

- Sair do grupo por conta própria (hoje só o criador remove).
- Troca de criador / vários administradores.
- Re-cifrar histórico para membros novos; forward secrecy.
- Quem foi removido continua conseguindo ler o que membros **ainda sem o retrato novo** selarem
  para ele até esses membros receberem a remoção.
- Arquivos em grupo ficam para depois da validação das mensagens (o `FileRef` já é aceito na
  conversa de grupo, mas a política de servir blobs a co-membros não foi validada em campo).

## Alternativas descartadas

- **Operações encadeadas na conversa do grupo** (`Add`/`Remove` como mensagens do grupo): o membro
  novo não decifra o passado e o primeiro convite exigiria sincronizar uma conversa desconhecida.
- **Chave de grupo compartilhada com rotação**: é o caminho do MLS (pós-MVP); selar por membro é
  suficiente até 20 e não exige nenhuma primitiva nova.
