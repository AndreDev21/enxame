# Relatório da Fase 1 — conectividade

Gerado com `scripts/netsim/netsim.sh <nat_a> <nat_b>` (network namespaces + nftables, sem root,
dois relays `iroh-relay` 1.4.1 com TLS próprio, `enxame-cli` desta árvore). Data: 2026-10-21.
Cada cenário: A em `serve`, B faz `add` do ticket de A (aceitação mútua via relay) e depois
`diag alice --wait-direct 20`.

## Resumo

| NAT de A | NAT de B | NAT detectado por B | Caminho após 20 s | RTT | Até ficar direto | Trocas |
|---|---|---|---|---|---|---|
| cone | cone | Cone | **direto** (hole punching) | 0,6 ms | 38,7 ms | 1 |
| simétrico | simétrico | Symmetric | **relay** | 5,5 ms | — | 0 |
| cone | simétrico | Symmetric | **relay** | 4,7 ms | — | 0 |
| simétrico | cone | Cone | **relay** | 6,6 ms | — | 0 |

- Cone = `masquerade` do nftables (porta de origem preservada, filtro por endereço/porta);
  simétrico = `masquerade random` (porta sorteada por destino).
- Sem relay o NAT não é classificável: com um único relay o QAD não compara destinos e o tipo
  fica `Unknown`. O netsim usa dois relays; o `diag` mostra `mapeamento varia por destino`.
- "Até ficar direto" é medido a partir do início da discagem; "trocas" conta relay↔direto.

## Resultado vs critério de aceite

- Netsim: o cenário simétrico cai em relay e o cone faz hole punching. **Atendido.**
- Duas máquinas reais em redes diferentes virarem contatos via ticket e `diag` mostrar o
  caminho: **atendido** (Linux ↔ Linux, ver "Medição em campo"). Linux ↔ Windows: não testado.

## Armadilha encontrada: conntrack do Linux e hole punching

Na primeira versão do netsim o cone/cone ficava em relay. A captura de pacotes mostrou o motivo:
a sonda do par chegava ao NAT *antes* da nossa saída, o conntrack criava (e, ao chegar ao
`input`, confirmava) uma entrada para esse fluxo "não solicitado", e a nossa sonda de saída
colidia com ela e recebia outra porta pública (29907 em vez de 50162), então o par enviava para
a porta errada. NATs domésticos têm firewall de entrada que descarta tráfego novo vindo da WAN;
o netsim agora reproduz isso (`ct state new drop` no hook `input` da WAN). Isto é um artefato de
simulação, mas mostra que a eficácia do hole punching depende de como o NAT real trata
pacotes não solicitados — motivo pelo qual a taxa direto vs relay precisa ser medida em campo.

## Medição em campo (2026-10-21)

Máquina A (Linux, fibra residencial, relay público padrão do iroh) e máquina C (Arch Linux, outra
LAN e outro IP público, também fibra), contatos via ticket com aceitação mútua conferida pela
fingerprint. `diag` executado em C, 5 rodadas com `--wait-direct 30`:

| Rodada | Handshake | Até ficar direto | RTT | Caminho |
|---|---|---|---|---|
| 1 | 1189,6 ms | 1145,5 ms | 39,4 ms | direto |
| 2 | 1185,6 ms | 1145,6 ms | 35,9 ms | direto |
| 3 | 1109,4 ms | 1069,5 ms | 33,7 ms | direto |
| 4 | 1160,5 ms | 1124,5 ms | 35,9 ms | direto |
| 5 | 1129,2 ms | 1090,1 ms | 41,0 ms | direto |

- **5/5 diretas** entre redes diferentes; 1 troca de caminho e 2 caminhos abertos em todas
  (começa no relay e migra para IP direto). NAT de C: Cone, UDP ok, mapeamento independente do
  destino; relay preferido `use1-1.relay.n0.iroh.link`. Com um único relay conhecido o tipo
  de NAT só é classificável porque o relatório do iroh traz mais de um relay padrão.
- O handshake (≈1,1 s) é quase todo o tempo até o caminho ficar direto (≈1,1 s): o `Hello` só
  termina depois que o hole punching conclui, ou seja ≈1 s é o custo de abrir uma sessão nova
  nesse par de redes. Vale investigar se o `Hello` deveria seguir pelo relay sem esperar.
- RTT direto de 34–41 ms, coerente com duas casas em cidades/ISPs diferentes.
- Não medido: sentido inverso (`diag` em A), 4G/CGNAT, Starlink, mesma LAN, Windows.
- Problema encontrado: em máquinas com Tailscale (MagicDNS em `100.100.100.100`), o resolvedor do
  iroh (que usa EDNS) não recebe resposta, o relay não conecta e o ticket sai sem relay. Em uma das
  máquinas foi preciso rodar com `resolv.conf` próprio; em C não ocorreu. Vale um fallback de DNS
  configurável.

## Roteiro manual (restante)

Para cada par de redes (fibra↔fibra, fibra↔4G por tethering, fibra↔Starlink):

1. Máquina A: `enxame-cli init --name a && enxame-cli serve` (copiar o ticket impresso).
2. Máquina B: `enxame-cli add <ticket>`; em A, responder `s`.
3. Em B: `enxame-cli diag a --wait-direct 30`; registrar caminho, RTT, tempo até direto, NAT.
4. Repetir 5 vezes por par e anotar a taxa de conexões diretas.

| Par de redes | NAT A / B | Direto em N/5 | RTT direto | Tempo até direto | Observações |
|---|---|---|---|---|---|
| fibra ↔ fibra | | | | | |
| fibra ↔ 4G | | | | | |
| fibra ↔ Starlink | | | | | |
