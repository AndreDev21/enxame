# Passagem de bastão — Fase 7, blocos 7.0 a 7.2 (2026-10-29)

Para quem continua. Contexto de produto e decisões: `docs/adr/0015-conectividade-de-midia-sem-servidor.md`,
`docs/roadmap-fase7-conformidade.md`, `docs/estudo-conectividade-sem-servidor.md`. Plano e resultados do 7.2 etapa
a etapa: `docs/plano-fase7-2-midia-sobre-iroh.md`. Estado geral: `docs/STATUS.md`.

## Em uma frase

Chamadas WebRTC só conectavam quando havia caminho UDP direto (falhavam com dois CGNAT). O 7.2 testa a ideia de
fazer a mídia viajar pelo **iroh** (que já fura NAT e tem relay): um TURN local em `127.0.0.1` cujo "relay" é uma
conexão QUIC do iroh com o contato. **Está implementado de ponta a ponta, mas nunca rodou com mídia real entre
duas máquinas.** Essa é a primeira coisa a testar.

## O que foi feito

| Bloco | Resultado | Onde |
|---|---|---|
| 7.0 | Prazo da 1ª tentativa 10→7 s; mensagens de falha por causa; detecção de CGNAT (STUN próprio, 100.64/10, NAT simétrico, sub-redes de hotspot); preferência por IPv6; quem está atrás de CGNAT oferece primeiro; memória de papéis por contato | `app/lib/src/calls.dart`, `netprobe.dart`; ADR 0012 |
| 7.1 | Medição local de cada conexão (degrau da escada, RTT, perda, jitter, trocas de caminho), sem IPs; exportação anônima; relatório | `callmetrics.dart`, `docs/metricas-chamadas.md`, `scripts/field/metrics_report.py`, `docs/reports/fase7-medicao-1.md` |
| 7.2 etapa 1 | Spike: o libwebrtc aceita TURN em loopback e conecta só por `relay/relay` | `scripts/spike/turn-loopback/`, `app/integration_test/turn_loopback_test.dart` |
| 7.2 etapa 2 | Ponte no núcleo: ALPN `enxame/media/1`, `BridgeConn`/`BridgeGen` (crate `turn` 0.17.2), endereços virtuais `198.18.0.1/2:40000`, só contatos mútuos | `crates/enxame-core/src/media.rs`, `src/node/media.rs`, `proto.rs`, `docs/protocol.md`; `tests/media.rs` |
| 7.2 etapa 3 | MTU: 1414 B no direto (loopback) e no relay local do iroh; pacote de 1232 B passa 10/10 | `tests/media.rs::relay_only_path_carries_an_srtp_sized_packet` |
| 7.2 etapa 4 | FFI `start_media_bridge`/`stop_media_bridge`/`media_bridge_stats`; `CallController` sobe **uma ponte por conexão WebRTC** e a põe em `iceServers`; degraus `irohDirect`/`irohRelay` na medição; vídeo/tela pausados no relay público do iroh | `crates/enxame-ffi/src/api/app.rs`, `calls.dart` (`MediaBridgeProvider`), `state.dart` (`_CoreBridge`) |

Como funciona o encaixe no app: o atendente sobe a ponte antes de responder `accept` (ele só espera ser discado); o
chamador sobe a dele ao criar a conexão e disca com insistência por 20 s. A cada troca de papéis (nova tentativa) a
ponte é derrubada e sobe outra, porque o TURN local só concede **uma** alocação (o libwebrtc pede uma por interface
de rede; as demais recebem erro de propósito). Se a ponte não sobe, a chamada segue como antes.

## O que precisa ser testado (em ordem)

1. **Mídia real pela ponte (bloqueante).** Linux ↔ Windows (contatos `linux-teste` ↔ `win-teste`, ver
   `docs/roteiros/windows-escuta.md` e `scripts/field/orq.sh`). Nenhum teste automático cobre libwebrtc + ponte
   real, porque o FFI guarda um único nó global (dois controladores no mesmo processo não têm duas pontes).
   - **Falta uma forma de forçar a ponte.** Sugestão: variável `ENXAME_ICE_POLICY=relay` lida pelo
     `CallController` (`iceTransportPolicy: 'relay'` no `createPeerConnection`), só para teste. Sem isso o ICE
     escolhe o caminho direto quando ele abre e a ponte não é exercitada.
   - Verificar no `calls.log`: "ponte de mídia pronta", par escolhido `relay/relay`, e no registro de medição
     `ladder: irohDirect` (ou `irohRelay`) com `bridge.sent/received` crescendo dos dois lados.
