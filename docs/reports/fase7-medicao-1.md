# Fase 7.1 — primeira medição de campo (2026-10-26)

Build `e8b080f`. Windows (notebook) ↔ Linux (PC), identidades de teste, áudio. Duas redes: Wi-Fi de casa (mesma
LAN) e hotspot 5G do celular (uma única operadora; não houve acesso a outras). Quatro chamadas, vistas pelos
dois lados (8 registros; cada chamada conta duas vezes no relatório).

| Rede | Quem liga | Degrau | Tempo até conectar | `swap` | RTT médio |
|---|---|---|---|---|---|
| Wi-Fi casa | Windows | lan | 0,9 s | não | 5 ms |
| Wi-Fi casa | Linux | lan | 0,8 s | não | 6 ms |
| Hotspot 5G | Linux | ipv4Direct (srflx/srflx) | 8,9 s | sim | 69 ms |
| Hotspot 5G | Windows | ipv4Direct (srflx/srflx) | 2,1 s | não | 74–89 ms |

- 4/4 chamadas conectaram, sem relay, perda 0 %, nenhuma troca de caminho.
- No hotspot a 1ª chamada precisou da inversão de papéis (8,7–8,9 s); a seguinte conectou de primeira em ~2 s
  (memória por contato). Consistente com a hipótese do estudo (ordem de quem envia primeiro importa), mas a
  amostra é pequena.
- Contraste com as 4 chamadas anteriores no mesmo hotspot (build `4d7f518`, sem caminho em nenhuma ordem): não
  se sabe se o NAT variou com o tempo ou se foi outro fator. Uma única operadora não permite concluir.
- Este lado (Linux): NAT `cone`, UPnP presente (igd=true), sem IPv6 global. Windows: IPv6 global (v6=true), UPnP no
  Wi-Fi de casa e não no hotspot. Nenhum IP público direto.
- Achado e corrigido: o libwebrtc devolve o candidato remoto `prflx` **sem endereço**; o Windows classificava a LAN
  como `ipv4Direct`. Agora `host` privado + remoto `prflx` sem endereço + RTT < 30 ms = `lan`.

**Fração de alcançáveis (regra de decisão do 7.5):** não calculável com 2 redes de uma operadora. Pendente:
outras operadoras, Wi-Fi público/corporativo e fibras diferentes (mínimo ~5 redes).
