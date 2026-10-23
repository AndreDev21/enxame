# Protocolo de wire

Mantido em sincronia com `crates/enxame-core/src/proto.rs` e `identity.rs`. Versão atual: **1**.
Serialização: `postcard`. Todo tipo de wire tem teste de round-trip.

## Transporte

QUIC via iroh. A autenticação do par é a do TLS do iroh: o `EndpointId` (NodeId) do par é a chave
pública do dispositivo. Cada protocolo tem um ALPN versionado.

| ALPN | Quem pode abrir | Conteúdo (Fase 1) |
|---|---|---|
| `enxame/contact/1` | qualquer um | `ContactMsg` (pedido de contato) |
| `enxame/sync/1` | só contatos mútuos | `Hello` nos dois sentidos |

Conexão em `enxame/sync/1` de quem não é contato mútuo é fechada com código `403`.

## Framing

`u32` big-endian com o tamanho do corpo + corpo `postcard`. O limite (`max_frame_bytes`, padrão
1 MiB) é verificado antes de alocar. Arquivos nunca passam por estes streams.

## Tipos

### `DeviceCert`
`{ identity_pk: [u8;32], node_id: [u8;32], created_at: u64, sig: Signature }`.
`sig` = Ed25519 da identidade sobre `"enxame/devicecert/1\0" ‖ identity_pk ‖ node_id ‖ created_at (LE)`.

### `InviteTicket`
`{ version: u8 (=1), identity_pk, enc_pk, device_cert, node_addr: EndpointAddr, display_name,
expires_at: u64, sig: Signature }`. `sig` = Ed25519 da identidade sobre
`"enxame/invite/1\0" ‖ postcard(ticket com sig zerada)`.

Texto: `enxame1` + base32 minúsculo sem padding do `postcard`. Validação (`verify`): versão, validade
(e teto de 30 dias), tamanho do nome, assinatura do ticket, assinatura do `DeviceCert`, mesma
identidade, e `device_cert.node_id == node_addr.id`.

### `ContactMsg` (`enxame/contact/1`)
`Request { ticket: InviteTicket } | Accept | Reject`.

Fluxo: B abre `contact/1` em A, abre um stream bidirecional e envia `Request` com o **próprio**
ticket. A valida o ticket e exige `ticket.node_addr.id == EndpointId autenticado da conexão`. Se a
identidade já está na agenda de A vinda do mesmo dispositivo (A já tinha pedido contato a B), A
aceita; senão pergunta ao usuário (até 16 pedidos pendentes, timeout configurável). A responde
`Accept`/`Reject`. Só então ambos gravam `mutual = true`.

### `Hello` (`enxame/sync/1`)
`{ device_cert, protocol_versions: Vec<u8>, role: Role, capabilities: u32 }`. O iniciador envia
primeiro; o receptor valida o certificado contra o contato e responde com o seu. A versão em uso é
a maior em comum. `role` é `Leaf` até a Fase 3; `capabilities` é reservado (0).

## Fase 2 — envelopes e sincronização

### `Envelope`
`{ version: u8 (=1), id, conversation_id, author, author_device, hlc: Hlc, parents: Vec<[u8;32]>,
recipients: Vec<SealedKey>, kind: EnvelopeKind, ciphertext: Vec<u8>, sig: Signature }`, todos os
ids/chaves com 32 bytes. `EnvelopeKind` = `Text | FileRef | Receipt | GroupOp | CallSignal` (a
ordem é parte do wire; só se acrescenta no fim; a Fase 2 só aceita `Text`).

- `id` = `BLAKE3("enxame/envelope-id/1\0" ‖ postcard((version, conversation_id, author,
  author_device, hlc, parents, recipients, kind, ciphertext)))`.
- `sig` = Ed25519 da identidade do autor sobre `"enxame/envelope-sig/1\0" ‖ id`.
- `ciphertext` = `nonce (24) ‖ XChaCha20-Poly1305(conteúdo)` com uma chave de conteúdo aleatória
  por mensagem; AAD = `"enxame/envelope-aad/1\0" ‖ conversation_id ‖ author ‖ hlc.ms (LE) ‖
  hlc.counter (LE) ‖ kind`.
