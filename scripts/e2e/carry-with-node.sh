#!/usr/bin/env bash
# Teste ponta a ponta com os binários reais: A escreve para B (offline) e o `enxame-node`
# (SuperPeer headless) carrega a mensagem até B voltar. Tudo em localhost, sem relay.
#
# Uso: scripts/e2e/carry-with-node.sh        (precisa de `cargo build -p enxame-cli -p enxame-node`)
# Variáveis: ENXAME_CLI, ENXAME_NODE (caminhos dos binários; padrão target/debug).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CLI=${ENXAME_CLI:-$ROOT/target/debug/enxame-cli}
NODE=${ENXAME_NODE:-$ROOT/target/debug/enxame-node}
[[ -x "$CLI" && -x "$NODE" ]] || { echo "compile antes: cargo build -p enxame-cli -p enxame-node" >&2; exit 1; }

W=$(mktemp -d)
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$W"; }
trap cleanup EXIT
export ENXAME_RELAY=disabled ENXAME_MDNS=0 ENXAME_DNS_DISCOVERY=0 RUST_LOG=error
step() { echo; echo "== $*"; }
fail() { echo "FALHOU: $*" >&2; exit 1; }

# Tempos curtos para o teste (a ponta que cai não responde: a discagem tem de desistir logo).
for d in a b; do mkdir -p "$W/$d"; printf 'dial_timeout_secs = 3\nsync_interval_secs = 2\n' > "$W/$d/config.toml"; done
A=("$CLI" --data-dir "$W/a" --no-mdns)
B=("$CLI" --data-dir "$W/b" --no-mdns)

step "identidades de A (alice) e B (bob)"
"${A[@]}" init --name alice >/dev/null 2>&1
"${B[@]}" init --name bob   >/dev/null 2>&1
FP_A=$("${A[@]}" id 2>/dev/null | awk '/fingerprint/ {print $2}')
FP_B=$("${B[@]}" id 2>/dev/null | awk '/fingerprint/ {print $2}')
echo "alice=$FP_A bob=$FP_B"

step "sobe o enxame-node (SuperPeer) só para A e B"
mkdir -p "$W/n"
ENXAME_DISPLAY_NAME=node ENXAME_ALLOWED_FINGERPRINTS="$FP_A,$FP_B" "$NODE" --data-dir "$W/n" run >"$W/node.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 1 40); do [[ -s "$W/n/node-ticket.txt" ]] && break; sleep 0.5; done
TICKET_N=$(cat "$W/n/node-ticket.txt")
[[ -n "$TICKET_N" ]] || { cat "$W/node.log"; fail "o nó não gerou ticket"; }

step "A e B adicionam o nó"
"${A[@]}" add "$TICKET_N" 2>&1 | tail -1
"${B[@]}" add "$TICKET_N" 2>&1 | tail -1

step "A e B viram contatos (B online em serve)"
mkfifo "$W/b.in"; (sleep 3600 > "$W/b.in" &) ; PIDS+=($!)
"${B[@]}" serve --auto-accept < "$W/b.in" >"$W/b.log" 2>&1 &
BPID=$!; PIDS+=($BPID)
for _ in $(seq 1 40); do grep -q '^enxame1' "$W/b.log" 2>/dev/null && break; sleep 0.5; done
TICKET_B=$(grep -o '^enxame1[a-z2-7]*' "$W/b.log" | head -1)
"${A[@]}" add "$TICKET_B" 2>&1 | tail -1

step "A e B sincronizam uma vez (trocam a lista de contatos em comum)"
mkfifo "$W/a.in"; (sleep 3600 > "$W/a.in" &) ; PIDS+=($!)
"${A[@]}" chat bob < "$W/a.in" >"$W/a.log" 2>&1 &
APID=$!; PIDS+=($APID)
sleep 6
kill "$APID" 2>/dev/null || true; wait "$APID" 2>/dev/null || true

step "B cai"
kill "$BPID"; wait "$BPID" 2>/dev/null || true

step "A escreve para B com ele offline: o nó deve aceitar carregar"
OUT=$("${A[@]}" send bob "mensagem que atravessa o nó" 2>&1 | tail -1)
echo "$OUT"
[[ "$OUT" == *"1 contato(s) em comum vão carregá-la"* ]] || fail "o nó não aceitou carregar"
HIST=$("${A[@]}" history bob 2>&1 | tail -1)
echo "$HIST"
[[ "$HIST" == *"…"* ]] || fail "A deveria ver a mensagem pendente (…)"
grep -q 'carry_items=1' "$W/n/health" 2>/dev/null || sleep 11
grep -q 'carry_items=1' "$W/n/health" || fail "o nó deveria estar guardando 1 mensagem: $(cat "$W/n/health")"
echo "nó: $(cat "$W/n/health")"

step "B volta: recebe do nó"
"${B[@]}" serve --auto-accept < "$W/b.in" >"$W/b2.log" 2>&1 &
BPID=$!; PIDS+=($BPID)
for _ in $(seq 1 30); do grep -q 'mensagem que atravessa o nó' "$W/b2.log" 2>/dev/null && break; sleep 1; done
grep -q 'mensagem que atravessa o nó' "$W/b2.log" || { cat "$W/b2.log"; fail "B não recebeu a mensagem"; }
grep 'mensagem que atravessa' "$W/b2.log"

step "o recibo remove a cópia no nó"
for _ in $(seq 1 30); do grep -q 'carry_items=0' "$W/n/health" && break; sleep 1; done
grep -q 'carry_items=0' "$W/n/health" || fail "a cópia deveria ter sido expurgada: $(cat "$W/n/health")"
echo "nó: $(cat "$W/n/health")"
kill "$BPID"; wait "$BPID" 2>/dev/null || true
"${B[@]}" history alice 2>&1 | tail -1

echo; echo "OK: A → nó → B, com B offline na hora do envio; recibo expurgou a cópia."
