# 0006 — Decisões da Fase 3 (papéis, carry e nó headless)

Status: aceito

## Contexto

A Fase 3 entrega `roles`, `carry`, o nó headless e o guarda-corpo Android. A spec (seções 8 e 9)
define as regras; aqui ficam as escolhas que ela deixou em aberto.

## Decisões

1. **Regras de papel** exatamente como na spec 8.3, em função pura (`target_role`) e testada em
   tabela. **Sinal desconhecido é conservador**: enlace `Unknown` limita o papel a `Peer`
   (não se faz relay pesado sem saber o enlace); `metered` desconhecido conta como não medido
   (senão todo desktop sem NetworkManager ficaria `LeafPlus`); `upload_kbps_est` ausente impede
   `SuperPeer` (não há speedtest ativo).
2. **Histerese** em `RoleEngine`, com o tempo injetado: promoção após `promote_after` estável
   (120 s; 600 s para `SuperPeer`; configurável), qualquer queda é imediata, e mudar o papel-alvo
   enquanto espera reinicia o relógio. O nó nasce em `Leaf`. Papel fixo (`role = "super_peer"`)
   ignora sinais e histerese.
3. **Sinais Linux** sem dependências novas: `/sys/class/power_supply`, `/proc/net/route`,
   `/sys/class/net/*/wireless` e `nmcli` (NetworkManager) para `metered`; CPU/memória via
   `sysinfo`. Outras plataformas caem em `FallbackSignals` (enlace desconhecido ⇒ máximo `Peer`)
   até haver implementação própria (Windows na Fase 6; Android em M1).
4. **Upload estimado passivamente** (EWMA com α = 0,2) sobre envios reais de ≥ 16 KiB; sem
   amostra não há estimativa. O teto de banda padrão é 30 % da estimativa (`max_contrib_kbps`
   sobrepõe). O limitador (balde de fichas) hoje só regula o `Deliver` do carry; os blobs o usarão
   na Fase 4.
5. **Budget**: ao atingir `max_contrib_gb_month` o papel cai para `LeafPlus` até o mês seguinte
   (`contribution_stats` por mês e por contato). As quotas de carry vêm do papel e podem ser
   sobrepostas por `carry_quota_mb_*`.
6. **`RoleUpdate`** é uma mensagem do `sync` e só alcança contatos com sessão aberta (as sessões
   são curtas por desenho); os demais leem o papel no `Hello`. Para isso o leitor da sessão
   virou uma tarefa com canal, que também torna a leitura cancelável.
7. **Contatos em comum sem vazar a agenda**: cada lado envia hashes `H(conversa 1:1 ‖ contato)`.
   Só a interseção é reconhecível. É o que permite escolher carriers sem uma "lista de contatos"
   pública.
8. **Carry em ALPN próprio** (`enxame/carry/1`), separado do `sync`: os envelopes carregados não
   pertencem a nenhuma conversa em comum entre carrier e destinatário, então não passam pela
   reconciliação por conversa. O carrier guarda `raw` + destinatário + TTL em `carry_queue`.
9. **Escolha de carriers**: contatos em comum com o destinatário, ordenados por papel conhecido
   (`SuperPeer > Relay > Peer > LeafPlus`; `Leaf` nunca) e, dentro do papel, pela taxa de sucesso
   de conexões (`contact_reliability`, o "uptime histórico" aproximado); no máximo 5 tentativas e
   `K = 3` carriers distintos com cópia. "Online" = a conexão abre.
10. **Recibo** = envelope `kind = Receipt` com os ids em `parents` (claro), fora do DAG da
    conversa. Só vale para mensagens destinadas ao **autor do recibo**, então um contato malicioso
    não consegue expurgar mensagens alheias. Também é emitido para o que chega por `sync` direto,
    para carriers que ainda guardam cópia expurgarem.
11. **Entrega pendente** (`pending_delivery`): toda mensagem minha nasce pendente e sai da lista
    quando uma sessão de sincronização com o destinatário termina bem ou quando chega um recibo
    dele. `Message.delivered` expõe isso na UI/CLI.
12. **Política de contatos do nó headless**: sem humano para responder, `allowed_fingerprints`
    (e, para ambientes controlados, `auto_accept_contacts`). O resto é recusado com aviso no log.
    O ticket do nó é regravado a cada hora em `<data>/node-ticket.txt`; o healthcheck do Docker é
    um arquivo atualizado a cada 10 s (sem porta HTTP).
13. **Android**: `cargo check -p enxame-core --target aarch64-linux-android` verificado localmente
    com o NDK 28 e no CI. O keystore do SO fica atrás do feature `os-keyring` (fora do padrão) e
    nenhuma API só-desktop entra no core.

## Consequências

- Sem recibo, uma cópia dura o TTL do carrier (7 ou 30 dias); sem carry, a mensagem espera o
  contato voltar (sync direto) como na Fase 2.
- Um carrier vê o destinatário (`recipient`), o tamanho e o horário dos envelopes que guarda; o
  conteúdo segue cifrado de ponta a ponta.
- Os carriers só conhecem o que o autor lhes oferece: não há replicação entre carriers.
