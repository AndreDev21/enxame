# 0003 — Formato do envelope

Status: aceito

## Contexto

Mensagens atravessam carriers que não são o destinatário; o formato precisa ser autenticado,
versionado desde o primeiro byte e deixar espaço para migrar para MLS (spec 0.7, 7.1).

## Decisão

Serialização `postcard` (serde) de:

```text
Envelope { version: u8, id: [u8;32], conversation_id: [u8;32], author: [u8;32],
           author_device: NodeId, hlc: Hlc, parents: Vec<[u8;32]>,
           recipients: Vec<SealedKey>, kind: EnvelopeKind, ciphertext: Vec<u8>, sig: [u8;64] }
SealedKey { recipient: [u8;32], ephemeral_pk: [u8;32], sealed: Vec<u8> }
```

- `version` é o primeiro campo e primeiro byte do wire; receptor rejeita versões que não
  suporta antes de interpretar o restante.
- `id` = BLAKE3 do envelope serializado sem `id` e `sig`. `sig` = Ed25519 da identity key do
  autor sobre o mesmo conteúdo (domínio separado por prefixo constante, definido na Fase 2).
- Conteúdo cifrado com XChaCha20-Poly1305 usando uma chave de conteúdo aleatória por mensagem,
  selada por destinatário (X25519 efêmero-estático + HKDF-BLAKE3). Nonce de 192 bits aleatório.
- Validação ao receber: versão → `id` recalculado → assinatura → autor é contato/membro →
  dedup por `id`. Qualquer falha descarta e registra em `tracing`.
- Ordenação: topológica por `parents`, desempate por `hlc`, depois por `id`.
- `recipient` e o tamanho ficam visíveis ao carrier (metadado necessário ao roteamento).

## Consequências

- Sem forward secrecy no MVP (documentado em `threat-model.md`); migração para MLS ocorre por
  novo valor de `version`.
- Fan-out O(membros) por mensagem; aceitável para ≤ 20 membros.
- Todo tipo de wire vive em `proto`, com teste de round-trip e entrada em `docs/protocol.md`.
- Pontos que a Fase 2 deve fechar: prefixo de domínio da assinatura e o layout exato do HKDF.
