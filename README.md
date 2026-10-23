# Enxame

Comunicador estilo Discord **sem servidor central obrigatório**, em Rust. Cada pessoa instala o app
e ele se conecta direto aos contatos (P2P, com *hole punching* e relay mínimo e self-hostável como
fallback). Enquanto está em uso, o dispositivo **contribui com a rede dos seus contatos**:
guarda mensagens offline cifradas, entrega-as quando o destinatário volta e semeia arquivos para
que quem baixa possa buscar de vários contatos ao mesmo tempo. Quanto mais contatos ativos, melhor a entrega offline.

> **Status: MVP em desenvolvimento** (fases 0 a 4 prontas: identidade, mensagens 1:1, papéis,
> carry, nó headless e arquivos). Sem forward secrecy, sem auditoria externa: veja
> [`SECURITY.md`](SECURITY.md) e [`docs/threat-model.md`](docs/threat-model.md). O nome é
> provisório. A especificação completa está em [`enxame-mvp.md`](enxame-mvp.md).

## Princípios

- **Local-first**: o banco local é a fonte da verdade; tudo funciona offline.
- **Friend-to-friend**: só contatos aceitos dos dois lados trocam tráfego e veem o IP um do outro.
- **E2E sempre**: carriers e relays só veem texto cifrado.
- **Contribuição visível e limitada**: papel automático por contexto (rede, energia, NAT), com
  budget de banda e GB/mês definido por você. Bônus, nunca punição: quem não pode contribuir
  (4G, bateria baixa, modo jogo) mantém a experiência básica completa.

## Arquitetura

```
┌──────────────────────── Dispositivo ────────────────────────┐
│  CLI (hoje)   ·   app Flutter (Fase 6)   ·   nó headless    │
│                          │                                  │
│                  ┌───────▼─────── enxame-core ──────┐       │
│                  │ identity  crypto  store  proto   │       │
│                  │ sync      carry   roles  net     │       │
│                  └───────┬──────────────────────────┘       │
│                          │ iroh (QUIC)                      │
└──────────────────────────┼──────────────────────────────────┘
          direto (hole punching) ◄──┴──► fallback via relay (self-host)
```

| Crate | Papel |
|---|---|
| `enxame-core` | Toda a lógica (identidade, sync, carry, papéis, arquivos). API: struct `Enxame` + stream de `Event`. |
| `enxame-cli` | Binário de desenvolvimento e teste. |
| `enxame-node` | Daemon headless, papel fixo `SuperPeer` (Docker em [`deploy/`](deploy/README.md)). |
| `enxame-ffi` | Fronteira para o Flutter (`flutter_rust_bridge`); o app está em [`app/`](app/). |

## Instalar (Linux)

Baixe a versão mais recente em **Releases** (GitHub), sem compilar nada:

```bash
# App com interface (mensagens, arquivos, chamadas de voz e de tela)
chmod +x Enxame-*-x86_64.AppImage && ./Enxame-*-x86_64.AppImage

# Ou só a linha de comando e o nó 24/7
tar xzf enxame-cli-*-x86_64-linux.tar.gz
```

Confira o download com `sha256sum -c Enxame-*.AppImage.sha256`. O AppImage precisa de GTK 3,
libdbus e PulseAudio do sistema (já presentes em qualquer desktop atual). Na primeira vez o app pede só
o seu nome; relay, DNS e pastas são automáticos. No Windows há um instalador (`Enxame-Setup-<versão>-x64.exe`, por usuário, sem administrador; ADR 0014), em validação de campo.

Para gerar os artefatos você mesmo (precisa só do Docker): `scripts/package/build-linux-release.sh`.

## Como rodar (a partir do código)

Requer Rust 1.91+ (no Linux, `libdbus-1-dev` e `pkg-config` para o keystore do SO).

```bash
cargo build --release -p enxame-cli
cp target/release/enxame-cli .

./enxame-cli init --name alice           # gera a identidade
./enxame-cli serve                       # fica online e imprime o seu ticket (enxame1...)
./enxame-cli add '<ticket do outro>'     # do outro lado: pede para virar contato
./enxame-cli contacts
./enxame-cli chat alice                  # conversa interativa (mostra o que chega)
./enxame-cli send alice "oi"             # uma mensagem avulsa
./enxame-cli history alice               # histórico (✓ = entregue)
./enxame-cli send-file ./foto.png alice bob   # arquivo cifrado; o mesmo blob serve os dois
./enxame-cli files alice                 # arquivos da conversa (↓ = ainda não baixado)
./enxame-cli get alice 1                 # baixa de todos os contatos que tiverem o arquivo
./enxame-cli diag alice                  # NAT, caminho direto/relay, RTT, papel
```

Os dados ficam em `~/.local/share/enxame` (`%APPDATA%\enxame` no Windows). Em desenvolvimento usa
os relays públicos do iroh; em produção, o seu relay (`ENXAME_RELAY=https://...`).

### Nó headless (VPS ou homelab)

```bash
cd deploy && cp .env.example .env     # RELAY_HOSTNAME, ACME_EMAIL, ALLOWED_FINGERPRINTS
docker compose up -d --build
```

Veja [`deploy/README.md`](deploy/README.md): relay com TLS, nó SuperPeer, healthchecks e limites.

## Testes

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # inclui testes de rede in-process, sem internet
scripts/netsim/netsim.sh cone symmetric   # NAT simulado (namespaces + nftables, sem root)
```

## Documentação

- [`enxame-mvp.md`](enxame-mvp.md): especificação e fases.
- [`docs/protocol.md`](docs/protocol.md): wire. [`docs/adr/`](docs/adr): decisões.
- [`docs/reports/`](docs/reports): medições de conectividade e validação em campo.

## Contribuir

Veja [`CONTRIBUTING.md`](CONTRIBUTING.md). Vulnerabilidades: [`SECURITY.md`](SECURITY.md).

## Licença

[AGPL-3.0-only](LICENSE).
