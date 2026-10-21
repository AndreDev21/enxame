# 0005 — Decisões da Fase 2 (mensagens 1:1)

Status: aceito

## Contexto

A Fase 2 entrega `store`, `crypto`, `sync` e `send`/`chat`/`history`. A spec fixa o envelope
(ADR 0003); aqui ficam as escolhas que ela deixou abertas.

## Decisões

1. **Versões das crates de cripto** (consultadas na crates.io em 2026-10-21): `chacha20poly1305`
   0.11, `hkdf` 0.13 + `sha2` 0.11, `blake3` 1.8, dalek 2. A spec lista só `hkdf`; ele precisa de
   um hash, então usamos SHA-256 (RustCrypto). Nonces e chaves aleatórias vêm do `getrandom` da
   própria `chacha20poly1305`.
2. **Selagem** conforme ADR 0003: X25519 efêmero-estático + HKDF-SHA256 (salt = efêmera ‖
   `enc_pk`) para cifrar a chave de conteúdo; assinatura sobre o `id`, com prefixos de domínio
   `enxame/*/1`. O cabeçalho (conversa, autor, HLC, tipo) entra como AAD do conteúdo, de modo que
   mover um ciphertext para outro cabeçalho falha mesmo com `id`/assinatura refeitos.
3. **Conversa 1:1 com id determinístico** (hash dos dois `identity_pk` ordenados), em vez de
   aleatório: os dois lados chegam ao mesmo id sem coordenação. Grupos (Fase 5) usam id aleatório
   do criador, como na spec.
4. **O remetente é destinatário de si mesmo** nas mensagens 1:1, para o histórico poder ser
   reconstruído a partir do par.
5. **Reconciliação**: a spec previa "faixa de HLC". Comparar só pontas e contagem se mostrou
   insuficiente (testes de propriedade acharam dois casos: ter a ponta sem os ancestrais, e dois
   lados cada um sem um dos pais de uma mesma ponta). O `Heads` ganhou um `digest` (BLAKE3 dos ids
   ordenados); se difere, pagina-se a lista de ids por faixa de HLC (cursor inclusivo, para não
   perder empates). Custo: 32 bytes por mensagem da conversa quando há divergência. RBSR continua
   pós-MVP e é o caminho para reduzir isso.
6. **Só se aceita o que foi pedido** (`Want`), com autor restrito ao par ou a mim e conversa
   restrita às compartilhadas.
7. **Relógio**: HLC retomado do maior carimbo do banco; `observe` ao receber. Envelope com HLC
   > 24 h no futuro é recusado para um par não envenenar o relógio.
8. **Ordem de exibição** = ordem topológica por `parents`, desempate por (HLC, id); pais ausentes
   não bloqueiam. Função pura com teste de propriedade.
9. **SQLite** (`rusqlite` bundled) com migrations por `PRAGMA user_version`, WAL. Mensagens vão por
   `spawn_blocking`; contatos (raros) escrevem direto. O texto fica **em claro** no banco local
   (sem SQLCipher no MVP).
10. **Contatos** saíram do `contacts.bin` para o banco; o arquivo da Fase 1 é importado uma vez e
    renomeado para `contacts.bin.migrated`.
11. **Sincronização automática** a cada 30 s por contato mútuo (`sync_interval_secs`, 0 desliga),
    com uma tarefa por contato para um par offline não atrasar os outros, e imediata ao enviar.
12. **Fora do escopo desta fase** (continuam como estão): recibos, carry, grupos e arquivos. Só
    `EnvelopeKind::Text` é aceito.

## Consequências

- O endereço de um contato vem do ticket e envelhece; sem relay/discovery (testes, LAN pura) quem
  reiniciou precisa ser o lado que disca. Com relay + DNS/pkarr isso se resolve.
- Sem forward secrecy (como na spec): vazamento da chave de encriptação expõe o histórico.
