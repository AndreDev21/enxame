# Medição de chamadas (Fase 7.1)

O app grava **um registro local por conexão de chamada** em `<dados>/call_metrics.jsonl`. Serve para
medir quanto das conexões fecha em cada degrau da escada (roadmap da Fase 7, D4) e decidir a
prioridade do 7.5. **Nada sai do computador sozinho**: em Diagnóstico → "Medição de chamadas" o
usuário pode exportar `call_metrics_export.json` e decidir se o envia.

Privacidade: sem IPs, sem fingerprints/EndpointIds, sem nome de contato, sem texto livre. A hora é
arredondada para a hora cheia. A exportação deixa passar só os campos abaixo.

| Campo | Valores | Observação |
|---|---|---|
| `v`, `hour`, `os` | 1, `2026-10-26T15`, `linux`/`windows` | |
| `kind`, `role` | `audio`/`video`/`screen`, `caller`/`callee` | |
| `outcome` | `connected`, `failed`, `cancelled` | só entra quem chegou a tentar conectar; `cancelled` = o usuário desligou antes |
| `attempts`, `swapped` | 1–2, bool | houve inversão de papéis |
| `ms_to_connect`, `seconds` | ms desde o aceite até conectar; duração | |
| `fail` | `timeout`, `ice_failed`, `remote_hangup`, `negotiation`, `signaling`, `other` | só em `failed` |
| `ladder` | `lan`, `ipv6`, `ipv4Direct`, `irohDirect`, `irohRelay`, `relay`, `none` | degrau que carregou a mídia (`none` = nenhum). `irohDirect`/`irohRelay`: a mídia foi pela ponte `enxame/media/1` (7.2), com o iroh em caminho direto ou no relay público |
| `bridge` | `iroh_direct`, `max_datagram`, `sent`, `received`, `dropped_too_large`, `dropped_other` | só em chamadas com ponte; contadores da ponte e MTU de datagrama |
| `local_type`, `remote_type`, `family` | `host`/`srflx`/`prflx`/`relay`, `v4`/`v6` | tipos de candidato do par escolhido |
| `path_changes` | n | trocas de par durante a chamada |
| `rtt_ms_avg/max`, `loss_pct`, `jitter_ms` | | amostras a cada ~4 s; perda e jitter do áudio recebido |
| `net` | `nat` (`public`/`cone`/`symmetric`/`cgnat`/`hotspot`/`unknown`), `cgnat`, `v6`, `igd` | sonda STUN/SSDP deste lado |
| `peer` | `cgnat`, `v6` | o que o outro lado informou na sinalização |

Limites conhecidos: o STUN não enxerga CGNAT do tipo cone fora de 100.64/10 (aparece como `cone`); NAT-PMP/PCP
ainda não são medidos (só UPnP por SSDP); relay de amigo/pool ainda não existe, então `relay` (TURN real) não aparece; o TURN
local da ponte entra como `irohDirect`/`irohRelay`. A coluna `ladder` mistura "não conseguiu" com "não tentou" nos degraus futuros.

Relatório: `scripts/field/metrics_report.py arquivo1 [arquivo2...]` (aceita `.jsonl` e a exportação).
