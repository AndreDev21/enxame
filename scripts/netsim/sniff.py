#!/usr/bin/env python3
"""Mini-sniffer UDP/IPv4 para depurar hole punching (precisa de CAP_NET_RAW no netns).

Imprime `tempo origem:porta -> destino:porta bytes` para cada datagrama UDP visto, ignorando
as portas de relay passadas em --skip. Uso: sniff.py [--skip 443,7842,80]
"""
import socket, struct, sys, time

skip = {80, 443, 7842}
if "--skip" in sys.argv:
    skip = {int(p) for p in sys.argv[sys.argv.index("--skip") + 1].split(",")}
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(0x0003))
t0 = time.time()
while True:
    data, _ = s.recvfrom(65535)
    if len(data) < 14 + 20 + 8 or data[12:14] != b"\x08\x00":
        continue
    ihl = (data[14] & 0x0F) * 4
    if data[14 + 9] != 17:
        continue
    src, dst = socket.inet_ntoa(data[26:30]), socket.inet_ntoa(data[30:34])
    sp, dp, ln = struct.unpack("!HHH", data[14 + ihl : 14 + ihl + 6])
    if sp in skip or dp in skip:
        continue
    print(f"{time.time()-t0:7.3f} {src}:{sp} -> {dst}:{dp} {ln-8}B", flush=True)
