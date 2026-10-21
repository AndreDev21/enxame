# netsim

Simula dois dispositivos atrás de NATs (cone ou simétrico) e dois relays numa única máquina Linux,
sem root (user + net + mount namespaces e nftables).

```bash
cargo build -p enxame-cli
cargo install iroh-relay --version '=1.4.1' --features server --locked   # uma vez
scripts/netsim/netsim.sh cone cone                       # hole punching esperado
scripts/netsim/netsim.sh symmetric symmetric             # relay esperado
NETSIM_DEBUG=1 scripts/netsim/netsim.sh cone cone        # + captura UDP e contadores nftables
```

Requisitos: `ip`, `nft`, `openssl`, `unshare`, `python3` e user namespaces sem restrição
(`kernel.unprivileged_userns_clone`/AppArmor no Ubuntu 24.04+ podem bloqueá-los).
Variáveis: `ENXAME_CLI`, `IROH_RELAY_BIN`, `NETSIM_WAIT`, `NETSIM_KEEP_LOGS`, `NETSIM_DEBUG`.
Resultados e a armadilha do conntrack: `docs/reports/fase1-conectividade.md`.
