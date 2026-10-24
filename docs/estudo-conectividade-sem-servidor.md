# Enxame: visão geral e estudo de alternativas de conectividade sem servidor

Documento de estudo (2026-10-24). Parte 1 dá o contexto do app. Parte 2 descreve o problema que
apareceu nos testes de campo. Parte 3 lista **todas as alternativas discutidas**, com prós e contras.
Parte 4 compara, recomenda e propõe o que medir antes de decidir. Nada aqui está implementado além do
que a Parte 1 e a seção 3.1 dizem; é material para decisão (e base para o ADR da Fase 7).

---

## Parte 1. O que é o Enxame

### 1.1 Em uma frase

Um comunicador estilo Discord **sem servidor central obrigatório**: cada pessoa instala o app, adiciona
amigos por ticket, e tudo (mensagens, arquivos, grupos, chamadas de voz/vídeo/tela) vai direto entre os
dispositivos, cifrado de ponta a ponta. Enquanto está em uso, cada dispositivo também **ajuda a rede dos
seus contatos** (guarda mensagens cifradas de quem está offline, semeia arquivos).

### 1.2 Princípios

- **Local-first**: o banco local (SQLite) é a fonte da verdade; tudo funciona offline.
- **Friend-to-friend**: só contatos aceitos dos dois lados trocam tráfego e veem o IP um do outro.
- **E2E sempre**: quem carrega ou repassa vê só texto cifrado.
- **Setup simples** (ADR 0008): o usuário básico não abre terminal, não configura servidor nem variável
  de ambiente. Relays públicos do iroh por padrão; relay/nó próprio é opt-in.
- **Contribuição visível e limitada**: papel automático por contexto (rede, energia, NAT), com budget de
  banda definido pelo usuário. Quem não pode contribuir (4G, bateria baixa, modo jogo) mantém a
  experiência básica completa.
- **Sem criptografia inventada**: só composição de crates auditadas.

### 1.3 Como funciona (resumo técnico)

| Camada | O que faz |
|---|---|
| Identidade | Par de chaves Ed25519 por pessoa. O `EndpointId` do iroh é a chave pública. Contato = troca de ticket (`enxame1...`) aceita dos dois lados. |
| Transporte | **iroh 1.4** (QUIC/TLS 1.3 sobre UDP). Tenta caminho direto (hole punching); cai para relay público do iroh quando não consegue. Descoberta por DNS do n0 e mDNS na LAN. |
| Mensagens | Envelopes selados e assinados, ordem por relógio híbrido (HLC), sincronização entre pares, entrega offline por *carry*. |
| Papéis | `Leaf` / `LeafPlus` / `Peer` / `SuperPeer`, escolhidos automaticamente, com histerese. Quem tem rede boa e energia guarda e entrega para os outros. |
| Arquivos | Conteúdo cifrado em fluxo (XChaCha20-Poly1305), blobs de 8 MiB via `iroh-blobs`, download de várias fontes em paralelo, retomada. |
| Grupos | Retrato assinado pelo criador, chave de conteúdo selada para cada membro. |
| Chamadas | Sinalização própria `enxame/call/1` (sobre o iroh, E2E) e mídia por **WebRTC** (`flutter_webrtc`, DTLS-SRTP). Voz, vídeo e tela. |
| App | Flutter (Linux e Windows), núcleo em Rust via `flutter_rust_bridge`. Instalador Inno Setup (Windows) e AppImage (Linux) pela CI. |
| Nó headless | `enxame-node`: daemon 24/7 com papel fixo `SuperPeer` (Docker em `deploy/`), opcional. |

### 1.4 Onde estamos

- Fases 0 a 5 implementadas e validadas em campo (identidade, mensagens, papéis/carry, arquivos, grupos).
- Fase 6 (app Flutter) em andamento; bloco 6.6 (Windows) quase fechado: mensagens, arquivo, voz, vídeo
  e tela funcionam Windows ↔ Linux; bandeja e "Sair" corrigidos.
- Fase 7 (planejada): relay/TURN próprio e endurecimento. **É aqui que entra este estudo.**

### 1.5 O que o servidor já é hoje (e o que não é)

- **Relays e DNS do iroh (n0)**: ajudam a achar o par e a conectar quando o caminho direto falha. Veem
  só metadados (quem fala com quem, quando, volume), nunca conteúdo.
