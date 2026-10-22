# Roteiro de campo — Fase 4: arquivos entre três máquinas

Objetivo: validar com máquinas reais (1) envio e download de arquivo cifrado, (2) **download de
duas fontes ao mesmo tempo**, (3) **retomada** de um download interrompido e (4) arquivo que
chega com o remetente offline, por um carrier `Relay`.

| Papel | Quem | Observação |
|---|---|---|
| **A** (envia) | andre | origem do arquivo |
| **B** (baixa das duas fontes) | bob | |
| **C** (segunda fonte) | andrecasa | a mais distante: rode com `--role peer` |

Os três já são contatos mútuos entre si (Fase 3). Use arquivos de **~100–200 MB** em campo (o
teste de 1 GiB já foi feito em localhost) e coloque o arquivo de teste em `~/`.

## 0. Atualizar (as três máquinas)

```bash
cd enxame && git pull && cargo build --release -p enxame-cli && cp target/release/enxame-cli .
```
Se o relay não conectar (ticket só com endereços 192.168.x.x): `export ENXAME_DNS_SERVERS=1.1.1.1,8.8.8.8`
no mesmo terminal, antes dos comandos. Para sair do chat use `/sair`.

## 1. Todos online

- **A**: `./enxame-cli chat bob andrecasa`
- **B**: `./enxame-cli chat andre andrecasa`
- **C**: `./enxame-cli --role peer chat andre bob`

Esperem 1 minuto. (`--role peer` no C é necessário: só um nó `Peer` ou acima semeia o que baixou.)

## 2. A envia o MESMO arquivo para B e C

No chat do **A**: `/enviar ~/teste-150mb.bin bob andrecasa` (um só comando com os dois nomes: o
arquivo é cifrado **uma vez** e os dois recebem o mesmo blob, que é o que permite ter mais de uma
fonte). Também funciona fora do chat: `./enxame-cli send-file ~/teste-150mb.bin bob andrecasa`.
O progresso da cifragem aparece como `(↑ teste-150mb.bin: 40%)`. **Deixe o A online** depois disso,
pois é dele que os outros baixam.

## 3. C baixa de A (primeira fonte)

**C**: `/arquivos` mostra `1 ↓ teste-150mb.bin`. Digite `/baixar 1`. Esperado: progresso em
passos de 10 % e `(salvo em ...)`. Depois `/arquivos` mostra `✓`. A partir daqui o C **semeia**.

## 4. B baixa de A e C ao mesmo tempo

**B**: `/baixar 1 andre`. Esperado: o arquivo vem mais rápido que o de C se as duas rotas forem
boas. Para comprovar que as duas serviram, olhe no **A** e no **C** o comando `/contribuicao`
(C: bytes servidos > 0; o A não conta o próprio arquivo) e, se quiser, o tempo do download.

Confira a integridade nas três máquinas:

```bash
sha256sum ~/teste-150mb.bin                    # em A
sha256sum <pasta de downloads>/teste-150mb.bin # em B e em C (a pasta é impressa no "salvo em")
```
Os três hashes devem ser **iguais**.

## 5. Interrupção e retomada

1. No **B**, apague o arquivo baixado (`rm`) e rode de novo `./enxame-cli get andre 1` (com o chat
   fechado) em outro terminal; quando o progresso passar de ~30 %, aperte **Ctrl-C**.
2. Rode o mesmo `get` outra vez. Esperado: termina mais rápido e o `sha256sum` bate.
   (Como o blob já estava completo no passo 4, para um teste de retomada de verdade use um
   arquivo novo: repita o passo 2 com outro arquivo e interrompa o primeiro download dele.)

## 6. Arquivo com o remetente offline (carrier replica o blob)

1. **C** abre com `--role relay` (`./enxame-cli --role relay chat andre bob`): só `Relay` e
   `SuperPeer` aceitam mensagens de arquivo e replicam o blob.
2. **B** sai (`/sair`).
3. **A** envia: `./enxame-cli send-file ~/teste-50mb.bin bob` (use ≤ 50 MB: é o `prefetch_max_mb`).
   Esperado: `offline; 1 carrier(s) guardam o aviso`. **A fica online ~1 minuto** (o C busca o
   blob dele nesse tempo).
4. **A sai de vez** (`/sair`).
5. **B volta** (`./enxame-cli chat andre andrecasa`) e, em até ~30 s, vê `📎 teste-50mb.bin`.
   `/baixar 1 andre`: o arquivo vem **só do C**. Confira o `sha256sum`.

## O que me mandar

- Passou ou falhou em cada passo (3, 4, 5, 6) e, em falha, a saída completa.
- Os `sha256sum` dos três, e quanto tempo levou cada download (e o tamanho do arquivo).
- Tipo de rede de cada um.
