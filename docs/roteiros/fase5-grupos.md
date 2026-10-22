# Roteiro de campo — Fase 5: grupos pequenos

Objetivo: validar com máquinas reais (1) criar grupo e distribuir o retrato, (2) mensagens de
vários autores, (3) membro offline que volta e se atualiza, (4) remoção: o removido não lê o que
vier depois, (5) o aviso de IP.

| Papel | Quem | Observação |
|---|---|---|
| **A** (criador) | andre | cria e mexe na lista |
| **B** | bob | `--role relay` (bom carrier) |
| **C** | jean | `--role peer` |
| **D** | notebook (Windows, 4G) | `--role peer`; o caminho mais difícil |

Todos já são contatos mútuos do André (e entre si, exceto onde indicado). Para exercitar o aviso
de IP é preciso um membro que **não** seja contato de outro: se todos já se conhecem, o aviso
aparece vazio (o caso com não-contatos está coberto nos testes automatizados).

## 0. Atualizar (todas as máquinas)

```bash
cd enxame && git pull && cargo build --release -p enxame-cli
```
No banco, a migração para a versão 5 é automática na primeira execução; depois dela não abra o banco
com um binário antigo.

## 1. Todos online

`./enxame-cli chat andre` (cada um com o seu papel acima; o André abre `chat bob jean notebook`).

## 2. Criar o grupo

**A**: `/grupo criar teste bob jean notebook`. Esperado: `(grupo #teste criado com 4 membros;
entregue direto a 3)`. Cada um vê `(você entrou no grupo #teste, criado por andre)` e, se houver
não-contatos, o `AVISO` de IP. `/grupos` lista o grupo e os membros.

## 3. Mensagens de vários autores

**A**: `#teste oi, grupo`. **B**: `#teste aqui é o bob`. **D**: `#teste do 4G`. Todos devem ver as três,
com o prefixo `#teste` e o autor certo; `✓` aparece quando todos receberam.

## 4. Membro offline

1. **D** sai (`/sair`). **A** manda `#teste escrevi com o notebook fora`. Esperado: `entregue a 2;
   offline: notebook; N carrier(s)`.
2. **D** volta (`--role peer chat andre`) e em até ~30 s a mensagem aparece sozinha.
   (`/grupo historico teste` mostra tudo.)

## 5. Remoção

**A**: `/grupo remover teste jean`. **C** vê `(você foi removido do grupo #teste)`. **A** e **B**
mandam mensagens: **C** não deve receber nenhuma (nem em `/grupo sync teste`) e `#teste ...` no
chat dele dá erro. B e D continuam recebendo.

## 6. Só o criador mexe na lista

**B**: `/grupo add teste jean` e `/grupo remover teste notebook` devem falhar com "só o criador…".

## O que me mandar

- Passou ou falhou em cada passo e, em falha, a saída completa.
- Tipo de rede de cada um. Para o passo 4, quantos carriers apareceram.
