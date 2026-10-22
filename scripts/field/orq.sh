#!/usr/bin/env bash
# Orquestrador de testes de campo: mantém um `enxame-cli chat` aberto em segundo plano, com a
# entrada por uma FIFO, para que um script (ou o Claude Code) mande mensagens e leia as respostas
# dos testadores sem um terminal interativo.
#
# Uso:
#   scripts/field/orq.sh start bob andrecasa      # abre o chat com esses contatos
#   scripts/field/orq.sh say "@bob texto"         # envia (`@nome texto` escolhe o destinatário)
#   scripts/field/orq.sh see                      # mostra as linhas novas do log
#   scripts/field/orq.sh stop                     # /sair e limpa
#
# Variáveis:
#   ENXAME_CLI        binário (padrão: target/release/enxame-cli)
#   ORQ_STATE         pasta de estado (padrão: $XDG_RUNTIME_DIR/enxame-orq ou /tmp/enxame-orq)
#   ORQ_CLI_ARGS      argumentos globais do CLI (ex.: "--data-dir /tmp/x --no-mdns")
#   ENXAME_DNS_SERVERS  padrão aqui: 1.1.1.1,8.8.8.8 (contorna DNS que descarta EDNS)
#
# Só um processo por identidade: não deixe outro `chat`/`serve` aberto no mesmo diretório de dados.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CLI=${ENXAME_CLI:-$ROOT/target/release/enxame-cli}
STATE=${ORQ_STATE:-${XDG_RUNTIME_DIR:-/tmp}/enxame-orq}
export ENXAME_DNS_SERVERS=${ENXAME_DNS_SERVERS:-1.1.1.1,8.8.8.8}

# Escreve uma linha na FIFO sem travar se o chat já saiu (sem leitor, a escrita bloquearia).
send_line() {
  if [[ -e "$STATE/chat.pid" ]] && kill -0 "$(cat "$STATE/chat.pid")" 2>/dev/null; then
    timeout 5 bash -c 'printf "%s\n" "$1" > "$2"' _ "$1" "$STATE/in" || return 1
  else
    echo "o chat não está rodando (veja: orq.sh see)" >&2
    return 1
  fi
}

clean() { sed 's/\x1b\[[0-9;]*m//g' | grep -v keystore || true; }

case "${1:-}" in
  start)
    shift
    [[ $# -ge 1 ]] || { echo "uso: orq.sh start <contato> [contato...]" >&2; exit 2; }
    [[ -x "$CLI" ]] || { echo "binário não encontrado: $CLI (cargo build --release -p enxame-cli)" >&2; exit 1; }
    [[ ! -e "$STATE/chat.pid" ]] || { echo "já existe um orquestrador ($STATE); rode stop antes" >&2; exit 1; }
    mkdir -p "$STATE"
    rm -f "$STATE/in"; mkfifo "$STATE/in"
    # Mantém a FIFO aberta para o chat não ver EOF entre uma mensagem e outra.
    sleep 2147483647 > "$STATE/in" & echo $! > "$STATE/keeper.pid"
    # shellcheck disable=SC2086
    nohup "$CLI" ${ORQ_CLI_ARGS:-} chat "$@" < "$STATE/in" > "$STATE/log" 2>&1 &
    echo $! > "$STATE/chat.pid"
    echo 0 > "$STATE/offset"
    echo "orquestrador no ar (pid $(cat "$STATE/chat.pid")); log em $STATE/log"
    ;;
  say)
    shift
    send_line "$*"
    ;;
  see)
    off=$(cat "$STATE/offset" 2>/dev/null || echo 0)
    total=$(wc -l < "$STATE/log")
    tail -n +$((off + 1)) "$STATE/log" | clean
    echo "$total" > "$STATE/offset"
    ;;
  stop)
    send_line "/sair" 2>/dev/null || true
    sleep 2
    for f in chat.pid keeper.pid; do
      [[ -e "$STATE/$f" ]] && kill "$(cat "$STATE/$f")" 2>/dev/null || true
    done
    rm -rf "$STATE"
    echo "orquestrador encerrado"
    ;;
  *)
    sed -n '2,20p' "$0"
    exit 2
    ;;
esac
