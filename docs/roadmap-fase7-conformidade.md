# Enxame — Atualização de roadmap: conectividade sem servidor, conformidade ECA Digital e segurança

> **Para o Claude Code.** Este documento consolida decisões tomadas em 2026-10-24 a partir do
> estudo `estudo-conectividade-sem-servidor.md`. Sua tarefa é **incorporar tudo isto ao roadmap do
> projeto e abrir os ADRs correspondentes**, seguindo as convenções já existentes no repositório.

---

## 0. Instruções de execução

1. **Antes de editar**, localize e leia:
   - o arquivo de roadmap atual (ex.: `ROADMAP.md`, `docs/roadmap.md` ou equivalente);
   - a pasta de ADRs (ex.: `docs/adr/`) e o template/numeração em uso;
   - `estudo-conectividade-sem-servidor.md` (se estiver no repo; senão, use este documento como fonte).
2. **Não remova** itens existentes do roadmap. Ajuste numeração de fases/blocos para encaixar na
   estrutura atual (este documento usa "Fase 7/8/9" como referência; adapte se a numeração real for
   outra e registre o mapeamento no commit).
3. Mantenha o idioma do repositório (português) e o estilo dos ADRs existentes.
4. Crie os ADRs usando os **próximos números livres** (referência abaixo: 0015, 0016, 0017).
   O estudo de conectividade entra como "Alternativas consideradas" do ADR de conectividade.
5. Itens marcados **[VERIFICAR]** exigem checagem no código/documentação antes de virar tarefa
   (não assuma; se já existir, marque como feito e referencie o arquivo).
6. Cada bloco novo do roadmap deve ter: objetivo, tarefas, critério de aceite e dependências.
7. Ao final, gere um resumo do que mudou (arquivos criados/alterados, ADRs abertos, itens
   [VERIFICAR] e o resultado de cada um).

---

## 1. Decisões de produto (registrar no roadmap e nos ADRs)

| # | Decisão |
|---|---|
| D1 | **Público-alvo:** usuário leigo. Foco inicial **desktop Windows e Linux**. Mobile não é foco agora, mas é **restrição de projeto**: tudo deve funcionar com o mobile como consumidor de relay (sempre CGNAT, bateria, sem servir relay). |
| D2 | **Princípio "sem servidor" reformulado:** *nenhum servidor que o projeto precise hospedar e pagar, e nenhum que não possa ser substituído por um equivalente open source rodado por qualquer pessoa.* (Os relays n0 já estão no caminho de mensagens entre dois CGNAT; o `iroh-relay` é open source e auto-hospedável.) |
| D3 | **Ninguém fica sem se comunicar:** voz deve funcionar sempre, inclusive com apenas duas pessoas atrás de CGNAT e sem terceiro disponível. Vídeo/tela dependem de caminho direto ou relay com banda. |
| D4 | **Escada de conexão** (ordem de tentativa; para no primeiro que funcionar): (1) IPv6 direto → (2) IPv4 direto com punching avançado → (3) relay de amigo → (4) relay de amigo-de-amigo / pool comunitário com endosso → (5) pool sem endosso (peso mínimo, só voz) → (6) relay público n0 (só voz, desligável). |
| D5 | **Relay de amigos:** opt-in, com limite de banda padrão. Quem quer experiência melhor contribui mais. **Contribuição é medida de forma total** (carry, seed de arquivos, uptime, relay quando alcançável), para não punir quem está atrás de CGNAT. |
| D6 | **Relay comunitário com confiança local:** relays de usuários desconhecidos, ranqueados por qualidade **medida pelo próprio cliente** + endosso de amigos + antiguidade + diversidade de rede. **Sem reputação global** (vulnerável a Sybil). |
| D7 | **Privacidade de metadados (padrão):** confiança nos amigos. Uso de relay comunitário é **sinalizado na UI** e pode ser bloqueado pelo usuário. "Chamada protegida" (forçar relay para esconder IP) fica para depois. Cebola/multi-hop **fora do roadmap**. |
| D8 | **Conformidade ECA Digital (Lei 15.211/2025, "Lei Felca"):** lançamento público **somente para maiores de 18 anos**, com aferição de idade por **credencial assinada verificada pelos pares** (não por check local, pois o código é aberto). **Controle parental** entra no roadmap como fase futura. |
| D9 | **Credencial de idade ≠ sistema de confiança:** credencial é portão binário e determinístico; confiança de relay é score probabilístico. Compartilham só infraestrutura (chave Ed25519, gossip). |

---

## 2. Fase 7 — Conectividade de mídia sem servidor (ADR 0015)

### 7.0 Ajustes imediatos (podem entrar ainda no fim da Fase 6)

