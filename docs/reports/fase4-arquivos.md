# Relatório da Fase 4 — arquivos

Data: 2026-10-22. Validação automatizada e local; o teste em máquinas reais está descrito em
`docs/roteiros/fase4-arquivos-tres-maquinas.md`.

## Critérios de aceite (spec 13)

| Critério | Como foi verificado | Resultado |
|---|---|---|
| Arquivo de **1 GB** baixado de **2 fontes simultâneas** | `one_gib_from_two_sources_with_interruption_and_resume` (`--release --ignored`): Alice envia 1 GiB a Bob e Carol (mesmo blob); Carol baixa de Alice; Bob baixa de Alice **e** Carol | ok |
| Interrupção e retomada | O mesmo teste corta o download do Bob com ~30 % servido e o retoma; também `an_interrupted_download_resumes_instead_of_starting_over` (64 MiB) e, no nível de blobs, `an_interrupted_download_resumes_without_refetching_what_arrived` | ok |
| Hash verificado | BLAKE3 de 1 GiB lido em blocos: origem = arquivo da Carol = arquivo do Bob; além do BLAKE3 por segmento do `iroh-blobs` e da autenticação por bloco do STREAM | ok |
| O budget limita a taxa servida | `the_bandwidth_budget_limits_the_served_rate` (3 MiB a 4000 kbps levam ≥ 4,5 s); `an_exhausted_monthly_budget_stops_seeding_but_the_origin_keeps_serving`; `a_leaf_serves_its_own_file_but_does_not_seed_what_it_downloaded` | ok |

### Números do teste de 1 GiB (localhost, build `--release`)

| Etapa | Resultado |
|---|---|
| Ciphertext (1 GiB + 28 B + 16 B/bloco de 64 KiB) | 1 074 003 996 bytes |
| Cifrar e importar 1 GiB | 2,0 s |
| Carol baixa de Alice (1 fonte) | 6,8 s |
| Bob: interrompido com | 335 MB servidos (31 %) |
| Bob: retomado e concluído em | 5,6 s |
| Bob recebeu de Alice / de Carol | 554 455 072 / 555 745 308 bytes (51,6 % / 51,7 % do ciphertext) |
| Total servido ao Bob | 1 110 200 380 bytes = 103,4 % do ciphertext (3,4 % de retrabalho dos segmentos em voo na interrupção) |

Os tempos são de loopback: **não** medem a internet. Servem para mostrar que as duas fontes
repartem a carga e que a retomada não recomeça do zero.

## O que foi construído

- `filecrypt`: STREAM sobre XChaCha20-Poly1305 (blocos de 64 KiB, nonce com contador e bit de
  "último"); 6 testes (todos os tamanhos de borda, truncamento em fronteira de bloco, blocos
  trocados/repetidos, chave errada, byte extra, `proptest`).
- `blobs`: segmentos de 8 MiB + `HashSeq`, importação em streaming, download multi-fonte em rodízio,
  exportação decifrada; 5 testes com 3 nós iroh e medição de bytes por provedor.
- Nó: serviço de blobs com política (só contatos mútuos; arquivo meu sempre; o resto por papel e
  budget; limite de banda por pedaço de 16 KiB; contabilidade), `send_file`/`send_file_to`,
  `files`, `download_file`, eventos `TransferProgress`, GC por tags, pré-replicação por carriers
  `Relay`/`SuperPeer`.
- CLI: `send-file`, `files`, `get` e `/enviar`, `/arquivos`, `/baixar` no chat.

## Testes

Total na suíte: 98 unitários + 10 (carry) + 8 (contatos) + 11 (arquivos, +1 `#[ignore]` de 1 GiB)
+ 6 (mensagens), `clippy -D warnings`, `fmt`, `cargo deny` e `cargo check` para
`aarch64-linux-android` verdes. Destaques dos 11 de arquivos: o conteúdo não aparece em claro em
nenhum arquivo do store; estranho com a raiz do blob não baixa nada; nomes `../../x` não saem da
pasta de destino; arquivo vazio, de 1 byte e de exatamente 64 KiB; limites de tamanho; carrier
`Relay` replica o blob e o arquivo **chega com o remetente totalmente offline**; `Peer` e blobs
acima de `prefetch_max_mb` não são replicados; a cópia vencida some e a GC apaga os dados.

