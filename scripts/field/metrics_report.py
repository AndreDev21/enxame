#!/usr/bin/env python3
"""Relatório da medição de chamadas (Fase 7.1): % de conexões resolvidas em cada degrau da escada.

Uso: scripts/field/metrics_report.py <call_metrics.jsonl | call_metrics_export.json>...
Aceita os dois formatos e várias fontes (uma por máquina). Esquema: docs/metricas-chamadas.md.
"""
import json
import sys
from collections import Counter


def load(path):
    text = open(path, encoding="utf-8").read()
    try:
        doc = json.loads(text)
        if isinstance(doc, dict) and "calls" in doc:
            return doc["calls"]
    except json.JSONDecodeError:
        pass
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def pct(n, total):
    return f"{100 * n / total:5.1f}%" if total else "    -"


def median(xs):
    xs = sorted(x for x in xs if x is not None)
    return xs[len(xs) // 2] if xs else None


def main(paths):
    rows = [r for p in paths for r in load(p)]
    rows = [r for r in rows if r.get("outcome") != "cancelled"]
    n = len(rows)
    if not n:
        print("sem registros")
        return
    ok = [r for r in rows if r["outcome"] == "connected"]
    print(f"conexões: {n}   conectaram: {len(ok)} ({pct(len(ok), n).strip()})")
    print("\nDegrau que carregou a mídia (entre as que conectaram):")
    ladder = Counter(r["ladder"] for r in ok)
    for k in ("lan", "ipv6", "ipv4Direct", "irohDirect", "irohRelay", "relay"):
        print(f"  {k:<11} {ladder.get(k, 0):4d}  {pct(ladder.get(k, 0), len(ok))}")
    print("\nFalhas por classe:")
    for k, v in Counter(r.get("fail", "?") for r in rows if r["outcome"] == "failed").most_common():
        print(f"  {k:<14} {v:4d}")
    print("\nTempo até conectar (mediana, ms):", median([r.get("ms_to_connect") for r in ok]))
    sw = [r for r in rows if r.get("swapped")]
    print(f"Precisaram inverter papéis: {len(sw)} ({pct(len(sw), n).strip()}); "
          f"dessas, conectaram {sum(1 for r in sw if r['outcome'] == 'connected')}")
    print("\nPor tipo de NAT deste lado (conectaram / total):")
    by = Counter(); byok = Counter()
    for r in rows:
        k = r.get("net", {}).get("nat", "unknown")
        by[k] += 1
        byok[k] += r["outcome"] == "connected"
    for k, v in by.most_common():
        print(f"  {k:<10} {byok[k]:3d}/{v:<3d} {pct(byok[k], v)}")
    print("\nAlcançabilidade (fração dos registros):")
    for label, key in (("IPv6 global", "v6"), ("gateway UPnP", "igd")):
        c = sum(1 for r in rows if r.get("net", {}).get(key))
        print(f"  {label:<13} {c:4d}/{n}  {pct(c, n)}")
    pub = sum(1 for r in rows if r.get("net", {}).get("nat") == "public")
    print(f"  IP público    {pub:4d}/{n}  {pct(pub, n)}")
    print("\nQualidade (conectadas): RTT médio mediano =",
          median([r.get("rtt_ms_avg") for r in ok]), "ms; perda mediana =",
          median([r.get("loss_pct") for r in ok]), "%; trocas de caminho =",
          sum(r.get("path_changes", 0) for r in ok))
    print("\nRegra de decisão (roadmap 7.1): alcançáveis >= ~20% segue com 7.5; ~3% prioriza enxame-node.")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
