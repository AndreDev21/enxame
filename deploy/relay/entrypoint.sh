#!/bin/sh
# Gera a configuração do iroh-relay a partir de variáveis de ambiente e sobe o servidor.
set -eu

: "${RELAY_HOSTNAME:?defina RELAY_HOSTNAME (nome DNS público deste relay)}"
MODE="${RELAY_CERT_MODE:-LetsEncrypt}"
CFG=/data/relay.toml
mkdir -p /data/certs

{
  echo 'enable_relay = true'
  echo 'enable_quic_addr_discovery = true'
  echo 'enable_metrics = false'
  echo 'http_bind_addr = "0.0.0.0:80"'
  echo '[tls]'
  echo "cert_mode = \"$MODE\""
  echo "hostname = \"$RELAY_HOSTNAME\""
  echo 'https_bind_addr = "0.0.0.0:443"'
  echo 'quic_bind_addr = "0.0.0.0:7842"'
  case "$MODE" in
    LetsEncrypt)
      : "${ACME_EMAIL:?defina ACME_EMAIL para o Let's Encrypt}"
      echo 'cert_dir = "/data/certs"'
      echo "contact = \"$ACME_EMAIL\""
      echo "prod_tls = ${ACME_PROD:-true}"
      ;;
    Manual)
      echo 'manual_cert_path = "/certs/relay.crt"'
      echo 'manual_key_path = "/certs/relay.key"'
      ;;
    *) echo "RELAY_CERT_MODE inválido: $MODE (use LetsEncrypt ou Manual)" >&2; exit 2 ;;
  esac
} > "$CFG"

exec iroh-relay --config-path "$CFG"