- **STUN do Google/Cloudflare**: só para a chamada descobrir o endereço público (não carrega mídia).
- **Nenhum servidor nosso**. Nenhum servidor guarda conta, mensagem ou arquivo.

---

## Parte 2. O problema

### 2.1 Sintoma

Mensagens e arquivos sempre funcionam (o iroh tem relay como rede de segurança). **Chamadas** usam WebRTC
com ICE e só STUN: se o hole punching UDP direto falha, **não há relay para a mídia** e a chamada não
conecta.

### 2.2 O que medimos (Windows no 5G do celular ↔ Linux em casa)

- Os dois NATs têm mapeamento independente do destino (tipo *cone*), o que em tese permite furar.
- O **CGNAT móvel** muda a porta a cada sessão e expira o mapeamento em ~30 s de ociosidade.
- Se um pacote **chega** ao CGNAT antes de ele ter **enviado** algo para aquele par, o fluxo fica
  "envenenado". Só funciona se o **lado atrás do CGNAT enviar primeiro**.
  - Windows (CGNAT) enviando primeiro: funciona.
  - Linux enviando primeiro, ou os dois ao mesmo tempo: falha.
- **Dois dispositivos atrás de CGNAT** não conectam sem um intermediário.

### 2.3 O que já mitiga (Seção 3.1)

Inversão de papéis automática com memória por contato: primeira chamada ~14 s, as seguintes ~4 s. Funciona
em 5G ↔ casa. Não resolve dois CGNAT, e a primeira chamada ainda demora.

### 2.4 Restrição de produto

O app **não pode depender de um servidor que alguém precise hospedar e pagar** (proposta do projeto), e o
usuário básico não configura nada. Então "subir um TURN" não é resposta por si só: falta responder **quem
hospeda**.

---

## Parte 3. Alternativas

Legenda de esforço: **P** pequeno (dias), **M** médio (1–3 semanas), **G** grande (mais que isso).

### 3.1 Status quo + inversão de papéis (já implementado)

Tenta uma ordem de oferta; se não conectar em 10 s, inverte quem oferece e tenta de novo (25 s). Lembra,
por contato, qual ordem funcionou.

| Prós | Contras |
|---|---|
| Já funciona e foi validado em campo. | Primeira chamada com CGNAT demora ~14 s. |
| Zero servidor, zero configuração. | Dois CGNAT: falha, e demora a avisar. |
| Sem custo de banda de terceiros. | Depende de o mapeamento do CGNAT ser tipo cone; operadoras com NAT simétrico não funcionam. |
| Mídia continua direta (menor latência). | É paliativo: não generaliza para redes corporativas e Wi-Fi público. |

Esforço: feito. Pendência: baixar o primeiro timeout de 10 s para ~7 s, melhorar a mensagem final.

### 3.2 TURN tradicional (coturn)

Servidor que repassa a mídia UDP/TCP quando o direto falha. É o padrão do WebRTC.

Quem hospeda? Variantes:

| Variante | Prós | Contras |
|---|---|---|
| **a) TURN do projeto (nosso VPS)** | Simples para o usuário; comportamento previsível. | Contraria "sem servidor"; custo mensal e de banda; ponto único de falha e de abuso; precisamos de credenciais temporárias e de quota. |
| **b) TURN de terceiros pago** (Cloudflare, Twilio, Metered...) | Rápido de integrar, global, sem operar. | Custo por GB; dependência de empresa externa; chaves precisam ficar em algum lugar (se no app, qualquer um as extrai). |
| **c) TURN próprio de cada usuário** (opt-in, `deploy/`) | Respeita a filosofia; quem quer, tem. | Usuário básico não configura; só ajuda quem tem VPS. |
| **d) TURN em nó de amigo** (o `enxame-node` já existe) | Reaproveita a estrutura de papéis. | Exige coturn embutido ou reimplementação; amigo precisa de IP público ou porta aberta; ver 3.5. |

Contras gerais: o TURN vê metadados e fluxo cifrado (DTLS-SRTP, então não vê conteúdo); relay UDP consome
muita banda (tela = vários Mbit/s); credenciais e abuso.

Esforço: P para integrar um TURN existente, G para resolver "quem hospeda" de forma coerente.

### 3.3 Mídia sobre o próprio iroh (encaminhador UDP local)

