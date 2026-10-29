# Plano do bloco 7.2 — mídia sobre o iroh (protótipo)

Base: ADR 0015, `docs/roadmap-fase7-conformidade.md` (7.2) e `docs/estudo-conectividade-sem-servidor.md` (3.3).
Objetivo do bloco: provar que voz e vídeo atravessam **dois CGNAT** pelo iroh, sem servidor nosso e sem
configuração, antes de investir em relay de amigo (7.4) e pool (7.5).

## APIs verificadas (não assumidas)

- `turn` 0.17.2 (webrtc-rs, MIT/Apache-2.0): `ServerConfig { conn_configs, realm, auth_handler, … }`;
  `ConnConfig { conn, relay_addr_generator }`; `trait RelayAddressGenerator { validate(); async
  allocate_conn(use_ipv4, requested_port) -> (Arc<dyn Conn + Send + Sync>, SocketAddr) }`. O `Conn` é o de
  `webrtc-util` (0.17.2): `recv_from`, `send_to`, `local_addr`, `close`, `as_any` (+ `connect`, `recv`, `send`).
  O endereço devolvido é o que o TURN anuncia como `XOR-RELAYED-ADDRESS`; ele **não precisa ser roteável**.
- iroh 1.4.1 (`Connection`): `send_datagram(Bytes)` (não confiável, pode descartar datagramas antigos para
  caber os novos), `send_datagram_wait`, `send_many_datagrams`, `read_datagram()`, `max_datagram_size()`
  (`Option`; "pouco mais de 1 KB" no mínimo), `datagram_send_buffer_space()`, `path_events()`/`paths()`;
  `QuicTransportConfig`: `datagram_receive_buffer_size`, `datagram_send_buffer_size`.

## Arquitetura

```
libwebrtc ──UDP── 127.0.0.1:P (TURN local, crate turn) ── Conn próprio ──datagrama QUIC iroh──► par
                       iceServers = turn:127.0.0.1:P (usuário/senha aleatórios por chamada)
```

1. **Endereços virtuais determinísticos:** o `RelayAddressGenerator` devolve `198.18.0.1:40000` (quem
   chamou) ou `198.18.0.2:40000` (quem atendeu), faixa de testes RFC 2544. Cada lado já conhece o endereço do
   outro. O candidato `relay` de cada lado, trocado pela sinalização existente, aponta para o endereço
   virtual do outro; quando o libwebrtc envia para ele pelo seu TURN, o `Conn` repassa ao par e o TURN do
   par entrega ao libwebrtc dele.
2. **Só o par:** `send_to` para qualquer destino que não seja o endereço virtual do par é descartado (pares
   ICE do candidato relay para candidatos reais não servem); só IPv4 (`use_ipv4 = false` → erro).
3. **Conexão de mídia:** ALPN `enxame/media/1`, uma conexão por chamada, só entre contatos mútuos (mesma
   regra de `handle_call`). O chamador disca assim que o atendente aceita; o atendente aceita a conexão.
   Quadro: `[versão u8][carga]`, carga = o datagrama UDP do TURN local sem alteração. Wire entra em
   `docs/protocol.md` com teste de round-trip em `proto` (regra do projeto).
4. **Política:** o servidor TURN local entra em `iceServers` da 1ª tentativa; o ICE prefere candidatos
   diretos (prioridade maior) e usa o relay enquanto o direto não abre. Falha em montar a ponte não impede a
   chamada (volta ao comportamento atual). Quando `path_events` indicar relay do n0, o app limita o envio a
   áudio (D4 degrau 6); vídeo e tela só em caminho direto ou relay de amigo.
5. **API:** `Enxame::media_bridge_start(contact) -> MediaBridge { url, username, credential }` e
   `media_bridge_stop()`; ponte em `enxame-ffi` (`start_media_bridge`/`stop_media_bridge`); no app,
   `CallController` acrescenta o servidor a `iceServers` antes de `createPeerConnection`.
6. **Segurança:** escuta só `127.0.0.1`, porta efêmera, credenciais aleatórias de 128 bits por chamada,
   encerrada com a chamada. A senha do TURN é MD5/HMAC-SHA1 (exigência do protocolo): exceção registrada no ADR 0015.

## Etapas (cada uma com critério de saída)