- `SealedKey { recipient (identity_pk), ephemeral_pk, sealed }`: X25519 efêmero-estático contra a
  `enc_pk` do destinatário; `HKDF-SHA256(salt = ephemeral_pk ‖ enc_pk, ikm = segredo
  compartilhado, info = "enxame/seal/1")` gera a chave que cifra a chave de conteúdo
  (`nonce ‖ XChaCha20-Poly1305`, AAD = `recipient`). Segredo compartilhado nulo é recusado.
- Limites: ≤ 32 destinatários, ≤ 64 pais, ciphertext ≤ 256 KiB; texto ≤ 16 KiB.
- Em 1:1 o remetente também se inclui como destinatário (recupera o próprio histórico a partir
  do par).
- `conversation_id` 1:1 = `BLAKE3("enxame/dm/1\0" ‖ min(pk_a, pk_b) ‖ max(pk_a, pk_b))`.

### `Hlc`
`{ ms: u64, counter: u32 }`, ordem lexicográfica. Envelopes com `ms` mais de 24 h no futuro são
recusados.

### `SyncMsg` (`enxame/sync/1`, depois do `Hello`, no mesmo stream)
`Heads { conversation_id, heads, count, hlc_max, digest } | RangeQuery { conversation_id,
since_hlc, limit } | Ids { conversation_id, ids, next } | Want { conversation_id, ids } |
Envelopes { envelopes, last } | Bye`.

Sessão simétrica: cada lado envia `Heads` por conversa em comum. Se o `digest` (BLAKE3 dos ids em
ordem crescente) do par difere do local, pagina `RangeQuery` (inclusivo em `since_hlc`, até 1000
ids por página, `next` = HLC do primeiro id que ficou de fora) → `Ids` → `Want` (≤ 256 ids) →
`Envelopes` (frames que cabem em `max_frame_bytes`; `last = true` fecha a resposta). Só se aceitam
envelopes que foram pedidos. Um lado envia `Bye` quando já recebeu os `Heads` do par e não tem
pedidos em aberto; a sessão termina quando os dois enviaram e receberam `Bye`. Silêncio por mais
de 30 s aborta a sessão.

Validação de cada envelope recebido: versão → `id` recalculado → assinatura → conversa permitida
com este par → autor é o par ou eu → HLC não está no futuro → tipo suportado → deduplicação por
`id` → decifra → texto UTF-8 válido. Qualquer falha descarta só aquele envelope e registra em
`tracing`.

## Fase 3 — papéis, contatos em comum e carry

### Mensagens novas em `enxame/sync/1`
- `Common { hashes: Vec<[u8;32]> }` (até 512), enviada no início da sessão. Cada hash é
  `BLAKE3("enxame/common/1\0" ‖ conversation_id ‖ identity_pk_do_contato)` para cada contato mútuo
  de quem envia (exceto o par). O `conversation_id` é o da conversa 1:1 do par, então só quem é
  contato de **ambos** reconhece o hash: os dois lados descobrem a **interseção** dos contatos sem
  revelar o resto da agenda.
- `RoleUpdate { role }`: enviada quando o papel de quem envia muda **durante** a sessão. Fora de
  sessão, o papel vem no `Hello`. O receptor guarda o último papel visto de cada contato.

### `enxame/carry/1` (store-and-forward)
Aberto apenas por contatos mútuos (senão a conexão é fechada com `403`). Mensagens (`CarryMsg`):

| Mensagem | Quem envia | Resposta |
|---|---|---|
| `Receipts { receipts, reply }` | os dois, no início e como resposta a `Deliver` (`reply = true`) | — |
| `Deliver { envelopes }` | carrier → **destinatário** (ao encontrá-lo) | exatamente um `Receipts { reply: true }` |
| `Offer { for_recipient, envelopes }` | autor → carrier ("carregue isto para fulano") | exatamente um `Accept { ids }` ou `Reject { reason }` |
| `Bye` | os dois | — |

