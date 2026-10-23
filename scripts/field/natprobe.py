#!/usr/bin/env python3
"""Sonda de NAT para os testes de campo (só biblioteca padrão, Linux e Windows).

Mede o que o WebRTC enfrenta: se o NAT reaproveita a porta mapeada para destinos diferentes
(mapeamento) e se ele deixa entrar pacotes de quem ainda não recebeu nada (filtragem), com um
par que roda a mesma sonda do outro lado.

  natprobe.py map
      Mesmo socket, vários servidores STUN: mostra o endpoint público visto por cada um.
  natprobe.py punch --peer IP:PORTA --at EPOCH [--secs 12] [--port PORTA_LOCAL]
      Descobre o endpoint pelo STUN, imprime "MEU <ip:porta>" e, a partir de EPOCH (segundos Unix),
      manda um pacote por 200 ms ao par durante --secs. Imprime o que recebeu.
      Os dois lados escolhem EPOCH diferentes para testar quem sai primeiro.
Não imprima nem comite endereços IP públicos em relatórios.
"""
import argparse, os, socket, struct, sys, time

STUN = [("stun.l.google.com", 19302), ("stun1.l.google.com", 19302),
        ("stun.cloudflare.com", 3478), ("stun.nextcloud.com", 443)]
MAGIC = 0x2112A442


def binding(sock, server, timeout=2.0):
    tid = os.urandom(12)
    req = struct.pack("!HHI", 0x0001, 0, MAGIC) + tid
    addr = socket.getaddrinfo(server[0], server[1], socket.AF_INET, socket.SOCK_DGRAM)[0][4]
    sock.settimeout(timeout)
    sock.sendto(req, addr)
    end = time.time() + timeout
    while time.time() < end:
        try:
            data, src = sock.recvfrom(2048)
        except socket.timeout:
            return addr, None
        if len(data) < 20 or data[8:20] != tid:
            continue
        i = 20
        while i + 4 <= len(data):
            t, l = struct.unpack("!HH", data[i:i + 4])
            v = data[i + 4:i + 4 + l]
            if t == 0x0020 and len(v) >= 8:  # XOR-MAPPED-ADDRESS
                port = struct.unpack("!H", v[2:4])[0] ^ (MAGIC >> 16)
                ip = socket.inet_ntoa(bytes(a ^ b for a, b in zip(v[4:8], struct.pack("!I", MAGIC))))
                return addr, f"{ip}:{port}"
            i += 4 + l + (-l % 4)
    return addr, None


def cmd_map(a):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("0.0.0.0", a.port))
    print("local", s.getsockname()[1])
    seen = []
    for srv in STUN:
        try:
            dst, mapped = binding(s, srv)
        except Exception as e:
            print(f"{srv[0]}: erro {e}")
            continue
        print(f"{srv[0]} ({dst[0]}): {mapped}")
        if mapped:
            seen.append(mapped)
    ports = {m.split(":")[1] for m in seen}
    ips = {m.split(":")[0] for m in seen}
    print(f"RESUMO: {len(seen)} respostas, {len(ips)} IP(s) público(s), {len(ports)} porta(s) distinta(s)")
    print("mapeamento:", "independente do destino (cone)" if len(ports) == 1 and seen else
          "DEPENDENTE do destino (simétrico)" if seen else "sem resposta")


def cmd_punch(a):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("0.0.0.0", a.port))
    _, mine = binding(s, STUN[0])
    print("MEU", mine, flush=True)
    if a.peer_file:
        # O endpoint público do par muda a cada execução (o CGNAT dá outra porta a cada sessão):
        # espera o arquivo que o orquestrador escreve com o "MEU" do outro lado.
        while not (os.path.exists(a.peer_file) and os.path.getsize(a.peer_file) > 0):
            time.sleep(0.2)
            if time.time() > a.at:
                sys.exit("o endereço do par não chegou a tempo")
        a.peer = open(a.peer_file).read().strip()
    ip, port = a.peer.rsplit(":", 1)
    peer = (ip, int(port))
    print("PAR", a.peer, flush=True)
    wait = a.at - time.time()
    print(f"saindo em {wait:.1f}s", flush=True)
    while time.time() < a.at:
        time.sleep(0.01)
    s.settimeout(0.05)
    t0 = time.time()
    got, first, srcs, sent = 0, None, set(), 0
    nxt = 0.0
    while time.time() - t0 < a.secs:
        if time.time() >= nxt:
            # Pacote no formato STUN (Binding Request): é o que o ICE manda, e há redes que tratam
            # UDP de formato desconhecido de outro jeito.
            s.sendto(struct.pack("!HHI", 0x0001, 0, MAGIC) + os.urandom(12), peer)
            sent += 1
            nxt = time.time() + 0.2
        try:
            data, src = s.recvfrom(2048)
        except socket.timeout:
            continue
        if len(data) >= 20 and struct.unpack("!I", data[4:8])[0] == MAGIC:
            got += 1
            srcs.add(src[1] == peer[1] and "porta esperada" or "porta DIFERENTE")
            if first is None:
                first = time.time() - t0
    print(f"RESULTADO: enviou {sent}, recebeu {got}, primeiro em {first if first is None else round(first, 2)}s, origem: {sorted(srcs)}")


p = argparse.ArgumentParser()
sub = p.add_subparsers(dest="cmd", required=True)
m = sub.add_parser("map"); m.add_argument("--port", type=int, default=0); m.set_defaults(f=cmd_map)
u = sub.add_parser("punch"); u.add_argument("--peer", default=None); u.add_argument("--peer-file", dest="peer_file", default=None); u.add_argument("--at", type=float, required=True)
u.add_argument("--secs", type=float, default=12); u.add_argument("--port", type=int, default=0); u.set_defaults(f=cmd_punch)
a = p.parse_args()
a.f(a)