| # | Etapa | Saída |
|---|---|---|
| 1 ✅ | **Spike do libwebrtc (feito em 2026-10-26, ver abaixo):** `turn` em `127.0.0.1` com `RelayAddressGenerator` que faz eco entre dois peers no mesmo processo (sem iroh) | `integration_test` Linux: duas conexões WebRTC conectam **só** por candidatos `relay` (filtrando host/srflx) e trocam áudio. Responde a incógnita "o libwebrtc aceita TURN em loopback?" |
| 2 ✅ | **Ponte sobre o iroh (feita em 2026-10-26, ver abaixo):** ALPN `enxame/media/1`, `Conn` com datagramas, endereços virtuais | Teste em `crates/enxame-core/tests/media.rs`: dois nós in-process, pacotes UDP de 1200 B ida e volta; `proptest` do quadro; contagem de descartes |
| 3 ✅ | **MTU:** medir `max_datagram_size` por caminho (direto, relay local do netsim); se o RTP não couber, fragmentar com `[idx][total]` | Tabela de tamanhos no relatório; nenhum pacote descartado por tamanho |
| 4 ✅ (sem teste com mídia real) | **Integração no app:** FFI, `iceServers`, política da 1ª tentativa, mensagens e métricas (`ladder: relay`, caminho do iroh) | `call_test` com os dois controladores usando a ponte: conecta com `dropAllIce` nos candidatos diretos |
| 5 | **Campo com dois CGNAT:** Windows no hotspot ↔ outro celular/operadora, ou netsim simétrico | Voz e vídeo sem configuração; medições do 7.1 (RTT, perda, trocas de caminho) |
| 6 | **Medição contra `iroh-relay` próprio no Brasil** (VM barata), não contra o n0, para isolar arquitetura de limite e distância | Latência adicional < 80 ms; sem corte de áudio na subida relay → direto |

## Riscos e planos B

- **libwebrtc recusa ou ignora TURN em loopback:** plano B é reescrever os candidatos ICE para um encaminhador
  UDP local (estudo 3.3); decide-se na etapa 1, antes de qualquer código no núcleo.
- **Interação dos controles de congestionamento (QUIC sobre WebRTC):** medir na etapa 5; se o QUIC estrangular o
  vídeo, ajustar `datagram_send_buffer_size` e limitar o bitrate de vídeo quando o caminho for relay.
- **Dependência `turn`/`ring` no Android:** rodar `cargo check -p enxame-core --target aarch64-linux-android`
  já na etapa 2.
- **Sem dois CGNAT para testar:** usar o `scripts/netsim/netsim.sh` (NAT simulado) para o `symmetric`; o campo
  real fica na etapa 5 e depende de uma segunda operadora.

## Fora deste bloco

Relay de amigo no `enxame-node` e no desktop (7.4), protocolo `enxame/relay/1` e pool (7.5), n0 como último
degrau (7.6), incentivo (7.7) e chamada protegida (7.8).

## Resultado da etapa 1 (spike, 2026-10-26)

Código: `scripts/spike/turn-loopback` (crate fora do workspace, `turn` 0.17.2) e
`app/integration_test/turn_loopback_test.dart` (`flutter test integration_test/turn_loopback_test.dart -d linux`).
Dois servidores TURN em `127.0.0.1` cujo relay entrega os pacotes de um lado ao outro em memória, com os
endereços virtuais `198.18.0.1:40000` e `198.18.0.2:40000`; duas conexões WebRTC (`flutter_webrtc`, libwebrtc).

- **O libwebrtc aceita um servidor TURN em loopback** e anuncia o endereço virtual como candidato `relay`.
  Com `iceTransportPolicy: relay` (nenhum caminho real permitido) as duas conexões abrem o canal de dados em
  cerca de 1 s e trocam mensagem; o par nomeado é `relay/relay`. A incógnita principal do 7.2 está resolvida
  a favor da abordagem; o plano B (reescrever candidatos ICE) fica de reserva.
- **O libwebrtc pede uma alocação por interface de rede** (12 por lado nesta máquina, com as pontes do Docker). Com
  todas aceitas, todas anunciam o mesmo endereço virtual e a entrega fica ambígua. Recusar as extras
  (primeira alocação vale, as seguintes dão erro) deixa **um** candidato `relay` por lado e a conexão continua
  abrindo. Decisão para a etapa 2: uma alocação por chamada.
- **Com candidatos diretos disponíveis** (`iceTransportPolicy: all`) o par nomeado termina `host/host`; o relay
  fica só como reserva, como a escada de conexão pede.
- **Ainda não testado:** mídia de verdade (áudio/vídeo) pelo relay, só o canal de dados (mesmo transporte
  DTLS); troca relay → direto no meio de uma chamada; MTU (o spike entrega em memória, sem limite de datagrama);
  o `Conn` sobre iroh (etapa 2).

## Resultado da etapa 2 (ponte sobre o iroh, 2026-10-26)

