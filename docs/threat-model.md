# Modelo de ameaças (rascunho — Fases 1 a 4)

Detalha a tabela da seção 12 da spec. Cobre só o que existe até a Fase 1 (identidade, contatos,
conectividade); mensagens, carry e arquivos entram nas fases seguintes.

| Ameaça | Mitigação | Limitação conhecida |
|---|---|---|
| Ticket forjado ou adulterado | Assinatura Ed25519 da identidade sobre o ticket e `DeviceCert` | — |
| Ticket trocado no caminho (MITM na troca do convite) | Fingerprint mostrada ao aceitar | O ticket não é segredo e quem o intercepta antes de B pode se passar por A. Confirmar a fingerprint por outro canal. |
| Ticket reusado após o prazo | `expires_at`, teto de 30 dias | Não há revogação; o ticket só permite *pedir* contato |
| Spam de pedidos de contato por quem conhece o NodeId | Teto de 16 pendentes, descarte sem resposta humana, timeout | Um nó pode manter 16 pedidos abertos por um tempo (DoS leve) |
| Desconhecido abre `enxame/sync/1` | Conexão fechada com `403` antes de qualquer dado | O desconhecido aprende que o NodeId existe e está online |
| Dispositivo de outra identidade usando um Hello | `DeviceCert` verificado contra o contato (identidade e NodeId) | — |
| Exposição de IP | Só contatos mútuos; o ticket publica endereços do emissor para quem o receber | Quem recebe um ticket aprende os IPs do emissor |
| Chave de identidade em disco | Keystore do SO (`os-keyring`); fallback em arquivo `0600` | Em Linux sem Secret Service o fallback é um arquivo legível pelo usuário |
| Relay vê metadados | Conteúdo é E2E (QUIC/TLS do iroh entre dispositivos) | O relay vê NodeIds, tamanhos e horários |
| Relay próprio com CA privada | `relay_ca_file` confia *apenas* nessa CA | Substitui as raízes públicas para o relay |
| Mensagem forjada ou adulterada | `id` por hash + assinatura Ed25519; cabeçalho como AAD do conteúdo | — |
| Terceiro injeta mensagem na conversa | Só autor = par ou eu, só conversas compartilhadas, só o que foi pedido (`Want`) | — |
| Reordenação/replay | Deduplicação por `id`; ordem por `parents` + HLC | Um autor controla o próprio HLC (limite: 24 h no futuro) |
| Envelope gigante / flood na sessão | Limites de tamanho, de pais e de destinatários, frame máximo, páginas de 1000 ids, timeout de 30 s | — |
| Vazamento do disco | Chaves no keystore do SO | O banco com as mensagens **não é cifrado em repouso** (confie na cifragem de disco do SO) |
| Chave de encriptação comprometida | — | Sem forward secrecy: expõe todo o histórico cifrado para essa chave (MLS pós-MVP) |
| Metadados | — | Quem relaia/guarda vê `recipient`, tamanhos e horários dos envelopes; o par vê o HLC |
| Carrier lê a mensagem que carrega | Envelope cifrado por destinatário (E2E) | O carrier vê `recipient`, tamanho, horário e `author_device` |
| Flood de carry / encher o disco de um carrier | Só contatos mútuos; só mensagens do próprio ofertante; quota por contato e total por papel; TTL (7 ou 30 dias); `Leaf` não carrega | Um contato mal-intencionado consome a sua quota por contato |
| Contato malicioso expurga mensagens de outro | Recibo vale só para mensagens destinadas ao **autor do recibo** (`recipient = autor`) | — |
| Contato forja "entregue" para que o autor pare de tentar | Recibo é assinado pelo destinatário e verificado | Um recibo legítimo atrasado só deixa de pendurar a mensagem |
| Carrier entrega mensagem alterada ou de outro autor | O destinatário reverifica `id` e assinatura, exige que o autor seja contato mútuo e a conversa seja a 1:1 | — |
| Carrier retém/descarta a mensagem | Até 3 carriers (`K = 3`); o autor continua entregando direto quando o destinatário voltar | Carry é "melhor esforço": sem garantia de entrega |
| Vazamento da agenda para descobrir contatos em comum | Só hashes salgados com a conversa 1:1; só a interseção é reconhecível | Quem já conhece uma identidade e a conversa pode testar se ela está na agenda do par |
| Bug ou nó mal-intencionado derruba a rede | Papel é decisão local, sem estado global; promoções lentas; protocolos versionados | — |
| Nó headless aceitando qualquer um | `allowed_fingerprints`; sem lista, recusa tudo | `auto_accept_contacts` abre o nó a quem tiver o ticket (uso controlado) |
| Relay do nó / contêiner comprometido | Volume `/data` guarda a identidade do nó; usuário sem privilégios no contêiner | Quem lê o volume assume a identidade do nó (faça backup e proteja) |
| Conteúdo de arquivo exposto a quem o hospeda ou carrega | Cifrado em streaming com chave aleatória por arquivo; a chave só viaja dentro do envelope E2E | Providers e carriers veem o hash, o tamanho do ciphertext e quem pede |
| Estranho baixa um blob | Só contatos mútuos conectam ao provedor; `get_many` e `push` desligados; a raiz de 32 bytes é uma capacidade | Um contato que conhece a raiz (recebeu o `FileRef`) pode baixá-la de qualquer outro contato que a tenha |
| Arquivo adulterado ou truncado no caminho | BLAKE3 por segmento e pela raiz (`iroh-blobs`) + autenticação por bloco com bit "último" e contador | — |
| Nome de arquivo malicioso (`../../x`) | Nome só para exibição; ao gravar é saneado e o arquivo nunca sai da pasta de destino nem sobrescreve | — |
| Arquivo gigante para encher o disco | `max_file_mb` na recepção e no envio; carrier recusa acima de `prefetch_max_mb` e da quota; download cortado se passa do tamanho declarado | O destinatário escolhe baixar; o espaço do próprio blob replicado conta na quota |
| Contato usa o nó como CDN | Papel e budget (GB/mês e kbps) valem para o que não é do próprio dono; limite de banda por pedaço | Arquivos que eu mesmo enviei não contam no budget (mas passam pelo limite de banda) |
| Sondagem revela o interesse num arquivo | Só contatos mútuos recebem a sondagem | Cada contato online vê que você buscou aquele hash |
| Estranho se declara dono de um grupo | `group_id` = hash de `(criador, nonce)`; retrato só vale se o autor do envelope é o `creator`; só entra pelo 1:1 do criador, que é contato mútuo | — |
| Membro tenta mudar a lista de membros | Só o criador emite retratos; retrato de outro autor é descartado | O criador é um ponto único de confiança: se for malicioso, controla quem entra |
| Removido continua lendo | Mensagens novas são seladas só para os membros do retrato novo | Até cada membro receber o retrato novo, ele ainda sela para o removido; o que ele já recebeu continua legível (sem forward secrecy) |
| Mensagem de quem não é membro | Autor tem de constar no retrato ativo (sincronização e carry) | Mensagem legítima de alguém removido que chega depois da remoção é descartada |
| Exposição de IP entre membros do grupo | `Event::GroupJoined` lista os membros que não são contatos; a CLI avisa | Quem entra num grupo passa a ser alcançável por todos os membros (spec 7.2) |
| Membro novo lê o passado | Mensagens antigas foram seladas só para os membros de então; não há re-cifragem | — |
| Flood de membros em carry | Mesmos limites de quota por ofertante, total por papel e TTL do carry 1:1 | Membro de grupo que não é contato pode ofertar carry só de mensagens de grupo |
