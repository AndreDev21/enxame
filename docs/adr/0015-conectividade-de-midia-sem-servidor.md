# ADR 0015 — Conectividade de mídia sem servidor

Status: aceito para as decisões D2–D7 (2026-10-24, `docs/roadmap-fase7-conformidade.md`); a abordagem
técnica do bloco 7.2 (TURN local sobre o iroh) é **proposta** e só vira aceita depois do protótipo
(`docs/plano-fase7-2-midia-sobre-iroh.md`). Spec: seção 13/Fase 7 e decisão aberta 5.

## Contexto

Mensagens e arquivos sempre chegam: o iroh tem relay como rede de segurança. As **chamadas** (ADR 0012)
usam WebRTC com ICE e só STUN; se o furo UDP direto falha, não há relay para a mídia e a chamada não
conecta. Medido em campo (`docs/reports/fase6-windows.md`, `docs/reports/fase7-medicao-1.md`): com CGNAT
móvel só funciona se o lado do CGNAT envia primeiro (a inversão de papéis, ADR 0012, mitiga); dois
dispositivos atrás de CGNAT não conectam sem intermediário. Na medição de 2026-10-26 (uma operadora) 4/4
chamadas conectaram direto, mas uma amostra de uma operadora não mostra a fração alcançável.

Restrição de produto (spec 1.1, ADR 0008): usuário básico sem configuração, e nenhum servidor que o projeto
precise hospedar e pagar.

## Decisões

1. **Sem TURN hospedado pelo projeto (D2).** Princípio: nenhum servidor que o projeto precise hospedar e
   pagar, e nenhum que não possa ser substituído por um equivalente open source rodado por qualquer pessoa.
   Os relays do n0 já carregam mensagens entre dois CGNAT; o `iroh-relay` é open source e auto-hospedável.
2. **Ninguém fica sem se comunicar (D3).** Voz funciona sempre, inclusive com duas pessoas em CGNAT e sem
   terceiro disponível. Vídeo e tela dependem de caminho direto ou de relay com banda.
3. **Escada de conexão (D4)**, parando no primeiro degrau que funciona: (1) IPv6 direto → (2) IPv4 direto
   com punching avançado → (3) relay de amigo → (4) relay de amigo-de-amigo / pool comunitário com endosso →
   (5) pool sem endosso (peso mínimo, só voz) → (6) relay público n0 (só voz, desligável).
4. **Relay de amigos opt-in, com limite de banda padrão (D5);** contribuição medida no total (carry, seed,
   uptime, relay) para não punir quem está em CGNAT.
5. **Relay comunitário com confiança local (D6):** qualidade medida pelo próprio cliente + endosso de
   amigos + antiguidade + diversidade de rede; sem reputação global (vulnerável a Sybil); chave efêmera por
   sessão.
6. **Metadados (D7):** confiança nos amigos por padrão; uso de relay comunitário é sinalizado na UI e pode ser
   bloqueado. "Chamada protegida" (forçar relay para esconder o IP) fica para depois. Cebola/multi-hop está
   fora do roadmap.
7. **Mecanismo proposto para a mídia sobre o iroh (7.2):** um **servidor TURN local**, só em `127.0.0.1`,
   embutido no núcleo Rust (crate `turn` do webrtc-rs, 0.17.2, MIT/Apache-2.0), configurado como `iceServers`
   do `flutter_webrtc`. Seu `RelayAddressGenerator` devolve um `Conn` próprio que, em vez de abrir um socket
   UDP, manda e recebe os pacotes por **datagramas QUIC do iroh** (`Connection::send_datagram`) numa conexão
   `enxame/media/1` entre os dois contatos. O libwebrtc vê um candidato `relay` comum; o ICE decide entre ele e
   os diretos pela prioridade, e a mudança relay → direto do próprio iroh (`path_events`) é transparente.
   Nenhuma criptografia nova: DTLS-SRTP continua ponta a ponta dentro do TLS do QUIC.

## Alternativas consideradas

Resumo de `docs/estudo-conectividade-sem-servidor.md` (3.1–3.10):

- **TURN próprio do projeto ou pago de terceiros:** viola D2 (custo, ponto único de falha e de abuso,
  credenciais). Descartado.
- **TURN opt-in de cada usuário (`deploy/`):** respeita a filosofia, mas o usuário básico não configura.
  Mantido apenas como relay de amigo (7.4).
- **Só inversão de papéis + melhor mensagem (3.1/3.10):** feito no 7.0; não resolve dois CGNAT.
- **Reescrever candidatos ICE para um encaminhador UDP local (3.3, variante):** mesmo efeito do mecanismo
  acima, porém manual e frágil; fica como plano B do 7.2.
- **Mídia própria sobre o iroh, sem WebRTC:** reimplementaria AEC, redução de ruído e controle de banda.
  Descartado.
- **Cebola/multi-hop, Tor/Lokinet/Nym, libp2p circuit relay:** latência e banda multiplicadas, rede pequena
  dá pouco anonimato, complexidade do tamanho do app. Descartados.
- **Reputação global de relays:** vulnerável a Sybil. Descartada em favor da confiança local.

## Consequências e riscos

- **Metadados:** um relay vê IPs, horários, volume e o par de identidades, e pode degradar a chamada; nunca
  vê o conteúdo (S1 do roadmap: fingerprints DTLS dentro da sinalização assinada).
- **TURN local:** a autenticação de longo prazo do TURN usa MD5/HMAC-SHA1 por exigência do protocolo (crate
  `turn`, via `ring`/`md-5`). É aceitável só porque o servidor escuta **somente loopback**, em porta efêmera,
  com usuário e senha aleatórios por chamada; ainda assim fica registrado como exceção à regra "só crates
  auditadas da seção 5" e precisa de revisão.
- **Incógnitas** (verificadas no protótipo do 7.2): ~~se o libwebrtc aceita um servidor TURN em
  `127.0.0.1`~~ (resolvida em 2026-10-26: aceita e conecta por `relay/relay`; ver o plano); se o RTP cabe no `max_datagram_size` do caminho (se não, fragmentar); a interação entre o
  controle de congestionamento do QUIC e o do WebRTC; o limite de banda dos relays do n0 (~1 MB/s medido em
  campo), por isso o último degrau é só voz.
- **Mobile é restrição de projeto (D1):** o `crates/enxame-core` continua compilando para
  `aarch64-linux-android`; o `turn` traz `ring`, que precisa do NDK (já usado no job `android` do CI).
- **Conformidade:** relays não exigem credencial de idade (Fase 8); a credencial é portão entre contatos,
  não entre cliente e relay.
