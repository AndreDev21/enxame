# 0007 — Decisões da Fase 4 (arquivos)

Status: aceito

## Contexto

A spec (seção 10) pede: arquivo cifrado com chave aleatória em streaming, ciphertext no
`iroh-blobs`, `FileRef` dentro do envelope E2E, download multi-fonte com verificação BLAKE3 e
retomada, semeadura automática de quem baixou, pré-replicação por `Relay`/`SuperPeer` e coleta de
lixo. O `iroh-blobs` 0.103.1 (fixado no ADR 0002) avisa que **ainda não é de qualidade de
produção**; o uso fica isolado em `blobs.rs` e `node/files.rs`, com testes de multi-fonte,
retomada, limite e negação de acesso.

## Decisões

1. **Formato de cifragem** (`filecrypt.rs`): construção **STREAM** (Hoang et al.) sobre
   XChaCha20-Poly1305, implementada por composição do AEAD do crate (o módulo `stream` do `aead`
   0.6 não existe mais). Blocos de 64 KiB; nonce = `prefixo aleatório (19) ‖ contador u32 BE ‖
   último (u8)`. O bit "último" impede truncar; o contador impede reordenar ou repetir blocos.
   Cabeçalho de 28 bytes (`"ENXF"`, versão, tamanho do bloco, prefixo). Testado contra: bit
   trocado, truncamento (inclusive em fronteira de bloco), byte extra, blocos trocados, bloco
   repetido, chave errada, arquivo vazio e todos os tamanhos de borda.
2. **Segmentação**: o ciphertext é cortado em segmentos de **8 MiB**, cada um um blob, e a lista de
   hashes vira um `HashSeq` cuja raiz é o `blob_hash` do `FileRef`. Motivo: o `Downloader` do
   `iroh-blobs` só paraleliza entre os filhos de um `HashSeq` (um blob único é buscado de um
   provedor por vez). Com segmentos, até 32 vão em paralelo e a verificação BLAKE3 e a retomada
   são por segmento.
3. **Escolha de provedores em rodízio** (`RoundRobin`), não aleatória: cada segmento começa por um
   provedor diferente, o que reparte a carga entre as fontes de forma previsível.
4. **`FileRef { blob_hash, size, name, mime, file_key }`** vai no envelope E2E (`kind = FileRef`);
   `size` é o tamanho em claro e o do ciphertext é derivado. O nome é só exibição: ao gravar em
   disco é saneado (sem separadores, controles nem ponto inicial) e nunca sobrescreve (`nome
   (1).ext`). Limite configurável (`max_file_mb`, 4096).
5. **Um arquivo, vários destinatários**: `send_file_to` cifra e importa **uma vez** e envia o mesmo
   `FileRef` a todos. Sem isso cada envio teria chave e hash próprios e não haveria "mesma fonte"
   para o multi-fonte.
6. **Quem pode baixar**: só contatos mútuos (`ClientConnected` do provedor é interceptado). O hash
   da raiz é uma capacidade (32 bytes) e o conteúdo é E2E; `get_many` e `push` ficam desligados.
7. **Política de servir** (spec 8.2/8.3): arquivo **meu** é servido sempre (enviar é uso normal, não
   contribuição, e um nó em modo jogo não pode travar o que o usuário acabou de mandar); arquivo
   baixado ou replicado só com papel `>= LeafPlus` e budget mensal não estourado (senão
   `Permission`/`RateLimited`). O limite de banda é o mesmo limitador de 30 % do upload estimado
   (ou `max_contrib_kbps`), aplicado a cada pedaço de 16 KiB pelo evento `Throttle`; os bytes de
   arquivos que não são meus entram na contribuição por contato e no teto mensal.
8. **Descoberta de fontes**: antes de baixar, sonda-se a conexão de blobs com **todos os contatos
   mútuos** (os offline caem em `dial_timeout`) e esses viram os provedores. Custo de privacidade:
   cada contato online vê que você procura aquele hash; só contatos recebem a sondagem. Os
   endereços dos contatos são registrados num `MemoryLookup` do endpoint porque o `Downloader`
   conecta só por `EndpointId`.
9. **Semeadura automática**: ao terminar, o blob ganha uma tag permanente e uma linha em `blobs`
   (origem `Downloaded`); a política de servir decide se ele é oferecido.
10. **Pré-replicação**: o carrier não consegue ler o `FileRef` (está cifrado), então quem **oferece**
    a mensagem anexa `BlobOffer { hash, enc_size }` na `Offer` de carry. `Relay`/`SuperPeer`
    (únicos que aceitam envelopes `FileRef`) buscam o blob do ofertante em segundo plano, se
    `enc_size <= prefetch_max_mb` e couber na quota; o download é cortado se passar do tamanho
    declarado. A cópia tem TTL do papel (7 ou 30 dias). O ofertante precisa ficar online até o
    carrier terminar de buscar.
11. **Coleta de lixo**: tudo que importa é protegido por tags `enx/<hash>`; o `iroh-blobs` apaga o
    que não tem tag (GC periódica). Cópias replicadas vencidas perdem a tag e a linha no
    `maintenance_loop`. Arquivos meus e baixados não expiram (a referência é a mensagem).
12. **Armazenamento**: `<dados>/blobs` (`FsStore`, redb) e `<dados>/downloads`. O plaintext só
    existe fora dos blobs (o arquivo original do remetente e a cópia decifrada do destinatário).

## Consequências

- O `iroh-blobs` cria um runtime tokio próprio para o store; aceitável no desktop e a observar no
  Android (o `cargo check` para `aarch64-linux-android` passa).
- Um arquivo ocupa ~1x no disco do remetente (ciphertext) e ~2x temporariamente no destinatário
  (ciphertext + cópia decifrada); o ciphertext continua sendo semeado.
- Sem recibos de arquivo: a mensagem `FileRef` tem recibo como qualquer envelope, mas não há
  confirmação de que o arquivo foi baixado.
- Fora do escopo: apagar arquivo/blobs localmente, arquivos em grupos (Fase 5), pausar download.