Código: `crates/enxame-core/src/media.rs` (endereços virtuais, `Conn`, autenticação, contadores),
`src/node/media.rs` (`Enxame::media_bridge_start(contato, BridgeRole)` → `MediaBridge { url, username, credential }`,
`stats()`, `is_linked()`, `is_direct()`, `max_datagram_size()`, `close()`; aceite do ALPN), `proto.rs`
(`ALPN_MEDIA`, quadro com versão e teste de round-trip com `proptest`), `docs/protocol.md`.
Teste: `cargo test -p enxame-core --test media` (nós in-process, **clientes TURN reais** do crate `turn`).

- Dois nós amigos, ponte ligada (atendente sobe primeiro, chamador disca); cada cliente TURN aloca no seu TURN
  local, recebe o endereço virtual (`198.18.0.1:40000` / `198.18.0.2:40000`) e troca 50 pacotes de 900 B em cada
  sentido com conteúdo conferido: ≥ 45 de 50 chegam (UDP), sem descartes por "ponte não pronta".
- Só a primeira alocação é concedida; senha errada e contato inexistente/não mútuo são recusados; sem atendente
  esperando a ponte do chamador não liga (e ele tenta de novo dentro de 20 s se o par fechar na hora).
- `max_datagram_size` em loopback: 1414 B desde o início (descoberta de MTU não atrasou). **Não vale para internet
  nem para relay**: a etapa 3 mede por caminho; o RTP do libwebrtc tem até ~1200 B + SRTP + 1 B de quadro.
- Guarda-corpo Android: `cargo check -p enxame-core --target aarch64-linux-android` passa com `turn`/`ring`.
- `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --check` limpos.
- Não feito aqui: `cargo deny` (não está instalado nesta máquina; rodar na CI/antes de fechar o 7.2: `turn` traz
  `ring`, `md-5`, `webrtc-util`); mídia real do libwebrtc passando pela ponte (etapa 4).

## Etapa 3 — MTU por caminho (2026-10-29)

- Teste `relay_only_path_carries_an_srtp_sized_packet` (relay local do iroh, sem transporte IP): `max_datagram_size`
  = 1414 B e pacotes de 200, 900, 1200, 1232 e 1300 B chegam 10/10, sem descarte por tamanho. No caminho direto
  em loopback também são 1414 B, estáveis por 4 s de tráfego.
- Limite do que foi medido: é loopback/relay local. Em redes reais a descoberta de MTU do QUIC pode ficar perto do
  piso (1200 B, ~1160 B úteis), abaixo dos ~1232 B de um RTP cheio com SRTP. **Fragmentação (`[idx][total]`) não
  foi implementada**: sem dado real não há como saber se é preciso. O contador `dropped_too_large` da ponte
  entra nas métricas da etapa 4; se aparecer no campo (etapa 5), implementa-se a fragmentação ou reduz-se o MTU
  do libwebrtc.
- Não medido: relay público do n0 (limite de datagrama pode ser menor) — fica para a etapa 6.

## Etapa 4 — integração no app (2026-10-29)

- FFI (`crates/enxame-ffi/src/api/app.rs`): `start_media_bridge(contact, caller)`, `stop_media_bridge()`,
  `media_bridge_stats()`; uma ponte por vez, a anterior é fechada ao subir outra.
- `CallController` (`MediaBridgeProvider`, injetável; `null` nos testes): **uma ponte por conexão WebRTC** — sobe em
  `_newPeerConnection` (chamador disca; atendente registra antes de responder) e entra em `iceServers` ao lado do
  STUN; cai em `_dropConnection` (troca de papéis) e `_end`. Falha ao subir só é registrada no log e a chamada
  segue como antes.
- Escada: relay do ICE + ponte vira `irohDirect` ou `irohRelay` no registro de medição, com o bloco `bridge`
  (MTU de datagrama, pacotes, `dropped_too_large`). Relay público do iroh (`direct == false`) em chamada de
  vídeo/tela: câmera pausada, `startShare` recusado e `audioOnlyPath = true` (a interface ainda não mostra aviso).
- Testes: `call_test` com ponte de mentira (sobe nos dois lados com os papéis certos, derruba no fim, medição) e com
  ponte indisponível (conecta como antes); unitário da escada. **Não há teste in-process com TURN real**: o FFI tem
  um único nó global, então dois controladores no mesmo processo não têm duas pontes; a prova com cliente TURN
  real entre dois nós está em `tests/media.rs`, e a de libwebrtc + ponte é o campo (etapa 5).
- Pendente: aviso de "só áudio" na tela de chamada; validar em campo que o ICE escolhe a ponte quando o direto
  não abre e que o direto vence quando abre.