2. **Direto vence quando abre.** Com a política `all` e rede que permite UDP direto, o par escolhido deve ser
   `host`/`srflx`, e a ponte só ociosa. Conferir que não há troca de caminho audível.
3. **Dois CGNAT de verdade.** Windows no hotspot de uma operadora ↔ celular/operadora diferente, ou `netsim`
   simétrico (`scripts/netsim/`). O usuário só tinha hotspot da mesma operadora até aqui: a matriz do 7.1 está
   **parcial** e a regra de decisão do 7.5 continua sem resposta.
4. **Troca relay → direto no meio da chamada** (sem corte de áudio) e **direto → relay** (queda de Wi-Fi).
5. **MTU em rede real e no relay público do n0.** `bridge.max_datagram` e `bridge.dropped_too_large` nas medições. Se
   `dropped_too_large` > 0, implementar fragmentação `[idx][total]` (não feita de propósito, sem dado) ou reduzir o
   tamanho de pacote do libwebrtc. O limite do relay do n0 não foi medido.
6. **Latência adicional** contra um `iroh-relay` próprio no Brasil (etapa 6; meta < 80 ms).
7. **Regressão:** as chamadas na mesma Wi-Fi continuam conectando em < 1 s sem inversão de papéis; chamada com
   ponte indisponível conecta como antes.

## Pendências conhecidas

- Aviso na tela de chamada quando o caminho é o relay público do iroh ("só áudio"): `audioOnlyPath` existe no
  controlador, a interface não o mostra (precisa de texto em `app_pt.arb`/`app_en.arb`).
- `cargo deny check` não foi executado (não instalado na máquina de desenvolvimento): conferir `turn` (traz `ring`,
  `md-5`, `webrtc-util`). A senha MD5/HMAC-SHA1 do TURN é exceção documentada no ADR 0015 (só loopback).
- IPv6 nunca foi exercitado em campo (a máquina de desenvolvimento não tem IPv6 global). A ponte é só IPv4.
- ADRs 0016 e 0017 (relay de amigo, pool) ainda não abertos. NAT-PMP/PCP não medidos (só UPnP por SSDP).
- `call_test.dart` inteiro: os 3 últimos testes (STUN real / largura) já falhavam antes do 7.x; rode os outros com
  `--plain-name`. Não é regressão.
- CI: job `app` no `ci.yml`, validação do instalador rc4 em PC limpo, Wayland, assinatura, primeira release (ver STATUS).

## Como rodar

```bash
cargo test -p enxame-core --test media                 # ponte: 4 testes (clientes TURN reais, relay local)
cargo test -p enxame-core --test media -- --ignored --nocapture   # exploração de MTU
cd app && flutter test && flutter analyze
flutter test integration_test/call_test.dart -d linux --plain-name "ponte"   # ponte de mentira
scripts/app/gen-bridge.sh                              # após mudar crates/enxame-ffi/src/api
ENXAME_DATA_DIR=$HOME/enxame-linux-dados flutter run -d linux     # app como linux-teste
python3 scripts/field/metrics_report.py <call_metrics.jsonl ou exportação>
```

## Armadilhas já pagas

- Um processo por identidade/diretório de dados: o CLI `id` com o app aberto trava. `pkill -f` com padrão que
  aparece no próprio comando mata o shell; use `pkill -x enxame_app`.
- Linux usa `ENXAME_DATA_DIR=$HOME/enxame-linux-dados` (contato `win-teste`); Windows usa `C:\dev\enxame-dados`.
  Identidade errada = "contato não encontrado".
- O libwebrtc só informa o endereço remoto `prflx` vazio: a LAN é inferida por RTT < 30 ms (`callmetrics.dart`).
- ICE `Failed` logo após `Checking` não é falha: `_failedGrace` de 3 s antes de contar.
- CGNAT do tipo cone não é detectável por STUN; a inversão de papéis é a rede de segurança.
- Não use xdotool na tela do usuário; use `integration_test`.