Em vez de a mídia do WebRTC sair pelo NAT por conta própria, o app abre um **encaminhador UDP local**: o
libwebrtc fala com `127.0.0.1:porta`, e o núcleo manda os pacotes por **datagramas QUIC do iroh**, que já
sabem usar caminho direto ou relay e fazem a subida automática relay → direto.

Como: candidatos ICE reescritos para o endereço do encaminhador; o iroh vira o "transporte" do WebRTC.

| Prós | Contras |
|---|---|
| Reaproveita a conectividade que já funciona (a mesma das mensagens). | Hack no ICE: depende de o libwebrtc aceitar candidatos locais "falsos" (**incógnita**; precisa protótipo). |
| Sem servidor novo: usa os relays do iroh que já existem. | Relays públicos do n0 têm limite de banda e não foram feitos para vídeo; abuso pode ser limitado por eles. |
| Mantém E2E duplo (DTLS-SRTP dentro do QUIC/TLS). | Camada extra: QUIC sobre UDP sobre QUIC; MTU, jitter e controle de congestionamento do QUIC podem brigar com o do WebRTC. |
| Funciona para dois CGNAT (via relay). | Dependência continuada do n0 para casos sem caminho direto. |
| Uniformiza: tudo passa pelo mesmo caminho e diagnóstico. | Esforço de engenharia relevante; precisa de testes em rede ruim. |

Variante: **abandonar WebRTC e fazer mídia própria** sobre iroh (codec + jitter buffer + eco). Evita o
hack, mas reimplementa o que o libwebrtc dá pronto (AEC, NS, controle de banda). **G**, não recomendado.

Esforço: M para protótipo, G para produção.

### 3.4 Relay hospedado por amigos (nível 1) e além

Idéia: cada pessoa tem um **relay de casa** (o iroh já tem esse conceito: o relay onde o nó fica sempre
conectado). Em vez de depender só do relay público, o app pode usar **nós de contatos** como relay.

Níveis de confiança/alcance:

| Nível | Quem pode ser relay | Prós | Contras |
|---|---|---|---|
| **1. Amigos** | Só contatos diretos | Confiança social; baixo risco de abuso; escala com o grafo de amigos. | Rede pequena = poucos relays; amigo precisa estar online e ter banda; quem tem só amigos em CGNAT não tem relay bom. |
| **2. Amigos de amigos** | Contatos dos contatos | Mais candidatos. | Reveal de grafo social a quem não é seu amigo; confiança diluída; mais superfície de abuso. |
| **3. Pool público voluntário** | Qualquer usuário do Enxame que ofereça | Escala a rede sem servidor; qualquer um ajuda. | Metadados expostos a desconhecidos; abuso e DoS; precisa de quotas, reputação e descoberta (DHT); banda de voluntário é instável. |

Pontos que valem para os três:

- Para ser relay, o nó precisa ser **alcançável** (IP público/IPv6/porta aberta/UPnP). A maioria dos
  usuários domésticos não é, o que limita muito quem pode servir.
- O relay precisa de **política de banda** (já temos `budget.*` e `RateLimiter` dos papéis).
- Combina com o papel `SuperPeer`: o nó headless já é um candidato natural.
- O iroh permite relay customizado por endpoint, mas a **descoberta do relay do par** passa pelo
  DNS/pkarr; seria preciso publicar o relay escolhido em algum lugar (no ticket, no registro DHT).

Esforço: M (nível 1), G (níveis 2 e 3: confiança, descoberta, abuso).

### 3.5 Criptografia ponta a ponta com relay não confiável

Pergunta: se só as duas pontas têm a chave, qualquer relay serve?

- **Conteúdo**: sim. O relay vê só bytes cifrados (iroh QUIC/TLS, mensagens seladas, DTLS-SRTP na mídia).
- **O que o relay ainda vê**: IPs de origem e destino, horários, volume e padrão de tráfego (dá para
  inferir "está em chamada de vídeo"), e o par `EndpointId` ↔ `EndpointId`.
- **O que o relay pode fazer**: descartar, atrasar, reordenar (mas não forjar nem ler). Relay hostil pode
  degradar a chamada de propósito.

Conclusão: E2E torna viável usar relays de desconhecidos **para conteúdo**, mas o custo passa a ser de
**metadados** e disponibilidade, não de segredo. Para um app de amigos, isso é aceitável nos níveis 1–2 e
precisa de aviso claro no nível 3.

### 3.6 Roteamento em múltiplos saltos (cebola)

