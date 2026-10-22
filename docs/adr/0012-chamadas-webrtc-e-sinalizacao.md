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
   **fresco** (HLC a até ±60 s), carga de 1 a 64 KiB, **sem repetição** (memória dos ids por ~2 min) e
   no máximo **200 sinais por contato a cada 10 s**. Só contatos mútuos conectam (como no carry).
   Co-membros de grupo que não são contatos não podem ligar.
4. **Protocolo da chamada (JSON dentro do sinal, opaco para o núcleo):** `invite{id,kind}` →
   `accept` | `reject` | `busy` → `offer`/`answer` (SDP) e `ice` → `hangup`. `kind`: `audio`, `video`
   ou `screen` (só o chamador transmite a tela). **Nada é capturado antes de o outro lado aceitar**:
   câmera, microfone e tela só são abertos depois do `accept` e, para a tela, depois de o usuário
   escolher a fonte num seletor. Há indicador permanente de "você está transmitindo" e botão de
   parar. Uma chamada por vez (`busy`); tocar por 45 s, conectar por 30 s.
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
