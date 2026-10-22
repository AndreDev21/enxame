# Política de segurança

## Reportar uma vulnerabilidade

**Não abra uma issue pública.** Use o *Security Advisory* privado do GitHub do repositório
(aba *Security* → *Report a vulnerability*). Inclua o que foi afetado, como reproduzir e o impacto
que você enxerga. Responderemos em até alguns dias e coordenaremos a correção e a divulgação.

## Estado atual (leia antes de confiar)

O Enxame é um **MVP sem auditoria externa**. Não o use para proteger informação de alto risco.

- **E2E sempre**: carriers e relays só veem texto cifrado, mas **metadados** (destinatário,
  tamanho, horário, IP entre contatos) não são ocultados.
- **Sem forward secrecy** no MVP: vazamento da chave de cifragem de um dispositivo expõe o
  histórico cifrado para ela (MLS está planejado, ver `docs/threat-model.md`).
- **O banco local não é cifrado em repouso**: o texto das mensagens fica em claro no disco. Use a
  cifragem de disco do sistema operacional.
- A chave de identidade fica no keystore do SO; sem Secret Service (Linux) cai em arquivo com
  permissão `0600`.
- Trocar o ticket por um canal inseguro permite a um atacante se passar pela pessoa. Confira a
  **fingerprint** por outro canal ao aceitar um contato.

O modelo de ameaças e as limitações conhecidas estão em
[`docs/threat-model.md`](docs/threat-model.md).

## Escopo

Em escopo: o código deste repositório (`enxame-core`, `enxame-cli`, `enxame-node`, `deploy/`), o
protocolo descrito em `docs/protocol.md` e a configuração padrão. Fora de escopo: vulnerabilidades
nas dependências sem impacto específico aqui (reporte-as aos mantenedores delas; o `cargo-deny`
acompanha advisories) e ataques que exigem comprometer o dispositivo do usuário.

## Versões suportadas

Apenas a ponta do `main` (MVP em desenvolvimento).
