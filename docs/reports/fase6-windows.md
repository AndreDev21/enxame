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

## Redes diferentes: notebook no 5G do celular ↔ PC Linux em casa (2026-10-23, à noite)

- Mensagens e arquivo de ~5 MB entre os apps: ok (download lento, esperado pelo relay/5G).
- **Voz, Linux liga:** conectou em ~4 s, par `srflx`↔`srflx` (direto via STUN), RTT 35–70 ms (2/2).
- **Windows liga (voz e vídeo):** a primeira conexão **não abria** (0/2; ICE falhava em 21–27 s, as
  checagens STUN do Windows expiravam e nenhuma do Linux chegava). Com o Windows como quem atende, ok.
  O resultado depende de quem oferece primeiro com o CGNAT do 5G; só STUN, sem TURN.
- **Correção (ADR 0012, `2e164c3` e `177edc6`):** se a primeira tentativa não conecta em 10 s, os
  papéis se invertem (`swap`); o app lembra por contato e por direção quem deve oferecer
  (`call_roles.json`). Medido: 1ª chamada 13,6 s (com a inversão), as 3 seguintes 3,8–4,0 s sem
  inversão. Se as duas tentativas falham: mensagem de que não há caminho direto (rede móvel ou
  roteador restritivo), em ~35 s.
- **Limite conhecido:** dois aparelhos atrás de CGNAT (por exemplo, dois celulares em 5G) não abrem
  caminho direto; só TURN resolve (Fase 7, decisão aberta 5 da spec).
- **Webcam do Linux sem enviar nada** (vídeo 0 bytes): o libwebrtc embutido não decodifica MJPEG
  (`Failed to convert capture frame from type 11 to I420`) e a câmera USB só dá 30 fps nesse formato.
  No Linux a câmera passa a pedir 640x480 a 15–24 fps (YUYV) (`96a33a3`). Teste de integração com
  câmera real: `ENXAME_TEST_CAMERA=1 flutter test integration_test/call_test.dart -d linux`.
  Obs.: a webcam USB usada travou depois de várias aberturas seguidas nos testes (voltou ao
  desplugar e plugar); não reproduzido no uso normal do app.
- Resultado final do teste H (vídeo nos dois sentidos, 5G): ok; a webcam do Linux chega ao Windows.

## Por que só uma direção abre no 5G: matriz de NAT (2026-10-23, noite)

Sonda de UDP cru entre os dois endpoints públicos (`scripts/field/natprobe.py` e `.ps1`, sem Python
no Windows). O endereço do par é lido de um arquivo a cada rodada (o CGNAT dá outra porta a cada
sessão) e o mapeamento é renovado enquanto se espera (o NAT móvel esquece UDP parado em ~30 s).

- Mapeamento: **cone** nos dois lados (a mesma porta pública para 3 a 4 servidores STUN).
- Quem sai primeiro, UDP cru, 14 a 24 s de pacotes:
  - os dois ao mesmo tempo: **nada passa** (3 rodadas);
  - o Linux 8 s antes do Windows: **nada passa**, nem depois do Windows começar;
  - o Windows (5G) 8 s antes do Linux: **passa nos dois sentidos** (60/63 e 63/101).
- Leitura: se um pacote de fora chega ao CGNAT antes de o notebook ter mandado algo para aquele
  endereço, o fluxo fica bloqueado por um bom tempo. Só abre se o lado do CGNAT sai primeiro.
- No ICE isso vira "quem atende manda as primeiras checagens": com o Linux oferecendo, o Windows
  (atendente) sai antes e conecta; com o Windows oferecendo, o Linux (atendente) sai antes e o fluxo
  morre. Por isso a inversão de papéis resolve e por isso não dá para acertar a ordem na primeira
  tentativa sem saber qual lado está atrás do CGNAT (atrasar o atendente conserta um sentido e
  quebra o outro).
- Sem TURN, o app faz: 10 s na ordem padrão → inverte → lembra por contato (ADR 0012). A 1ª chamada
  de cada par de redes custa ~14 s, as seguintes ~4 s. Dois aparelhos atrás de CGNAT restritivo
  continuam sem caminho direto.

Defeito encontrado e corrigido no caminho: ao inverter, **reaproveitar o microfone** do pc fechado
derrubou o app do Windows (`libwebrtc.dll` 0xc0000005, ~1 s após conectar). A inversão agora libera
e captura a mídia de novo (`8f6e0dd`); validado: swap, 66 s conectado, sem queda.
