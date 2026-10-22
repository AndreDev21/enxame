#!/usr/bin/env bash
# Teste ponta a ponta da IMAGEM Docker do enxame-node (mensagem + arquivo), tudo em localhost:
# A envia uma mensagem e um arquivo a B (offline); o contêiner (SuperPeer) guarda a mensagem e
# replica o arquivo; A sai da rede; B volta e baixa o arquivo SÓ do contêiner. Sem relay.
#
# Uso: docker build -f deploy/Dockerfile.node -t enxame/node:local .
#      scripts/e2e/node-image.sh
# Variáveis: IMAGE (padrão enxame/node:local), ENXAME_CLI (padrão target/release/enxame-cli).
# Usa `--network host` (Linux).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CLI=${ENXAME_CLI:-$ROOT/target/release/enxame-cli}
IMAGE=${IMAGE:-enxame/node:local}
NAME=enxame-e2e-node
[[ -x "$CLI" ]] || { echo "compile antes: cargo build --release -p enxame-cli" >&2; exit 1; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || { echo "falta a imagem $IMAGE" >&2; exit 1; }

W=$(mktemp -d)
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  # Os arquivos do volume pertencem ao usuário do contêiner: apaga como root dentro dele.
  docker run --rm --user 0 --entrypoint rm -v "$W:/w" "$IMAGE" -rf /w/n >/dev/null 2>&1 || true
  rm -rf "$W"
}
trap cleanup EXIT
export ENXAME_RELAY=disabled ENXAME_MDNS=0 ENXAME_DNS_DISCOVERY=0 RUST_LOG=error
step() { echo; echo "== $*"; }
fail() { echo "FALHOU: $*" >&2; docker logs "$NAME" 2>&1 | tail -20 >&2 || true; exit 1; }

for d in a b; do mkdir -p "$W/$d"; printf 'dial_timeout_secs = 3\nsync_interval_secs = 2\n' > "$W/$d/config.toml"; done
A=("$CLI" --data-dir "$W/a" --no-mdns --role peer)
B=("$CLI" --data-dir "$W/b" --no-mdns --role peer)

step "identidades de A (alice) e B (bob)"
"${A[@]}" init --name alice >/dev/null 2>&1
"${B[@]}" init --name bob   >/dev/null 2>&1
FP_A=$("${A[@]}" id 2>/dev/null | awk '/fingerprint/ {print $2}')
FP_B=$("${B[@]}" id 2>/dev/null | awk '/fingerprint/ {print $2}')

step "sobe o contêiner $IMAGE (SuperPeer) só para A e B"
mkdir -p "$W/n" && chmod 777 "$W/n"
docker run -d --name "$NAME" --network host -v "$W/n:/data" \
  -e ENXAME_DISPLAY_NAME=node -e ENXAME_RELAY=disabled -e ENXAME_MDNS=0 \
  -e ENXAME_ALLOWED_FINGERPRINTS="$FP_A,$FP_B" -e RUST_LOG=info "$IMAGE" >/dev/null
# O volume pertence ao usuário do contêiner (uid 10001): o ticket é lido por dentro dele.
for _ in $(seq 1 60); do docker exec "$NAME" test -s /data/node-ticket.txt 2>/dev/null && break; sleep 0.5; done
TICKET_N=$(docker exec "$NAME" cat /data/node-ticket.txt 2>/dev/null || true)
[[ -n "$TICKET_N" ]] || fail "o contêiner não gerou ticket"
echo "contêiner no ar: $(docker inspect -f '{{.State.Status}}' "$NAME")"

step "A e B adicionam o nó e viram contatos"
"${A[@]}" add "$TICKET_N" 2>&1 | tail -1
"${B[@]}" add "$TICKET_N" 2>&1 | tail -1
mkfifo "$W/b.in"; exec 7<>"$W/b.in"   # aberto nos dois sentidos: a leitura não chega a EOF
"${B[@]}" serve --auto-accept < "$W/b.in" >"$W/b.log" 2>&1 &
BPID=$!; PIDS+=($BPID)
for _ in $(seq 1 40); do grep -q '^enxame1' "$W/b.log" 2>/dev/null && break; sleep 0.5; done
"${A[@]}" add "$(grep -o '^enxame1[a-z2-7]*' "$W/b.log" | head -1)" 2>&1 | tail -1

step "A abre o chat (fica online) e sincroniza com B para o nó aprender os contatos em comum"
mkfifo "$W/a.in"; exec 8<>"$W/a.in"
"${A[@]}" chat bob < "$W/a.in" >"$W/a.log" 2>&1 &
APID=$!; PIDS+=($APID)
sleep 8

step "B cai; A manda texto e um arquivo de 6 MB para B"
kill "$BPID"; wait "$BPID" 2>/dev/null || true
head -c 6000000 /dev/urandom > "$W/arquivo.bin"
SUM=$(sha256sum "$W/arquivo.bin" | cut -d' ' -f1)
printf 'mensagem pela imagem\n' > "$W/a.in"
printf '/enviar %s bob\n' "$W/arquivo.bin" > "$W/a.in"
for _ in $(seq 1 40); do grep -q 'carrier' "$W/a.log" && break; sleep 0.5; done
grep -i 'carrier\|guardada\|entregue\|offline' "$W/a.log" | tail -3 || true

step "o contêiner guarda o aviso e replica o arquivo (A ainda online)"
for _ in $(seq 1 30); do docker exec "$NAME" cat /data/health 2>/dev/null | grep -q 'carry_items=[1-9]' && break; sleep 1; done
docker exec "$NAME" cat /data/health | tee "$W/health.txt"
grep -q 'carry_items=[1-9]' "$W/health.txt" || fail "o contêiner deveria guardar mensagens"
sleep 12   # tempo do prefetch do blob (A segue servindo)

step "A sai da rede"
kill "$APID"; wait "$APID" 2>/dev/null || true

step "B volta e baixa o arquivo só do contêiner"
"${B[@]}" serve --auto-accept < "$W/b.in" >"$W/b2.log" 2>&1 &
BPID=$!; PIDS+=($BPID)
for _ in $(seq 1 40); do grep -q 'mensagem pela imagem' "$W/b2.log" 2>/dev/null && break; sleep 1; done
grep -q 'mensagem pela imagem' "$W/b2.log" || { cat "$W/b2.log"; fail "B não recebeu a mensagem"; }
grep 'mensagem pela imagem' "$W/b2.log" | head -1
kill "$BPID"; wait "$BPID" 2>/dev/null || true
OUT=$("${B[@]}" get alice 1 --out "$W/dl" 2>&1 | tail -1)
echo "$OUT"
GOT=$(sha256sum "$W/dl/arquivo.bin" 2>/dev/null | cut -d' ' -f1 || true)
[[ "$GOT" == "$SUM" ]] || fail "hash do arquivo difere (esperado $SUM, obtido ${GOT:-nada})"

step "saúde e usuário do contêiner"
docker exec "$NAME" enxame-node health && echo "healthcheck: ok ($(docker exec "$NAME" cat /data/health))"
docker exec "$NAME" id -u | grep -qx 10001 || fail "o contêiner deveria rodar como uid 10001"
echo; echo "OK: imagem $IMAGE guardou a mensagem, replicou o arquivo e serviu B com A offline (sha256 confere)."