- **Tarefas**
  - Reduzir timeout da primeira tentativa da inversão de papéis de 10 s para ~7 s (seção 3.1 do estudo).
  - Melhorar mensagem final de falha (explicar causa provável e sugerir ação: Wi-Fi, IPv6, adicionar amigo-relay).
  - Heurística de detecção de CGNAT antes de ligar: IP público ≠ IP local, faixa `100.64.0.0/10`,
    resultado STUN. Usar para **começar já pela ordem certa** (lado CGNAT oferece/envia primeiro).
  - **Preferir candidatos IPv6** quando ambos os lados tiverem IPv6 global.
- **Aceite:** chamada 5G ↔ casa conecta na 1ª tentativa quando o lado CGNAT é detectado; tempo médio < 5 s.

### 7.1 Instrumentação e medição de campo (pré-requisito das decisões seguintes)

- **Tarefas**
  - Registrar por conexão (localmente; exportação **opt-in**, sem IPs, sem EndpointIds em claro):
    tipo de caminho (IPv6 direto / IPv4 direto / relay amigo / relay pool / relay n0), tipo de NAT
    inferido, tempo até conectar, RTT, perda, troca de caminho durante a chamada.
  - Medir **fração de usuários alcançáveis** (IPv6 com entrada liberada, IP público, UPnP/NAT-PMP/PCP).
  - Matriz de teste com 5–10 redes reais (fibras residenciais diferentes, 5G de operadoras diferentes,
    Wi-Fi público/corporativo).
- **Aceite:** relatório com % de casos resolvidos em cada degrau da escada.
- **Regra de decisão:** se alcançáveis ≥ ~20%, seguir com pool comunitário (7.5). Se muito abaixo
  (ex.: ~3%), priorizar `enxame-node` como forma principal de contribuição e rebaixar 7.5.

### 7.2 Mídia sobre o iroh (encanamento) — protótipo

- **Abordagem preferencial:** **TURN local "falso"** em `127.0.0.1` embutido no núcleo Rust
  (crate `turn` do webrtc-rs), configurado como `iceServers` no `flutter_webrtc`; o upstream do TURN
  são datagramas QUIC do iroh. Do outro lado, encaminhador UDP simples entregando ao libwebrtc.
  Objetivo: evitar reescrever candidatos ICE manualmente (libwebrtc suporta TURN nativamente).
- **Fallback de abordagem:** reescrita de candidatos ICE apontando para encaminhador local (seção 3.3 do estudo).
- **Tarefas**
  - Protótipo voz + vídeo entre dois dispositivos atrás de CGNAT.
  - Medir contra um **`iroh-relay` próprio no Brasil** (VM barata/free tier), não contra o n0, para
    isolar a arquitetura do rate limit e da distância (n0 tem relays só em EUA, Europa e Ásia).
  - Verificar se a subida relay → direto do iroh durante a chamada é transparente para o áudio.
  - Medir interação entre controle de congestionamento do QUIC e do WebRTC; MTU; jitter.
- **Aceite:** voz e vídeo entre dois CGNAT sem configuração do usuário; latência adicional < 80 ms
  com relay no Brasil; sem corte de áudio na troca de caminho.
- **Segurança:** TURN local escuta **somente loopback**, credenciais aleatórias por chamada, porta efêmera.

### 7.3 Punching avançado (depende de 7.2: núcleo controla o socket da mídia)

- **Tarefas**
  - **Pacote com TTL baixo** antes do pacote real (abre o mapeamento local sem chegar ao NAT remoto),
    atacando o "envenenamento" do CGNAT medido em campo.
  - **Birthday/multiporta** para NAT simétrico, como último recurso, com limites para não parecer varredura.
  - UPnP/NAT-PMP/PCP onde disponível (desligável).
- **Aceite:** taxa de conexão direta entre dois CGNAT cone > baseline de 7.1.

### 7.4 Relay embutido no `enxame-node` e no desktop (relay de amigo)

- **Tarefas**
  - `enxame-node` passa a servir relay para contatos (papel `SuperPeer`).
  - Desktop com **opt-in** "ajudar meus amigos como relay", apenas se alcançável; limite padrão de banda
    (valor inicial sugerido 5 Mbit/s, ajustar com dados de 7.1) usando `budget.*`/`RateLimiter`.
  - Publicar relays disponíveis no ticket/registro do contato.
  - **[VERIFICAR]** se o `iroh-relay` 1.4 exige URL HTTPS com certificado válido (domínio). Se sim,
    para voluntários domésticos usar o relay de aplicação de 7.5 em vez do `iroh-relay`.
- **Aceite:** chamada entre dois CGNAT via `enxame-node` de um amigo, sem configuração dos dois.

### 7.5 Relay de aplicação `enxame/relay/1` + pool comunitário com confiança local

