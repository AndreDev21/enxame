# Fase 6.6 — Windows: relatório de campo (2026-10-23)

Máquinas: notebook Windows com webcam (`flutter run -d windows`, build Debug, HEAD indicado em cada
item) ↔ PC Linux (`flutter run -d linux`). Mesma LAN, par direto (`host`↔`prflx`, RTT 4–10 ms).
A coordenação foi pelo próprio chat do Enxame: um Claude no Windows executava os pedidos de outro no
Linux e devolvia logs (`docs/roteiros/windows-escuta.md`); ações humanas (clicar, falar) foram pedidas
ao André.

## O que passou

| Item | Resultado |
|---|---|
| Voz Linux↔Windows (nos dois sentidos, desde o início) | ok (após a correção 3) |
| Vídeo (webcam do Windows) | ok |
| Tela Linux→Windows (1920x1080, ~26 fps) e Windows→Linux (1366x768, 1–9 fps com tela parada) | ok |
| Tela dos dois lados ao mesmo tempo | ok |
| Tela no meio da chamada de voz (acrescenta vídeo e renegocia) e na de vídeo (troca de faixa), 3 vezes seguidas | ok, sem falha intermitente |
| 4 chamadas seguidas no mesmo processo | ok, sem queda |
| Bandeja: ícone, menu com o botão direito, Sair | ok (saída limpa, sem filho nem evento de falha) |

## Defeitos encontrados e corrigidos

1. **Menu da bandeja não abria** no Windows: o gatilho do menu vem desligado; `setContextMenuTrigger
   (rightClicked)` (`fd46338`). O ícone só existe depois do onboarding (a bandeja nasce com o nó).
2. **Queda ao escolher Sair** (`cnativeapi.dll`, violação de acesso): o clique mudava o estado, o menu
   era refeito e os itens nativos descartados com o clique em andamento. Agora o menu é refeito por
   timer e o Sair roda fora do callback (`8fcdb1f`). Os "processos extras" na saída eram o tratamento
   de falha do Windows, não uma 2ª instância. `exit` também passou a usar `TerminateProcess` no
   Windows (`8e4e334`).
3. **Atendente sem enviar o microfone até compartilhar a tela**: `addTransceiver` antes de aplicar a
   oferta deixa o transceptor órfão (mid novo); a resposta saía `recvonly`. O atendente agora aplica a
   oferta e liga as faixas aos transceptores dela (`a38c939`). Diagnóstico do Claude do Windows a
   partir do log nativo (`recv=1 send=0`, `AddSendStream` só na renegociação).
4. **Queda na 2ª/3ª chamada do processo** (`libwebrtc.dll` 0xc0000409; `ntdll.dll` 0xc0000374, heap):
   o plugin só solta o observador nativo da conexão em `dispose()` e só enquanto ela está no mapa
   (`close()` a remove). Agora `dispose()` antes de `close()` (`6721aba`). Antes caía sempre na 2ª ou
   3ª chamada; depois, 4 seguidas sem queda.
5. **Prévia da tela**: a câmera ficava no canto mesmo transmitindo a tela. Há uma prévia separada
   para a tela (`2d923d0`).

## Pendências do Windows

- Ao sair, "o nó não parou a tempo" (5 s): investigar o que demora no `state.stop()`.
- Falta validar o **instalador** (rc4) num PC sem ambiente de desenvolvimento: mensagens, arquivo,
  voz, vídeo, tela, bandeja, Sair e bateria; e o 4G/NAT simétrico (TURN).
- A origem do "estava intermitente" da tela com `replaceTrack` não ficou isolada: não reproduziu em 3
  rodadas depois das correções 3 e 4; mantenha os logs de troca de faixa (`3bd45a8`).