Ideia: usuário → relay 1 → relay 2 → ... → usuário, cada relay conhecendo só o próximo salto; relays
escolhidos por proximidade do destino para não aumentar demais a latência.

| Prós | Contras |
|---|---|
| Nenhum relay sozinho liga origem e destino. | **Latência**: cada salto soma ao menos um RTT; chamada precisa de ~150 ms ida; 3 saltos entre cidades distantes estouram. |
| Esconde seu IP do contato (ele vê o último salto). | **Banda multiplicada**: tela de 8 Mbit/s vira 24 Mbit/s consumidos, pagos por voluntários. |
| Resiste a um relay malicioso. | **Escolher por proximidade do destino enfraquece a privacidade**: o último salto revela onde o destino está. Privacidade quer saltos *sem relação* com as pontas; latência quer o oposto. |
| Dá para somar caminhos (resiliência). | **Correlação de tempo/volume** entre entrada e saída continua possível sem tráfego falso. |
| Reaproveita E2E e identidade. | **Rede pequena = pouco anonimato**: com poucos relays, o conjunto de suspeitos é pequeno. |
|  | Complexidade enorme: circuitos, chaves por salto, falha no meio, remontagem de caminhos paralelos. Projeto do tamanho do app. |
|  | Tor não transporta UDP; Lokinet e Nym transportam, mas com custo e maturidade menores. |

Avaliação: em rede de **amigos**, quem vê os metadados já é gente que você conhece; o ganho real só
aparece com relays desconhecidos e ameaça de observador de rede. Para o objetivo atual (falar com amigos
sem servidor), um ou dois relays bastam.

Se um dia for necessário: **adotar rede pronta** (Lokinet, Nym) em vez de construir; ou restringir
multi-hop a mensagens e *carry* (latência irrelevante), nunca a chamadas.

Esforço: G. Não recomendado agora.

### 3.7 Modo "chamada protegida" (via relay, nunca direta)

Opção do usuário que força a mídia pelo relay para não expor o IP ao contato (como o Signal faz).

| Prós | Contras |
|---|---|
| Barato: reaproveita o relay da 3.3 ou 3.2. | Latência e banda do relay. |
| Entrega a maior parte do ganho de privacidade de IP da cebola. | Só vale se existir relay confiável e com banda (volta ao problema da Parte 2.4). |

Esforço: P depois que existir 3.2 ou 3.3.

### 3.8 Tornar o hole punching mais esperto (sem relay)

Variações para aumentar a chance de conexão direta:

| Técnica | Prós | Contras |
|---|---|---|
| **Coordenar quem envia primeiro** (já testado: o lado do CGNAT envia primeiro) | Resolve 5G ↔ casa na primeira tentativa se soubermos o lado certo. | Não sabemos qual lado é CGNAT a priori (inferir pelo IP público ≠ IP local e por faixa 100.64/10 ajuda parcialmente); dois CGNAT continuam sem solução. |
| **Previsão de porta / "birthday"** para NAT simétrico | Cobre alguns NATs simétricos. | Frágil, lento, muitas portas, pode ser tratado como varredura pela operadora. |
| **IPv6** | Sem NAT na maioria das redes que têm; ligação direta fácil. | Cobertura desigual; firewall residencial costuma bloquear entrada; precisa funcionar em conjunto com o resto. |
| **UPnP / NAT-PMP / PCP** | Abre porta de verdade em muitos roteadores domésticos. | Não existe em CGNAT; pode estar desligado; riscos de segurança do UPnP. |
| **Sondagem do tipo de NAT antes de ligar** e escolha do papel | Evita esperar 10 s à toa. | Heurístico; só reduz o tempo, não resolve a classe sem relay. |

Esforço: P a M cada. São complementos, não substitutos de um relay.

### 3.9 Outras fontes de relay/descoberta

- **Relay do projeto, em `deploy/` (opt-in)**: já existe o pacote Docker. Bom para quem quer; não resolve
  o usuário básico.
- **DHT para descoberta** (pkarr/mainline do iroh): reduz dependência do DNS do n0 para achar pares;
  independente do problema de mídia.
- **Rede pronta como transporte** (Tor/Lokinet/Nym/libp2p relay): evita construir, mas adiciona
  dependência pesada e perfil de latência ruim.
- **libp2p circuit relay v2**: modelo parecido com 3.4; trocar a pilha seria reescrever o transporte.