`RejectReason`: `NotCarrying` (papel `Leaf`), `Quota`, `NotContact` (destinatário não é meu
contato mútuo), `Invalid`. A sessão termina quando os dois enviaram e receberam `Bye`, que só é
enviado sem respostas pendentes. Silêncio por mais de 30 s aborta.

**Regras do carrier ao receber uma oferta:** papel que aceita carry; destinatário é contato
mútuo meu e diferente do ofertante; cada envelope válido (`verify`), do tipo `Text`, com
`author == ofertante`, com o destinatário entre os `recipients`, HLC não está no futuro e ainda
sem recibo do destinatário; cabe nas quotas por contato e total. Reaceitar um envelope que já
guarda é idempotente.

**Regras do destinatário ao receber `Deliver`:** cada envelope vale se o autor é contato mútuo
meu, a conversa é a 1:1 entre nós, não está no futuro e eu sou destinatário; o conteúdo vai para
o histórico como mensagem recebida. Para cada autor, devolve um recibo dos ids (mesmo dos já
duplicados).

### Recibo
Envelope `kind = Receipt` assinado pelo destinatário: os ids confirmados vão em `parents` (em
claro — o carrier precisa lê-los), o conteúdo é vazio, `recipients = [autor da mensagem]`. Recibos
**não** entram no DAG da conversa; ficam numa tabela própria por 30 dias. Quem recebe um recibo
válido, de um contato mútuo, apaga as cópias e as pendências daquele destinatário com aqueles ids
(`DELETE ... WHERE recipient = autor_do_recibo`: um recibo só vale para mensagens destinadas ao seu
autor). Recibos são espalhados a cada sessão de carry (os 256 mais recentes).

### Papéis (`Role`)
`Leaf < LeafPlus < Peer < Relay < SuperPeer`. Limites de carry por papel (padrões):

| Papel | Carry | Quota por contato / total | TTL |
|---|---|---|---|
| Leaf | não | — | — |
| LeafPlus | só texto | 1 MB / 5 MB | 7 dias |
| Peer | texto | 10 MB / 100 MB | 7 dias |
| Relay | texto (e `FileRef` a partir da Fase 4) | 25 MB / 250 MB | 7 dias |
| SuperPeer | alta quota | 100 MB / 2 GB | 30 dias |

## Fase 4 — arquivos

### `FileRef` (conteúdo de um envelope `kind = FileRef`)
`postcard( { blob_hash: [u8;32], size: u64, name: String, mime: String, file_key: [u8;32] } )`,
cifrado como qualquer envelope. Validação ao receber: `size <= max_file_mb`, nome de 1 a 255
caracteres, MIME ASCII sem espaços (até 127). `size` é o tamanho **em claro**.

### Formato do arquivo cifrado (`filecrypt`)
```
"ENXF" | versão=1 | tam_bloco=65536 (u32 BE) | prefixo_nonce (19)     ← 28 bytes
bloco_0 (65536 + 16) | bloco_1 | ... | último (1..=65536 + 16; 16 se o arquivo é vazio)
```
Bloco `i` cifrado com XChaCha20-Poly1305 e nonce `prefixo ‖ i (u32 BE) ‖ último (0/1)`.
Tamanho total = `28 + tamanho + 16 · max(1, ⌈tamanho/65536⌉)`.

### Blobs
Ciphertext cortado em segmentos de 8 MiB; cada segmento é um blob BLAKE3 e `blob_hash` é a raiz do
`HashSeq` com os hashes na ordem. Transporte: protocolo `iroh-blobs` (ALPN do crate), aceito só de
contatos mútuos. O download pede a raiz inteira (`GetRequest::all`) e o `Downloader` reparte os
segmentos entre os provedores.

### `Offer.blobs` (`enxame/carry/1`)
`Offer { for_recipient, envelopes, blobs: Vec<BlobOffer { hash, enc_size }> }`. Um carrier
`Relay`/`SuperPeer` que aceitar ao menos um envelope busca os blobs do ofertante (limite
`prefetch_max_mb`, quota de carry, e corte se passar de `enc_size`).

