# Roteiro de campo — Fase 3: carry entre três máquinas

Objetivo: **A escreve para B enquanto B está offline; C (contato dos dois) guarda a mensagem e a
entrega quando B volta; o recibo apaga a cópia em C.** Três pessoas, três redes.

| Papel no teste | Quem | Contatos que já tem |
|---|---|---|
| **A** (quem escreve) | andre | bob, andrecasa |
| **B** (quem fica offline) | bob | andre |
| **C** (o carrier) | andrecasa | andre |

B e C **ainda não são contatos**: o passo 1 resolve isso.

## Se o relay não conectar (ticket só com endereços 192.168.x.x, `diag` com `udp: false`)

Alguns roteadores e o MagicDNS do Tailscale descartam consultas DNS com EDNS, e o iroh não
alcança o relay. Use servidores DNS próprios, em qualquer comando:

```bash
ENXAME_DNS_SERVERS=1.1.1.1,8.8.8.8 ./enxame-cli chat andre
```
(PowerShell: `$env:ENXAME_DNS_SERVERS="1.1.1.1,8.8.8.8"`.) Ou fixe no `config.toml`:
`dns_servers = ["1.1.1.1", "8.8.8.8"]`.

## 0. Atualizar (as três máquinas)

Pare qualquer `serve`/`chat` aberto (Ctrl-C) e atualize:

```bash
cd enxame && git pull && cargo build --release -p enxame-cli && cp target/release/enxame-cli .
./enxame-cli contacts          # os contatos antigos devem continuar lá (o banco migra sozinho)
```
Windows: `.\enxame-cli.exe` e `Copy-Item target\release\enxame-cli.exe .`

> Depois de atualizar, o banco passa para a versão 2. **Não volte a abrir com o binário antigo.**

## 1. B e C viram contatos

1. **B**: `./enxame-cli serve` e copia a linha do ticket (`enxame1…`). Deixa aberto.
2. **C**: `./enxame-cli add '<ticket do B>'`.
3. **B** vê `Pedido de contato: andrecasa [fingerprint]`. Confirmem a fingerprint por fora
   (C: `./enxame-cli id`) e **B digita `s`** + Enter.
4. C deve ver `Contato mútuo com bob`. Confira com `./enxame-cli contacts` nas duas.
5. **B** para o `serve` (Ctrl-C).

## 2. Aquecimento: todos online por 1 minuto

Os contatos em comum são descobertos quando os pares sincronizam entre si; a sincronização
automática roda a cada 30 s. Então, **ao mesmo tempo, nas três máquinas**:

- **A**: `./enxame-cli chat bob`
- **B**: `./enxame-cli chat andre`
- **C**: `./enxame-cli --role peer chat andre`

(`--role peer` no C: um nó recém-aberto começa em `Leaf`, que não carrega, e só sobe depois de
2 min estável. Fixar `peer` evita a espera.)

Esperem **1 minuto**. No C, digite `/papel`: deve responder `Peer`.

Opcional: cada um manda uma mensagem curta (`oi`), só para ver que tudo conversa.

## 3. B sai

**B** digita `/sair`. (Daqui para a frente B está offline.)

## 4. A escreve para B (offline)

**A** digita no chat: `teste de carry 1`. O esperado é a linha:

```
(guardada; contato inacessível agora, segue depois)
```

Se **A** usar `./enxame-cli send bob "teste de carry 1"` (com o chat fechado), a saída esperada
é `Guardada; o contato está offline. 1 contato(s) em comum vão carregá-la até ele voltar.`

No **C**, digite `/contribuicao`: deve mostrar `guardando 1 mensagens de terceiros`.

> Se **A** vir `inacessível` sem a menção ao carrier, o C não aceitou. Anote o que o C respondeu a
> `/papel` e se o passo 2 durou 1 minuto com os três ao mesmo tempo.

## 5. A sai

**A** digita `/sair`. Agora só o C está online.

## 6. B volta

**B**: `./enxame-cli chat andre`. Em até ~30 s deve aparecer, **sem o A estar online**:

```
[hh:mm UTC] andre: teste de carry 1
```

## 7. O recibo apaga a cópia

No **C**, digite `/contribuicao` de novo: `guardando 0 mensagens de terceiros` e um valor de bytes
servidos maior que zero.

## 8. A volta e vê "entregue"

**A**: `./enxame-cli history bob`. A mensagem muda de `eu …` (pendente) para `eu ✓` (entregue):
o recibo do B chegou pelo C, sem o A ter falado com o B.

## O que me mandar

- Passou ou falhou cada passo (4, 6, 7, 8) e a saída das linhas esperadas.
- Em qualquer falha: a saída completa do comando, o resultado de `/papel` no C e o tipo de rede de
  cada máquina (fibra, 4G, Wi-Fi).

## Extras (se sobrar tempo)

- **Modo jogo no C**: no C, `/jogo on`; repita os passos 3–4. A deve ver `inacessível` **sem**
  carrier (C virou `Leaf`). Depois `/jogo off`.
- **Mensagens em ordem**: no passo 4, A manda 3 mensagens (`1`, `2`, `3`); no passo 6, B deve
  recebê-las na mesma ordem.
- **Duas rodadas**: repita o ciclo trocando os papéis (B escreve para A com A offline).