### 3.10 Aceitar a limitação e comunicar bem

Manter 3.1 + melhor diagnóstico/mensagem, e documentar "dois celulares em 5G podem não conseguir chamar;
use Wi-Fi". Zero custo, zero dependência, mas é uma falha de produto para uma parcela real dos usuários.

---

## Parte 4. Comparação e recomendação

### 4.1 Quadro-resumo

Nota: ○ ruim, ◐ médio, ● bom. "Sem servidor" = não exige infraestrutura nova mantida por alguém.

| Alternativa | Resolve 2 CGNAT | Sem servidor | Latência | Setup do usuário | Privacidade | Esforço |
|---|---|---|---|---|---|---|
| 3.1 Inversão de papéis (atual) | ○ | ● | ● | ● | ● | feito |
| 3.2a/b TURN do projeto / pago | ● | ○ | ◐ | ● | ◐ | P–M |
| 3.2c TURN próprio opt-in | ◐ (só quem hospeda) | ● | ◐ | ○ | ◐ | P |
| 3.3 Mídia sobre iroh | ● | ◐ (relays do n0) | ◐ | ● | ◐ | M–G |
| 3.4 Relay de amigos (nível 1) | ◐ | ● | ◐ | ● | ● | M |
| 3.4 Pool público (nível 3) | ● | ● | ◐ | ● | ○ | G |
| 3.6 Cebola | ● | ● | ○ | ● | ● | G |
| 3.7 Chamada protegida | ● (se houver relay) | depende | ◐ | ● | ● | P (depois) |
| 3.8 Punching esperto | ○ | ● | ● | ● | ● | P–M |

### 4.2 Recomendação (a validar com protótipo)

Em camadas, da mais barata à mais complexa, parando quando o campo mostrar que basta:

1. **Agora**: manter 3.1; baixar o timeout inicial; melhorar a mensagem final. Somar a heurística de 3.8
   (descobrir quem está atrás de CGNAT e já começar pela ordem certa).
2. **Fase 7, primeiro passo**: protótipo de **3.3 (mídia sobre iroh)** usando os relays do n0 como
   padrão. É o único caminho que cobre dois CGNAT **sem** infraestrutura nossa e sem configuração do
   usuário. Se o libwebrtc não aceitar o encaminhador, cair para 3.2b com TURN de terceiros como
   opção de última instância (documentando o custo).
3. **Depois**: **3.4 nível 1** (relays de amigos) para tirar carga do n0 e dar independência real; o
   `enxame-node` já é candidato.
4. **Opcional**: **3.7** como recurso de privacidade, quando houver relay.
5. **Não fazer agora**: 3.6 (cebola), 3.4 nível 3. Revisitar se o app ganhar rede pública aberta.

### 4.3 O que medir no protótipo (perguntas em aberto)

- O libwebrtc aceita candidatos ICE apontando para um encaminhador local e mantém DTLS-SRTP estável?
- Qual a vazão e a perda dos relays públicos do n0 com um fluxo de vídeo de 2–8 Mbit/s? Há limite por
  conexão?
- A subida relay → direto do iroh acontece durante a chamada, e o encaminhador acompanha sem cortar o
  áudio?
- Jitter e atraso adicionais do QUIC por baixo do WebRTC; interação entre os controles de
  congestionamento.
- Quantos usuários reais têm IPv6 funcional ou UPnP aberto (reduz a necessidade de relay)?
- No caso de 3.4: quantos contatos típicos são alcançáveis (IP público/IPv6) e com banda?

### 4.4 Decisões que dependem de você

1. O produto aceita **depender dos relays públicos do iroh** para mídia no pior caso (com a opção de
   relay próprio)? Se não, só sobram 3.4 e 3.2c.
2. Quanto de **banda de terceiros** é aceitável pedir a amigos (limite padrão, opt-in ou opt-out)?
3. Nível de privacidade pretendido para **metadados**: basta confiança nos amigos, ou o produto promete
   algo contra observadores de rede?
4. Aceitar **degradação** (somente voz) quando só houver relay?

### 4.5 Próximo passo sugerido

Escrever o ADR 0015 (Fase 7: relay de mídia sem servidor), incluindo esta comparação como "alternativas
consideradas", e abrir o protótipo de 3.3 com critérios de aceite: chamada de voz e vídeo entre dois
dispositivos em CGNAT, latência adicional menor que 80 ms, sem configuração do usuário.