- **Protocolo:** o relay R é um peer iroh alcançável; A e B abrem conexão iroh direta com R
  (autenticada pelo `EndpointId` de R); R repassa datagramas/streams num ALPN próprio
  `enxame/relay/1`. Identidade do relay = chave Ed25519 (mesma chave usada pela confiança).
  Sem domínio, sem certificado externo.
- **Score de confiança (local):**

  | Sinal | Fonte | Peso |
  |---|---|---|
  | Qualidade observada (RTT, perda, quedas, banda entregue) | medição própria em uso real | principal |
  | Endosso | scores medidos por amigos, recebidos por gossip | alto |
  | Antiguidade | tempo de histórico da chave | crescente, começa baixo |
  | Diversidade | máx. N relays por ASN / faixa de IP | limite duro |

- **Descoberta:** gossip pela rede de amigos (principal) + DHT mainline/pkarr (complementar, peso de desconhecido).
- **Tarefas:** protocolo, score, gossip, anúncio em DHT, troca de relay no meio da chamada em caso de
  degradação, UI sinalizando "relay comunitário" e opção de bloquear relays fora do círculo de amigos.
- **Aceite:** chamada de voz/vídeo via relay comunitário endossado; troca automática de relay sabotador/degradado.

### 7.6 Fallback n0 e degradação

- n0 como último degrau: **somente voz** (Opus); vídeo/tela desligados com aviso.
- Toggle "nunca usar relays públicos" (desligado por padrão).
- **Aceite:** com apenas duas pessoas em CGNAT e nenhum relay de amigo/pool, a voz conecta.

### 7.7 Incentivo por contribuição total

- Métrica de contribuição: carry, seed de arquivos, uptime, relay (quando alcançável).
- Benefícios de quem contribui mais: prioridade em relays de amigos/pool, mais qualidade de vídeo via relay.
- **Aceite:** usuário em CGNAT consegue pontuar contribuição sem ser relay.

### 7.8 (Posterior) Chamada protegida

- Opção de forçar mídia via relay confiável para não expor IP ao contato. Depende de 7.4/7.5.

### Fora do roadmap (registrar como alternativas descartadas no ADR 0015)

- Roteamento em cebola/multi-hop para chamadas (latência e banda inviáveis; ganho real só com ameaça
  de observador de rede). Revisitar só para mensagens/carry, se necessário.
- Reputação global de relays (Sybil).
- TURN hospedado/pago pelo projeto como padrão.
- Mídia própria sem WebRTC (reimplementa AEC/NS/controle de banda).

---

## 3. Fase 8 — Conformidade ECA Digital: somente maiores de 18 (ADR 0016)

> **Contexto legal (registrar no ADR, com data):** Lei 15.211/2025 em vigor desde 17/03/2026;
> Decreto 12.880/2026 regulamenta; ANPD é a agência responsável. Autodeclaração é vedada; aferição
> de idade ≠ coleta de identidade; dados de aferição só podem ser usados para essa finalidade.
> Sanções administrativas previstas a partir de nov/2026 e fiscalização formal a partir de jan/2027.
> Cadeia prevista: SO/lojas emitem "sinal de idade" via API; apps consomem, com contingência.
> **Bloqueante do lançamento público:** revisão por advogado de direito digital (enquadramento do
> projeto open source como "fornecedor", lista completa de obrigações, canal de denúncia).

### 8.1 Credencial de idade e portão nos pares

- **Formato** (sem dado pessoal):

  ```text
  AgeCredential {
    subject:    EndpointId      // vinculada à chave do usuário
    claim:      "18+"           // apenas faixa etária
    issuer:     IssuerId        // emissores aceitos: lista versionada no repo (auditável)
    issued_at, expires_at
    sig:        assinatura do emissor
  }
  ```

- **Portão:** cliente oficial **recusa** aceite de contato, entrada em grupo e início de chamada com
  quem não apresenta credencial válida, não expirada e não revogada. Revalidar na expiração.
- **Relays não exigem credencial** (não veem conteúdo; camada de rede neutra).
- **Interface plugável no núcleo:** `trait AgeAttestor` (verificação de assinatura, emissores aceitos,
  revogação), com **emissor de teste** para desenvolvimento.
- **Revogação:** lista assinada pelo emissor, distribuída por gossip.
- **Aceite:** cliente oficial não estabelece contato com identidade sem credencial; cliente modificado
  que não apresenta credencial fica isolado da rede oficial.

### 8.2 Emissor de aferição (decidir após revisão jurídica e parâmetros da ANPD)

- Avaliar: provedor terceiro de aferição que emita token vinculado à chave (preferência por ZKP /
  tokens não vinculáveis), sinal de idade do SO/loja quando existir no desktop (Windows; Linux
  provavelmente sem sinal), contingência quando o sinal faltar.
