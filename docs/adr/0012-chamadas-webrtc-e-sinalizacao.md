# ADR 0012 — Chamadas 1:1: WebRTC com sinalização E2E própria

Status: aceito (Fase 6, bloco 6.4). Spec: seção 13/Fase 6 e decisão aberta 5.

## Contexto

O Enxame precisa de chamadas de áudio, de vídeo e, principalmente, **transmissão de tela** entre dois
contatos, sem servidor de sinalização e sem criptografia escrita por nós. O `iroh` entrega dados
(QUIC) mas não tem mídia em tempo real; os relays públicos do iroh limitam a banda (~1 MB/s por
endpoint, visto em campo), então a mídia não deve passar por eles.

## Decisões

1. **Mídia: WebRTC (`flutter_webrtc` 1.6.2).** Traz DTLS-SRTP, controle de congestionamento, codecs
   e captura de tela (X11 e, em tese, PipeWire no Linux; nativa no Windows). Nenhuma primitiva de
   criptografia é nossa.
2. **Sinalização pelo Enxame, E2E:** envelope `kind = CallSignal` (assinado, selado só para o
   contato) num ALPN próprio, **`enxame/call/1`**, e não no `enxame/sync/1`. Motivo: o sync
   reconcilia conjuntos de mensagens persistentes; oferta/ICE são efêmeras e não devem entrar no
   histórico nem ser repetidas depois. Entrega só direta (sem fila nem carrier): se o contato está
   inacessível, a chamada falha na hora.
3. **Validação de um sinal (`calls::open_signal`):** envelope válido (id e assinatura), `kind =
   CallSignal`, autor = o contato de quem veio, conversa 1:1 certa, um destinatário e sem pais,
   **fresco** (carimbo a até ±24 h, como as mensagens; ver abaixo), carga de 1 a 64 KiB, **sem repetição** (memória dos 10 000 ids mais recentes) e
   no máximo **200 sinais por contato a cada 10 s**. Só contatos mútuos conectam (como no carry).
   Co-membros de grupo que não são contatos não podem ligar.
4. **Protocolo da chamada (JSON dentro do sinal, opaco para o núcleo):** `invite{id,kind}` →
   `accept` | `reject` | `busy` → `offer`/`answer` (SDP) e `ice` → `hangup`. `kind`: `audio`, `video`
   ou `screen` (só o chamador transmite a tela). **Nada é capturado antes de o outro lado aceitar**:
   câmera, microfone e tela só são abertos depois do `accept` e, para a tela, depois de o usuário
   escolher a fonte num seletor. Há indicador permanente de "você está transmitindo" e botão de
   parar. Uma chamada por vez (`busy`); tocar por 45 s, conectar por 30 s.
   **Inversão de papéis (2026-10-23):** se a primeira conexão não abre (ICE falhou ou prazo), quem
   ofereceu manda `swap` e quem atendeu passa a oferecer (`a`=1; sinais de outra tentativa são
   ignorados; microfone, câmera e tela são reaproveitados). Motivo: com CGNAT de rede móvel o resultado
   depende de quem abre primeiro (campo: Linux→Windows 2/2, Windows→Linux 0/2). Se a 2ª também
   falha, o app diz que não há caminho direto e sugere outra rede; TURN fica para a Fase 7.
   Compatível para trás: um app antigo ignora `swap` e a chamada acaba no prazo de 30 s.
5. **Compartilhar tela durante a chamada:** com câmera, troca a imagem (`replaceTrack`); só com áudio,
   acrescenta um vídeo e renegocia (educado/impolite para colisão de ofertas).
6. **Caminho de rede:** ICE com STUN público (Google e Cloudflare), **sem TURN** nesta fase. Entre NATs
   cone e na mesma LAN funciona direto; com NAT simétrico ou 4G/CGNAT a mídia pode não abrir. TURN
   próprio opcional fica na Fase 7 (decisão aberta 5). Os servidores STUN veem os IPs públicos
   (não o conteúdo).

## Consequências

- O sinal revela ao STUN e ao outro lado os endereços (ICE) — o contato já conhecia o IP; o STUN não.
- Calls em grupo e SFU ficam fora (Fase 7).
- Áudio do sistema durante a transmissão de tela não está implementado (o `flutter_webrtc` tem captura
  por loopback do PulseAudio no Linux; fica para depois).

## Validação

- Núcleo: testes unitários de `open_signal` (repetição, velho, futuro, de outro autor, adulterado, tipo
  errado, selado para outro, limite de taxa) e de rede (`tests/calls.rs`: entrega em ordem, sem
  histórico, estranho e contato offline falham).
- App: `integration_test/call_test.dart` roda dois `CallController` no mesmo processo trocando os sinais
  por JSON e a **tela real é capturada, codificada e decodificada** pelo outro lado; recusar, ocupado,
  cancelar e sinal de outra chamada não deixam nada pendurado. `app_test.dart` envia um sinal pela ponte
  e pelo núcleo até um nó de verdade.
- **Não verificado:** microfone e câmera reais (a máquina de teste não os usou), captura no Wayland
  (a sessão é X11), áudio de sistema, chamada entre duas máquinas reais, NAT simétrico/4G.

## Validação em campo (2026-10-23, internet real)

