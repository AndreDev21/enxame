#!/usr/bin/env bash
# Simulação de NAT com network namespaces + nftables numa única máquina Linux.
#
# Topologia (tudo dentro de um user+net+mount namespace próprio; não precisa de root):
#
#   a (10.1.0.2) -- nat_a --\                       /-- relay (11.3.0.1)
#                            inet (roteador público)
#   b (10.2.0.2) -- nat_b --/
#
# Uso:
#   scripts/netsim/netsim.sh <nat_a> <nat_b> [relatorio.txt]
#     nat_x ∈ cone | symmetric
#
# Variáveis:
#   ENXAME_CLI      caminho do enxame-cli      (padrão: target/debug/enxame-cli)
#   NETSIM_WAIT     segundos esperando o caminho virar direto (padrão: 30)
#   NETSIM_KEEP_LOGS  diretório onde copiar os logs dos processos
#   IROH_RELAY_BIN  caminho do iroh-relay      (cargo install iroh-relay --features server)
#
# O que mede: o `add` entre A e B passa pelo relay; depois `diag` em B informa se o caminho
# virou direto (hole punching) ou ficou em relay, RTT e tempo até ficar direto.
set -euo pipefail

if [[ "${1:-}" != "--inner" ]]; then
  [[ $# -ge 2 ]] || { sed -n '2,20p' "$0"; exit 2; }
  for n in "$1" "$2"; do
    [[ "$n" == cone || "$n" == symmetric ]] || { echo "NAT inválido: $n" >&2; exit 2; }
  done
  exec unshare --user --map-root-user --net --mount -- "$0" --inner "$@"
fi
shift # --inner

NAT_A=$1 NAT_B=$2 REPORT=${3:-}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CLI=${ENXAME_CLI:-$ROOT/target/debug/enxame-cli}
RELAY_BIN=${IROH_RELAY_BIN:-iroh-relay}
[[ -x "$CLI" ]] || { echo "enxame-cli não encontrado em $CLI (cargo build -p enxame-cli)" >&2; exit 1; }
command -v "$RELAY_BIN" >/dev/null || { echo "iroh-relay não encontrado (IROH_RELAY_BIN)" >&2; exit 1; }
for tool in ip nft openssl; do command -v $tool >/dev/null || { echo "falta $tool" >&2; exit 1; }; done

WORK=$(mktemp -d)
PIDS=()
cleanup() { if [[ -n "${NETSIM_KEEP_LOGS:-}" ]]; then cp "$WORK"/*.log "$NETSIM_KEEP_LOGS"/ 2>/dev/null || true; fi; for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$WORK"; }
trap cleanup EXIT

mount -t tmpfs tmpfs /run
mkdir -p /run/netns

ns() { local n=$1; shift; ip netns exec "$n" "$@"; }

for n in inet nat_a nat_b relay relay2 a b; do ip netns add $n; ns $n ip link set lo up; done
for n in inet nat_a nat_b; do ns $n sysctl -qw net.ipv4.ip_forward=1; done

# link <ns1> <if1> <addr1/len> <ns2> <if2> <addr2/len>
link() {
  ip link add "$2" netns "$1" type veth peer name "$5" netns "$4"
  ns "$1" ip addr add "$3" dev "$2"; ns "$1" ip link set "$2" up
  ns "$4" ip addr add "$6" dev "$5"; ns "$4" ip link set "$5" up
}
link a   eth0 10.1.0.2/24     nat_a lan  10.1.0.1/24
link b   eth0 10.2.0.2/24     nat_b lan  10.2.0.1/24
link nat_a wan 11.1.0.1/24  inet  to_a 11.1.0.254/24
link nat_b wan 11.2.0.1/24  inet  to_b 11.2.0.254/24
link relay eth0 11.3.0.1/24 inet  to_r 11.3.0.254/24
link relay2 eth0 11.4.0.1/24 inet  to_r2 11.4.0.254/24
ns a ip route add default via 10.1.0.1
ns b ip route add default via 10.2.0.1
ns nat_a ip route add default via 11.1.0.254
ns nat_b ip route add default via 11.2.0.254
ns relay ip route add default via 11.3.0.254
ns relay2 ip route add default via 11.4.0.254

# NAT: `masquerade` mantém a porta de origem (mapeamento independente do destino: cone com
# filtro por endereço/porta). `masquerade random` sorteia a porta por destino (simétrico).
setup_nat() {
  local n=$1 kind=$2 flags=""
  [[ $kind == symmetric ]] && flags="random"
  ns "$n" nft -f - <<NFT
table ip nat {
  chain post { type nat hook postrouting priority 100; oifname "wan" masquerade $flags; }
}
NFT
}
# Contadores de diagnóstico (NETSIM_DEBUG=1): pacotes UDP que chegam na wan e os encaminhados.
setup_dbg() {
  ns "$1" nft add table ip dbg
  ns "$1" nft "add chain ip dbg pre { type filter hook prerouting priority -300; }"
  ns "$1" nft "add chain ip dbg forwarded { type filter hook forward priority 0; }"
  ns "$1" nft add rule ip dbg pre iifname wan meta l4proto udp counter comment '"udp in wan"'
  ns "$1" nft add rule ip dbg forwarded iifname wan meta l4proto udp counter comment '"udp forwarded from wan"'
}
setup_dbg nat_a; setup_dbg nat_b
# Firewall de entrada do "roteador doméstico": descarta tráfego novo vindo da wan. Sem isso, a
# primeira sonda do par chega antes da nossa e o conntrack do Linux cria uma entrada que rouba o
# mapeamento da porta, quebrando o hole punching (artefato do conntrack, não de NATs reais).
setup_fw() {
  ns "$1" nft add table ip fw
  ns "$1" nft "add chain ip fw input { type filter hook input priority 0; policy accept; }"
  ns "$1" nft add rule ip fw input iifname wan ct state new drop
}
setup_fw nat_a; setup_fw nat_b
setup_nat nat_a "$NAT_A"
setup_nat nat_b "$NAT_B"

# Relay com TLS próprio (QAD exige TLS+QUIC); os clientes confiam nesse certificado via relay_ca_file.
# CA própria (confiada pelos clientes) + certificado de folha do relay (CA:FALSE, SAN com o IP).
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 \
  -keyout "$WORK/ca.key" -out "$WORK/ca.crt" -subj "/CN=enxame-netsim-ca" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout "$WORK/relay.key" -out "$WORK/relay.csr" -subj "/CN=enxame-netsim-relay" 2>/dev/null
printf 'basicConstraints=CA:FALSE\nsubjectAltName=IP:11.3.0.1,IP:11.4.0.1\nextendedKeyUsage=serverAuth\n' > "$WORK/leaf.ext"
openssl x509 -req -in "$WORK/relay.csr" -CA "$WORK/ca.crt" -CAkey "$WORK/ca.key" -CAcreateserial \
  -days 2 -extfile "$WORK/leaf.ext" -out "$WORK/relay.crt" 2>/dev/null
cat > "$WORK/relay.toml" <<TOML
enable_relay = true
enable_quic_addr_discovery = true
enable_metrics = false
http_bind_addr = "0.0.0.0:80"
[tls]
cert_mode = "Manual"
hostname = "11.3.0.1"
https_bind_addr = "0.0.0.0:443"
quic_bind_addr = "0.0.0.0:7842"
manual_cert_path = "$WORK/relay.crt"
manual_key_path = "$WORK/relay.key"
TOML
# Dois relays: com um só, o QAD não compara destinos e o tipo de NAT fica `Unknown`.
ns relay "$RELAY_BIN" --config-path "$WORK/relay.toml" >"$WORK/relay.log" 2>&1 &
PIDS+=($!)
ns relay2 "$RELAY_BIN" --config-path "$WORK/relay.toml" >"$WORK/relay2.log" 2>&1 &
PIDS+=($!)
sleep 2
ns a ping -c1 -W2 11.3.0.1 >/dev/null || { echo "A não alcança o relay"; cat "$WORK/relay.log"; exit 1; }

export ENXAME_RELAY=https://11.3.0.1,https://11.4.0.1 ENXAME_RELAY_CA_FILE="$WORK/ca.crt" \
       ENXAME_DNS_DISCOVERY=0 RUST_LOG=${RUST_LOG:-error}
ARGS=(--no-mdns)

"$CLI" "${ARGS[@]}" --data-dir "$WORK/a" init --name alice >/dev/null 2>&1
"$CLI" "${ARGS[@]}" --data-dir "$WORK/b" init --name bob >/dev/null 2>&1

ns a "$CLI" "${ARGS[@]}" --data-dir "$WORK/a" serve --auto-accept >"$WORK/a.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 1 30); do grep -q '^enxame1' "$WORK/a.log" 2>/dev/null && break; sleep 0.5; done
TICKET=$(grep -o '^enxame1[a-z2-7]*' "$WORK/a.log" | head -1)
[[ -n "$TICKET" ]] || { echo "A não gerou ticket"; cat "$WORK/a.log"; exit 1; }

if [[ -n "${NETSIM_DEBUG:-}" ]]; then
  # Captura UDP no roteador público (exceto tráfego de relay) para ver as sondas de hole punching.
  ns inet python3 "$ROOT/scripts/netsim/sniff.py" >"$WORK/sniff.log" 2>&1 &
  PIDS+=($!)
fi
WAIT=${NETSIM_WAIT:-30}
{
  echo "# netsim: A atrás de NAT $NAT_A, B atrás de NAT $NAT_B (2 relays, espera de ${WAIT}s por caminho direto)"
  echo "# $(date -u +%Y-%m-%dT%H:%M:%SZ)  $("$CLI" version)"
  echo
  echo "## add (B → A)"
  ns b timeout 90 "$CLI" "${ARGS[@]}" --data-dir "$WORK/b" add "$TICKET" 2>&1 | grep -v keystore
  echo
  echo "## diag (B → A)"
  ns b timeout 120 "$CLI" "${ARGS[@]}" --data-dir "$WORK/b" diag alice --wait-direct "$WAIT" 2>&1 | grep -v keystore
} | tee ${REPORT:+"$REPORT"} /dev/null
if [[ -n "${NETSIM_DEBUG:-}" ]]; then
  echo "## pacotes UDP fora do relay vistos no roteador público (primeiros 40)"
  head -40 "$WORK/sniff.log" || true
  for n in nat_a nat_b; do echo "## contadores $n"; ns $n nft list table ip dbg | grep -E "counter|comment"; done
fi