- Requisitos: o emissor não recebe o `EndpointId` ligado à identidade civil além do necessário;
  retenção mínima; nenhuma dessas informações é armazenada pelo Enxame além do token.
- **[VERIFICAR]** parâmetros normativos definitivos da ANPD sobre aferição (previstos a partir de ago/2026).

### 8.3 Transparência e UX

- Tela explicando o mecanismo de aferição, quais dados são usados e para quê (linguagem acessível).
- Fluxo de onboarding: instalar → validar idade (evento único) → usar P2P normalmente.
- Riscos residuais documentados (adulto que entrega identidade a menor; forks).

### 8.4 Builds oficiais assinados (pré-requisito do portão fazer sentido)

- Assinatura de código no instalador Windows, assinatura do AppImage, checksums publicados.
- **[VERIFICAR]** viabilidade de builds reprodutíveis na CI.
- Atualização automática apenas com pacotes assinados.

---

## 4. Fase 9 (futuro) — Controle parental (ADR futuro)

- Credencial com faixas (ex.: 12–15, 16–17) em vez de apenas "18+".
- **Chave do responsável co-assina** o aceite de contatos e a entrada em grupos do menor
  (supervisão parental nativa no modelo friend-to-friend, sem servidor).
- Configurações padrão protetivas para menores, limites de uso, painel do responsável.
- Reavaliar obrigações legais adicionais antes de iniciar (revisão jurídica obrigatória).

---

## 5. Segurança e privacidade transversal (ADR 0017 — modelo de ameaças)

Criar um **modelo de ameaças** consolidado e os itens abaixo como tarefas (cada um com [VERIFICAR] do estado atual):

| Item | Tarefa |
|---|---|
| S1. Autenticação da mídia | [VERIFICAR] fingerprints DTLS do SDP trafegam na sinalização `enxame/call/1` assinada e são validados; nenhum relay/TURN consegue fazer MITM. |
| S2. Sigilo futuro (forward secrecy) | [VERIFICAR] se envelopes de mensagem usam chave estática. Se sim, avaliar crates auditadas: `vodozemac` (1:1, Double Ratchet) e `openmls` (grupos, MLS RFC 9420). Motivo: ciphertext guardado por carriers não pode ser aberto se a chave vazar no futuro. |
| S3. Dados em repouso | [VERIFICAR] SQLite cifrado (ex.: SQLCipher) com chave no cofre do SO (Windows DPAPI/Credential Manager, Linux Secret Service). Chave privada Ed25519 nunca em texto plano no disco. |
| S4. Backup e perda de chave | Backup cifrado por passphrase da identidade; plano para vínculo multi-dispositivo (necessário para o mobile futuro). |
| S5. Metadados em relays | Conexões a relays comunitários com **chave iroh efêmera por sessão** (relay não vê a identidade de longo prazo); só o relay de amigo vê a identidade real. |
| S6. Gossip de confiança | Compartilhar scores agregados e grosseiros, sem timestamps, com atraso, para não revelar a amigos quando/com quem houve chamadas. |
| S7. Abuso de relays | Budget e `RateLimiter` obrigatórios; limites por conexão e por identidade; corte de fluxos ociosos. |
| S8. TURN local | Somente loopback, credenciais aleatórias por chamada (ver 7.2). |
| S9. Telemetria | Somente local por padrão; exportação opt-in, sem IPs nem EndpointIds em claro. |
| S10. Credencial de idade | Sem dado pessoal; lista de emissores versionada e assinada; rotação de chave de emissor via release assinada. |
| S11. Cadeia de suprimentos | `cargo audit`/`cargo deny` na CI, pin de dependências, revisão de crates de criptografia. |
| S12. Denúncia de abuso | [VERIFICAR com jurídico] mecanismo de denúncia iniciado pelo usuário (encaminhar mensagem assinada pelo remetente como prova), compatível com E2E. |

---

## 6. Dependências entre blocos

```text
7.0 ──► 7.1 ──► (decide prioridade de 7.5)
7.2 ──► 7.3
7.2 ──► 7.4 ──► 7.5 ──► 7.7
7.2 ──► 7.6
7.4/7.5 ──► 7.8
8.4 ──► 8.1 ──► 8.2 ──► 8.3 ──► [revisão jurídica] ──► lançamento público
S1, S3 ──► lançamento público
Fase 8 ──► Fase 9
```

## 7. ADRs a abrir

- **0015 — Conectividade de mídia sem servidor:** D2–D7, escada de conexão, protocolo `enxame/relay/1`,
  confiança local, alternativas consideradas (todo o estudo) e descartadas.
- **0016 — Aferição de idade (somente 18+):** D8–D9, credencial, portão nos pares, contexto legal datado,
  riscos residuais, dependência de revisão jurídica.
- **0017 — Modelo de ameaças e segurança de dados:** seção 5.
