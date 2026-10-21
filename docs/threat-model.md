# Modelo de ameaças (rascunho — Fase 1)

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