## Fase 5 — grupos pequenos (ADR 0009)

### `GroupSnapshot` (conteúdo de um envelope `kind = GroupOp`)
`postcard( { version: u8 (=1), nonce: [u8;16], creator: [u8;32], seq: u64 (≥ 1), name: String,
members: Vec<GroupMember>, op: GroupOpKind } )`, com
`GroupMember { identity_pk, enc_pk, device_cert, node_addr, display_name }` e
`GroupOpKind = Create | Add{member} | Remove{member} | Rename` (a ordem é parte do wire).

- `group_id = BLAKE3("enxame/group/1\0" ‖ creator ‖ nonce)`; também é o `conversation_id` das
  mensagens do grupo.
- O retrato é o **estado completo**; vale o de maior `seq`. Só o `creator` emite retratos: um
  `GroupOp` cujo autor ≠ `creator` é descartado.
- Validação: versão, `seq ≥ 1`, nome de 1 a 64 caracteres, 1 a 20 membros sem repetição, cada
  `device_cert` com assinatura válida da própria `identity_pk` e `node_id` igual ao do `node_addr`,
  e o criador consta na lista. Tamanho ≤ 64 KiB.
- Um retrato que **não me inclui** só é aceito para um grupo que eu já conheço (é a minha remoção).
- Transporte: envelope na conversa 1:1 entre o criador e cada membro (e o removido), selado só
  para os dois. Sincronização, pendência, carry e recibo funcionam sem mudança.

### Mensagens do grupo
`Text`/`FileRef` com `conversation_id = group_id`, uma chave de conteúdo selada para **todos** os
membros do retrato que o remetente conhece (≤ 20, dentro do limite de 32 destinatários).
`Receipt` continua 1:1. `GroupOp` não circula dentro da conversa do grupo.

### Sincronização e carry com co-membros
- Um co-membro que não é contato entra na lista de pares: o laço de sincronização, o aceite de
  `enxame/sync/1`, `enxame/carry/1` e `iroh-blobs` passam a considerá-lo.
- Uma sessão de sincronização negocia `conversations` = 1:1 (só se o par for contato) ∪ grupos
  ativos que os dois têm. Um `Heads` de conversa que não temos é respondido com `Heads` vazio, para
  o par encerrar a contagem sem esperar o timeout de 30 s.
- Autorização do autor: no grupo, qualquer membro do retrato atual (não precisa ser quem entrega);
  no 1:1, o par ou eu. Em carry de grupo, o autor e eu precisamos constar no retrato.

### Banco (`user_version` 4 e 5)
v4: `groups(group_id, creator, seq, active, snapshot, updated_at)`. v5: `pending_delivery` e
`carry_queue` passam a ter chave `(envelope_id, recipient)`, porque uma mensagem de grupo tem vários
destinatários.

## Fase 6.4 — chamadas (ADR 0012)

### `enxame/call/1`
Uma conexão QUIC por sinal, apenas de contatos mútuos: o iniciador abre um stream bidirecional e envia
`CallMsg::Signal { envelope }`; o receptor valida e responde `CallMsg::Ack` (ou `CallMsg::Reject`).
`CallMsg = Signal { envelope: Box<Envelope> } | Ack | Reject | Refused { reason: String }` (a ordem é parte do wire; `Refused` leva o motivo para o remetente mostrar).

O envelope é `kind = CallSignal`, com `conversation_id` 1:1, sem `parents`, **um** destinatário (o
contato) e conteúdo de 1 a 64 KiB (JSON do app: `invite`, `accept`, `reject`, `busy`, `offer`,
`answer`, `ice`, `hangup`). Validação: id/assinatura, autor = o par, carimbo a ±24 h do relógio de parede, o mesmo limite das mensagens (o remetente usa `Hlc::new(agora, 0)`, nunca o HLC lógico; o receptor não o incorpora ao seu), id inédito
(memória dos 10 000 ids mais recentes) e ≤ 200 sinais por contato a cada 10 s. Não é gravado no banco nem sincronizado.