App em duas máquinas de redes e cidades diferentes (andrecasa, Wi-Fi com Tailscale; bob, LAN com
roteador), apps pareados, `flutter_webrtc` 1.6.2. Caminho de mídia: **direto via STUN
(`srflx` ↔ `srflx`), RTT 20–50 ms, sem relay nem TURN**.

| Teste | Resultado |
|---|---|
| Voz (andrecasa → bob) | conectou em 0,73 s após o aceite; áudio chegou ao bob (53/102 blocos com som); o André ouviu os bipes do bob (áudio nos dois sentidos); silenciar/desmutar funciona |
| Tela, LAN (André → bob) | conectou; tela legível; o "Conectando…" de ~18 s era o André escolhendo a tela no seletor, não rede |
| Tela, internet | conecta em 0,5 s após a oferta; voz junto com a tela; texto de terminal legível |

Achados e correções que o campo trouxe:

1. **A tela começava em 640x360 (texto borrado)** por o WebRTC medir a banda aos poucos e preferir
   fluidez. Corrigido com `degradationPreference = MAINTAIN_RESOLUTION`.
2. **O limite de bitrate padrão (~2,5 Mbit/s a 1440p) fazia o encoder descartar quadros** (21 fps de
   entrada → 4 fps codificados) com 5 Mbit/s livres. O `setParameters` do `RTCRtpSender` é **ignorado
   pelo plugin no Linux**; o que vale é o SDP: `boostVideoBitrate` acrescenta `b=AS` e
   `x-google-{min,start,max}-bitrate` na seção de vídeo do SDP local de cada lado. Bitrate foi de
   ~2 para 6,5–7 Mbit/s.
3. **A resolução se reduz no transceptor**, não depois: `addTransceiver(sendEncodings:
   scaleResolutionDownBy)`. A largura capturada vem do primeiro quadro do renderer local (o plugin
   não a informa em `getSettings()`); o teto é 1920 px (teste: teto 960 → enviado 960).
   A `constraints` `maxWidth/maxHeight` do `getDisplayMedia` são ignoradas pelo capturador do X11.
4. Chamada de tela agora leva voz nos dois sentidos (o bob notou que o mudo não fazia nada).
5. O outro lado vê "fulano está escolhendo o que transmitir…" e quem transmite vê "Você está
   transmitindo para fulano".

**Tela 1080p nativa (PC `andre`, X11, LAN com o bob):** enviado em 1920x1080 a **24–26 fps**
(máximo 26, média 24), alvo de 6,5–7 Mbit/s, sem limitação de banda nem de CPU, texto legível. É a
melhor rodada, mas **também não chega a 30**: o capturador do X11 satura em ~25 quadros por segundo
mesmo em 1080p.

**Fluidez (alvo de 30 fps): não atingido no X11.** Numa tela 2560x1440 o capturador
do X11 entrega ~21 quadros por segundo de entrada (o Xorg usa ~48% de CPU só para copiar a tela) e o
resto do caminho acompanha: enviado em 1920x1080 a ~21 fps estáveis, ~7 Mbit/s, sem limitação de
banda nem de CPU do encoder. Na tela de 1080p chega a 24–26 fps (e a 25–27 no teste local). Para chegar a 30 em telas grandes é preciso outra forma de captura (PipeWire/portal no
Wayland, ou capturar uma janela em vez da tela inteira); fica como trabalho futuro. Em redes lentas
os quadros caem antes da nitidez (decisão deliberada para texto).

**Ainda sem teste real:** câmera (nenhuma das duas máquinas tem), Wayland, Windows, NAT simétrico e
4G (sem TURN).

**Achados menores para depois:** o registro "par escolhido" sai duplicado quando há mais de um par com
bytes enviados (visto com o bob, que tem uma VPN `tun0`) e o candidato `prflx` aparece sem endereço
(só a porta); convém registrar o IP remoto e distinguir o par realmente selecionado.

### Correção após o teste no Windows (2026-10-23)

Com o instalador do Windows, a chamada falhou com "violação de protocolo: o contato recusou o sinal"
enquanto as mensagens funcionavam. A validação de frescor do sinal comparava o **HLC** do envelope
com o relógio de parede, mas o HLC é lógico e pode andar à frente (mensagens de pares com o relógio
adiantado são aceitas até 24 h e o elevam). Correções: o sinal usa carimbo de **relógio de parede**
(`Hlc::new(agora, 0)`) e o receptor não o incorpora ao HLC; a janela passou de ±60 s para ±5 min
(relógios de PC desacertados); e a recusa agora devolve o **motivo** (`CallMsg::Refused`), que o app
mostra ("relógio fora de hora: N s de diferença…"). A causa exata no caso do amigo não foi observada;
a mensagem nova a revela se não for esta.

**Segunda correção (a causa foi confirmada):** o PC Windows de teste tinha o **fuso/relógio ~3 h fora**
do ar; mesmo a janela de ±5 min recusaria o sinal. As mensagens já toleravam 24 h. A janela do sinal passou
a ser a mesma (±24 h), porque a proteção contra repetição vem do **id inédito** e do canal cifrado, não do
relógio; a memória de ids passou a ser limitada por tamanho (10 000, os mais antigos saem) em vez de por
tempo. Testes: relógio ±3 h e ±23 h passam; ±25 h é recusado; a memória não passa do limite.