## Achados durante a implementação

1. O `Downloader` do `iroh-blobs` só paraleliza entre os filhos de um `HashSeq`; um blob único é
   buscado de um provedor por vez. Por isso o arquivo é cortado em segmentos (ADR 0007).
2. Enviar o mesmo arquivo a duas pessoas em chamadas separadas gerava chaves e hashes diferentes
   (sem fonte em comum). `send_file_to` cifra uma vez e envia o mesmo `FileRef` a todos.
3. `Enxame::shutdown` não liberava o store de blobs (trava de arquivo do redb): um novo `start` no
   mesmo diretório ficava esperando. Corrigido.
4. 80 eventos de progresso por arquivo estouravam o canal de 64; agora no máximo ~100 por arquivo e
   os consumidores toleram `Lagged`.
5. O `iroh-blobs` 0.103.1 é pré-produção (aviso da própria documentação): uso isolado em
   `blobs.rs` e `node/files.rs`, versão fixada.

## Validação em campo (2026-10-22)

Máquinas: **André** (Linux, LAN), **bob** (Linux, mesma LAN/IP público que o André, papel `Relay`) e
**notebook** (Windows, hotspot 4G do celular, `10.61.x`, papel `Peer`). Arquivos aleatórios de 100 MB
(≈100 MB de ciphertext); orquestração pelo próprio chat (`scripts/field/orq.sh`).

| Rodada | Quem baixa | Fonte(s) | Tempo | Vel. | SHA-256 |
|---|---|---|---|---|---|
| 1 | notebook (4G) | André | 99,2 s | ~1,0 MB/s | igual |
| 1 | bob (LAN) | André | 17 s | ~5,8 MB/s | igual |
| 3 | bob | André | 17,7 s | ~5,6 MB/s | igual |
| 3 | notebook (4G) | André + bob | 101,9 s | ~0,95 MB/s | igual |

- **Multi-fonte confirmada na rede real:** na rodada 3 o bob serviu **50.332.338 bytes**, cerca de
  metade do arquivo (o `RoundRobin` divide os segmentos entre as duas fontes). O hash bateu.
- O tempo do notebook não melhorou com duas fontes: o gargalo é o próprio caminho dele (hotspot atrás
  de relay, ~1 MB/s), não as fontes.
- **Windows validado pela primeira vez** (`init`, pareamento, chat, envio e download). Rede móvel: o
  caminho ficou em `relay` (NAT sem furo direto em 20 s), RTT ~295 ms.
- **Pareamento é pré-requisito de fonte:** só contatos mútuos servem e são discados. Na rodada 2 o
  notebook não era contato do bob, então baixou só do André (0 bytes do bob). Só depois de parear
  notebook ↔ bob a multi-fonte aconteceu. Não é bug, mas a UI da Fase 6 precisa deixar claro quem é
  contato de quem.

### Achados de campo

1. O relay público do iroh limita a banda por endpoint (`The relay is rate-limiting this endpoint`).
   Por relay, 100 MB levam ~100 s: arquivos grandes dependem de caminho direto ou de relay próprio
   (opt-in, ADR 0008).
2. O chat fica bloqueado durante o `/baixar` e o progresso só aparece depois do "salvo em". A CLI
   não mostra de quais fontes veio (`baixando N de andre...`).
3. `history` e `contacts` sobem um nó completo com a mesma chave e derrubam o chat aberto no relay
   (`Another endpoint connected with the same endpoint id`); corrigido: `history` agora lê o banco sem subir o nó (`read_history`, com teste).
4. `/contribuicao` só atualiza quando a conexão de blobs fecha; o contador do serviço aparece com
   atraso.
5. O prompt de aceitação de contato (`serve`) recusa respostas com BOM/espaços (visto numa ponte no
   Windows com codepage 65001); corrigido: a resposta é normalizada (espaços, BOM e caixa), com teste.
6. O chat não aceita pedidos de contato (só o `serve`); para parear dois nós já online é preciso
   parar o chat.

## Limites desta validação

- Rodadas de retomada de download e de arquivo com remetente offline (passos 5 e 6 do roteiro)
  **não foram feitas** neste dia: a andrecasa não estava disponível.
- O `enxame-node` em Docker não foi reconstruído com o suporte a arquivos.
- Grupos (Fase 5) não têm arquivos ainda.
