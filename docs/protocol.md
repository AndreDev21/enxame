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
