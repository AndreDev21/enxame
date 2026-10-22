//! Persistência em SQLite (spec 5): contatos, envelopes e relações de causalidade.
//!
//! Migrations por `PRAGMA user_version` com SQL versionado. Operações de mensagens rodam em
//! `spawn_blocking` via [`Store::run`]; as de contatos (raras e minúsculas) usam
//! [`Store::with`] direto.
//!
//! O banco **não é cifrado em repouso**: o texto das mensagens fica em claro no arquivo. A
//! proteção é a do disco/conta do SO (ver `docs/threat-model.md`).

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use iroh::EndpointAddr;
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    contacts::{Contact, ContactStatus},
    crypto::Envelope,
    hlc::Hlc,
    identity::DeviceCert,
    order::OrderItem,
    Error, Result,
};

/// Migrations em ordem; o índice + 1 é o `user_version` resultante.
const MIGRATIONS: &[&str] = &[
    // v1: contatos e envelopes.
    "CREATE TABLE contacts (
        identity_pk  BLOB PRIMARY KEY,
        enc_pk       BLOB NOT NULL,
        device_cert  BLOB NOT NULL,
        node_addr    BLOB NOT NULL,
        display_name TEXT NOT NULL,
        status       INTEGER NOT NULL,
        added_at     INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE TABLE envelopes (
        id              BLOB PRIMARY KEY,
        conversation_id BLOB NOT NULL,
        author          BLOB NOT NULL,
        hlc_ms          INTEGER NOT NULL,
        hlc_ctr         INTEGER NOT NULL,
        kind            INTEGER NOT NULL,
        raw             BLOB NOT NULL,
        plaintext       BLOB,
        outgoing        INTEGER NOT NULL,
        received_at     INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE INDEX envelopes_by_conv ON envelopes(conversation_id, hlc_ms, hlc_ctr);
     CREATE TABLE envelope_parents (
        child  BLOB NOT NULL REFERENCES envelopes(id) ON DELETE CASCADE,
        parent BLOB NOT NULL,
        PRIMARY KEY (child, parent)
     ) WITHOUT ROWID;
     CREATE INDEX envelope_parents_by_parent ON envelope_parents(parent);",
    // v2: carry, recibos, entrega pendente, contabilidade de contribuição.
    "-- mensagens minhas ainda não entregues ao destinatário
     CREATE TABLE pending_delivery (
        envelope_id BLOB PRIMARY KEY,
        recipient   BLOB NOT NULL,
        created_at  INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE INDEX pending_by_recipient ON pending_delivery(recipient);
     -- a quem já ofereci cada mensagem para carregar
     CREATE TABLE carry_offers_sent (
        envelope_id BLOB NOT NULL,
        carrier     BLOB NOT NULL,
        offered_at  INTEGER NOT NULL,
        PRIMARY KEY (envelope_id, carrier)
     ) WITHOUT ROWID;
     -- envelopes de terceiros que eu guardo para entregar
     CREATE TABLE carry_queue (
        envelope_id  BLOB PRIMARY KEY,
        recipient    BLOB NOT NULL,
        from_contact BLOB NOT NULL,
        raw          BLOB NOT NULL,
        size         INTEGER NOT NULL,
        accepted_at  INTEGER NOT NULL,
        expires_at   INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE INDEX carry_by_recipient ON carry_queue(recipient);
     CREATE INDEX carry_by_contact ON carry_queue(from_contact);
     -- recibos assinados (envelope kind = Receipt) e os ids que confirmam
     CREATE TABLE receipts (
        id         BLOB PRIMARY KEY,
        author     BLOB NOT NULL,
        raw        BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE TABLE receipt_acks (
        envelope_id BLOB NOT NULL,
        author      BLOB NOT NULL,
        receipt_id  BLOB NOT NULL REFERENCES receipts(id) ON DELETE CASCADE,
        PRIMARY KEY (envelope_id, author)
     ) WITHOUT ROWID;
     CREATE INDEX receipt_acks_by_receipt ON receipt_acks(receipt_id);
     -- contatos em comum com cada par (para escolher carriers)
     CREATE TABLE common_contacts (
        peer  BLOB NOT NULL,
        other BLOB NOT NULL,
        PRIMARY KEY (peer, other)
     ) WITHOUT ROWID;
     -- confiabilidade e último papel conhecido de cada contato
     CREATE TABLE contact_reliability (
        identity_pk BLOB PRIMARY KEY,
        attempts    INTEGER NOT NULL DEFAULT 0,
        successes   INTEGER NOT NULL DEFAULT 0,
        last_seen   INTEGER NOT NULL DEFAULT 0,
        last_role   INTEGER
     ) WITHOUT ROWID;
     -- quanto servi a terceiros, por mês (AAAAMM) e por contato
     CREATE TABLE contribution_stats (
        month        INTEGER NOT NULL,
        contact      BLOB NOT NULL,
        bytes_served INTEGER NOT NULL DEFAULT 0,
        items        INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (month, contact)
     ) WITHOUT ROWID;",
    // v3: arquivos (blobs). `origin`: 0 = enviado por mim, 1 = baixado, 2 = replicado como carrier.
    "CREATE TABLE blobs (
        hash       BLOB PRIMARY KEY,
        origin     INTEGER NOT NULL,
        plain_size INTEGER NOT NULL,
        enc_size   INTEGER NOT NULL,
        name       TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER
     ) WITHOUT ROWID;
     CREATE INDEX blobs_by_expiry ON blobs(expires_at) WHERE expires_at IS NOT NULL;",
    // v4: grupos. Guarda só o retrato mais novo; `active` = eu consto nele.
    "CREATE TABLE groups (
        group_id   BLOB PRIMARY KEY,
        creator    BLOB NOT NULL,
        seq        INTEGER NOT NULL,
        active     INTEGER NOT NULL,
        snapshot   BLOB NOT NULL,
        updated_at INTEGER NOT NULL
     ) WITHOUT ROWID;",
    // v5: uma mensagem de grupo tem vários destinatários pendentes e pode ser carregada para
    // vários: a chave passa a ser (envelope, destinatário).
    "CREATE TABLE pending_delivery_v5 (
        envelope_id BLOB NOT NULL,
        recipient   BLOB NOT NULL,
        created_at  INTEGER NOT NULL,
        PRIMARY KEY (envelope_id, recipient)
     ) WITHOUT ROWID;
     INSERT INTO pending_delivery_v5 SELECT envelope_id, recipient, created_at FROM pending_delivery;
     DROP TABLE pending_delivery;
     ALTER TABLE pending_delivery_v5 RENAME TO pending_delivery;
     CREATE INDEX pending_by_recipient ON pending_delivery(recipient);
     CREATE TABLE carry_queue_v5 (
        envelope_id  BLOB NOT NULL,
        recipient    BLOB NOT NULL,
        from_contact BLOB NOT NULL,
        raw          BLOB NOT NULL,
        size         INTEGER NOT NULL,
        accepted_at  INTEGER NOT NULL,
        expires_at   INTEGER NOT NULL,
        PRIMARY KEY (envelope_id, recipient)
     ) WITHOUT ROWID;
     INSERT INTO carry_queue_v5
        SELECT envelope_id, recipient, from_contact, raw, size, accepted_at, expires_at FROM carry_queue;
     DROP TABLE carry_queue;
     ALTER TABLE carry_queue_v5 RENAME TO carry_queue;
     CREATE INDEX carry_by_recipient ON carry_queue(recipient);
     CREATE INDEX carry_by_contact ON carry_queue(from_contact);",
];

fn db<E: std::fmt::Display>(e: E) -> Error {
    Error::Store(e.to_string())
}

/// Aplica as migrations pendentes.
fn migrate(conn: &mut Connection) -> Result<()> {
    let current: usize = conn
        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        .map_err(db)? as usize;
    if current > MIGRATIONS.len() {
        return Err(Error::Store(format!(
            "banco na versão {current}, este build só entende até {}",
            MIGRATIONS.len()
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction().map_err(db)?;
        tx.execute_batch(sql).map_err(db)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)
            .map_err(db)?;
        tx.commit().map_err(db)?;
    }
    Ok(())
}

/// Mensagem guardada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEnvelope {
    pub envelope: Envelope,
    pub plaintext: Option<Vec<u8>>,
    pub outgoing: bool,
    pub received_at: u64,
}

/// Acesso ao banco. Barato de clonar.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Store")
    }
}

impl Store {
    /// Abre (criando e migrando) o banco em `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(db)?;
        Self::init(conn)
    }

    /// Banco em memória (testes).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db)?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
        )
        .map_err(db)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Executa `f` na thread atual (para operações curtas).
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        f(&mut self.lock())
    }

    /// Executa `f` numa thread de bloqueio.
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.with(f))
            .await
            .map_err(|e| Error::Store(format!("tarefa de banco: {e}")))?
    }

    /// Versão do schema (`user_version`).
    #[cfg(test)]
    pub fn schema_version(&self) -> Result<usize> {
        self.with(|c| {
            Ok(
                c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .map_err(db)? as usize,
            )
        })
    }
}

// ---------------------------------------------------------------- contatos

fn status_to_i(s: ContactStatus) -> i64 {
    match s {
        ContactStatus::Outgoing => 0,
        ContactStatus::Mutual => 1,
    }
}

pub fn contacts_load(conn: &Connection) -> Result<Vec<Contact>> {
    let mut stmt = conn
        .prepare("SELECT identity_pk, enc_pk, device_cert, node_addr, display_name, status, added_at FROM contacts ORDER BY added_at, identity_pk")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .map_err(db)?;
    let mut out = Vec::new();
    for row in rows {
        let (pk, enc, cert, addr, name, status, added) = row.map_err(db)?;
        out.push(Contact {
            identity_pk: pk
                .try_into()
                .map_err(|_| Error::Store("identity_pk corrompida".into()))?,
            enc_pk: enc
                .try_into()
                .map_err(|_| Error::Store("enc_pk corrompida".into()))?,
            device_cert: postcard::from_bytes::<DeviceCert>(&cert)?,
            node_addr: postcard::from_bytes::<EndpointAddr>(&addr)?,
            display_name: name,
            status: if status == 1 {
                ContactStatus::Mutual
            } else {
                ContactStatus::Outgoing
            },
            added_at: added as u64,
        });
    }
    Ok(out)
}

pub fn contact_upsert(conn: &Connection, c: &Contact) -> Result<()> {
    conn.execute(
        "INSERT INTO contacts (identity_pk, enc_pk, device_cert, node_addr, display_name, status, added_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(identity_pk) DO UPDATE SET enc_pk=?2, device_cert=?3, node_addr=?4,
            display_name=?5, status=?6, added_at=?7",
        params![
            c.identity_pk.as_slice(),
            c.enc_pk.as_slice(),
            postcard::to_stdvec(&c.device_cert)?,
            postcard::to_stdvec(&c.node_addr)?,
            c.display_name,
            status_to_i(c.status),
            c.added_at as i64,
        ],
    )
    .map_err(db)?;
    Ok(())
}

pub fn contact_remove(conn: &Connection, pk: &[u8; 32]) -> Result<()> {
    conn.execute(
        "DELETE FROM contacts WHERE identity_pk = ?1",
        [pk.as_slice()],
    )
    .map_err(db)?;
    Ok(())
}

// --------------------------------------------------------------- envelopes

fn arr32(v: Vec<u8>) -> Result<[u8; 32]> {
    v.try_into()
        .map_err(|_| Error::Store("id corrompido".into()))
}

/// Insere o envelope (e seus pais). `false` se já existia (deduplicação por `id`).
pub fn envelope_insert(
    conn: &mut Connection,
    env: &Envelope,
    plaintext: Option<&[u8]>,
    outgoing: bool,
    now_ms: u64,
) -> Result<bool> {
    let raw = postcard::to_stdvec(env)?;
    let tx = conn.transaction().map_err(db)?;
    let n = tx
        .execute(
            "INSERT OR IGNORE INTO envelopes (id, conversation_id, author, hlc_ms, hlc_ctr, kind, raw, plaintext, outgoing, received_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                env.id.as_slice(),
                env.conversation_id.as_slice(),
                env.author.as_slice(),
                env.hlc.ms as i64,
                env.hlc.counter as i64,
                env.kind as i64,
                raw,
                plaintext,
                outgoing as i64,
                now_ms as i64,
            ],
        )
        .map_err(db)?;
    if n == 1 {
        for p in &env.parents {
            tx.execute(
                "INSERT OR IGNORE INTO envelope_parents (child, parent) VALUES (?1, ?2)",
                params![env.id.as_slice(), p.as_slice()],
            )
            .map_err(db)?;
        }
    }
    tx.commit().map_err(db)?;
    Ok(n == 1)
}

pub fn envelope_exists(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM envelopes WHERE id = ?1",
        [id.as_slice()],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(db)
}

/// Pontas da conversa: envelopes que nenhum outro cita como pai.
pub fn heads(conn: &Connection, conv: &[u8; 32]) -> Result<Vec<[u8; 32]>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id FROM envelopes e
             WHERE e.conversation_id = ?1
               AND NOT EXISTS (SELECT 1 FROM envelope_parents p WHERE p.parent = e.id)
             ORDER BY e.hlc_ms, e.hlc_ctr, e.id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([conv.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    rows.map(|r| arr32(r.map_err(db)?)).collect()
}

/// (quantidade, maior HLC) da conversa.
pub fn conversation_summary(conn: &Connection, conv: &[u8; 32]) -> Result<(u64, Hlc)> {
    let (count, ms, ctr): (i64, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), MAX(hlc_ms), (SELECT hlc_ctr FROM envelopes WHERE conversation_id = ?1 ORDER BY hlc_ms DESC, hlc_ctr DESC LIMIT 1)
             FROM envelopes WHERE conversation_id = ?1",
            [conv.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db)?;
    Ok((
        count as u64,
        Hlc::new(ms.unwrap_or(0) as u64, ctr.unwrap_or(0) as u32),
    ))
}

/// Resumo do conjunto de ids da conversa: BLAKE3 dos ids em ordem crescente.
pub fn conversation_digest(conn: &Connection, conv: &[u8; 32]) -> Result<[u8; 32]> {
    let mut stmt = conn
        .prepare("SELECT id FROM envelopes WHERE conversation_id = ?1 ORDER BY id")
        .map_err(db)?;
    let rows = stmt
        .query_map([conv.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    let mut h = blake3::Hasher::new();
    h.update(b"enxame/conv-digest/1\0");
    for r in rows {
        h.update(&r.map_err(db)?);
    }
    Ok(*h.finalize().as_bytes())
}

/// Maior HLC de todo o banco (para retomar o relógio).
pub fn max_hlc(conn: &Connection) -> Result<Hlc> {
    let r: Option<(i64, i64)> = conn
        .query_row(
            "SELECT hlc_ms, hlc_ctr FROM envelopes ORDER BY hlc_ms DESC, hlc_ctr DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    Ok(r.map_or(Hlc::ZERO, |(ms, c)| Hlc::new(ms as u64, c as u32)))
}

/// Até `limit` ids com HLC `>= since` (inclusive), em ordem, mais o HLC do primeiro que ficou
/// de fora (cursor da próxima página), se houver.
pub fn ids_since(
    conn: &Connection,
    conv: &[u8; 32],
    since: Hlc,
    limit: usize,
) -> Result<(Vec<[u8; 32]>, Option<Hlc>)> {
    let mut stmt = conn
        .prepare(
            "SELECT id, hlc_ms, hlc_ctr FROM envelopes
             WHERE conversation_id = ?1 AND (hlc_ms, hlc_ctr) >= (?2, ?3)
             ORDER BY hlc_ms, hlc_ctr, id LIMIT ?4",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map(
            params![
                conv.as_slice(),
                since.ms as i64,
                since.counter as i64,
                (limit + 1) as i64
            ],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(db)?;
    let mut ids = Vec::new();
    let mut next = None;
    for row in rows {
        let (id, ms, ctr) = row.map_err(db)?;
        if ids.len() == limit {
            next = Some(Hlc::new(ms as u64, ctr as u32));
            break;
        }
        ids.push(arr32(id)?);
    }
    Ok((ids, next))
}

/// Como [`envelopes_by_ids`], com o conteúdo decifrado guardado (meus envelopes).
pub fn envelopes_with_plaintext(
    conn: &Connection,
    ids: &[[u8; 32]],
) -> Result<Vec<(Envelope, Option<Vec<u8>>)>> {
    let mut stmt = conn
        .prepare("SELECT raw, plaintext FROM envelopes WHERE id = ?1")
        .map_err(db)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some((raw, plain)) = stmt
            .query_row([id.as_slice()], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<Vec<u8>>>(1)?))
            })
            .optional()
            .map_err(db)?
        {
            out.push((postcard::from_bytes(&raw)?, plain));
        }
    }
    Ok(out)
}

/// Envelopes completos pelos ids pedidos (os que não existem são omitidos).
pub fn envelopes_by_ids(conn: &Connection, ids: &[[u8; 32]]) -> Result<Vec<Envelope>> {
    let mut stmt = conn
        .prepare("SELECT raw FROM envelopes WHERE id = ?1")
        .map_err(db)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(raw) = stmt
            .query_row([id.as_slice()], |r| r.get::<_, Vec<u8>>(0))
            .optional()
            .map_err(db)?
        {
            out.push(postcard::from_bytes(&raw)?);
        }
    }
    Ok(out)
}

/// Todos os envelopes da conversa, com texto e direção.
pub fn conversation_envelopes(conn: &Connection, conv: &[u8; 32]) -> Result<Vec<StoredEnvelope>> {
    let mut stmt = conn
        .prepare("SELECT raw, plaintext, outgoing, received_at FROM envelopes WHERE conversation_id = ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([conv.as_slice()], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Option<Vec<u8>>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(db)?;
    let mut out = Vec::new();
    for row in rows {
        let (raw, plaintext, outgoing, received_at) = row.map_err(db)?;
        out.push(StoredEnvelope {
            envelope: postcard::from_bytes(&raw)?,
            plaintext,
            outgoing: outgoing != 0,
            received_at: received_at as u64,
        });
    }
    Ok(out)
}

// ------------------------------------------------- entrega pendente e carry

/// Registra uma mensagem minha como ainda não entregue a `recipient`.
pub fn pending_add(
    conn: &Connection,
    id: &[u8; 32],
    recipient: &[u8; 32],
    now_ms: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO pending_delivery (envelope_id, recipient, created_at) VALUES (?1,?2,?3)",
        params![id.as_slice(), recipient.as_slice(), now_ms as i64],
    )
    .map_err(db)?;
    Ok(())
}

/// Ids pendentes de entrega a `recipient`, do mais antigo ao mais novo.
pub fn pending_list(conn: &Connection, recipient: &[u8; 32]) -> Result<Vec<[u8; 32]>> {
    let mut stmt = conn
        .prepare("SELECT envelope_id FROM pending_delivery WHERE recipient = ?1 ORDER BY created_at, envelope_id")
        .map_err(db)?;
    let rows = stmt
        .query_map([recipient.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    rows.map(|r| arr32(r.map_err(db)?)).collect()
}

/// Remove da lista de pendentes (entregue direto ou confirmado por recibo).
pub fn pending_remove(conn: &Connection, ids: &[[u8; 32]], recipient: &[u8; 32]) -> Result<()> {
    for id in ids {
        conn.execute(
            "DELETE FROM pending_delivery WHERE envelope_id = ?1 AND recipient = ?2",
            params![id.as_slice(), recipient.as_slice()],
        )
        .map_err(db)?;
    }
    Ok(())
}

/// Registra que oferecemos `id` ao `carrier`.
pub fn offer_record(
    conn: &Connection,
    id: &[u8; 32],
    carrier: &[u8; 32],
    now_ms: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO carry_offers_sent (envelope_id, carrier, offered_at) VALUES (?1,?2,?3)",
        params![id.as_slice(), carrier.as_slice(), now_ms as i64],
    )
    .map_err(db)?;
    Ok(())
}

/// Carriers a quem `id` já foi oferecido e aceito.
pub fn offered_carriers(conn: &Connection, id: &[u8; 32]) -> Result<Vec<[u8; 32]>> {
    let mut stmt = conn
        .prepare("SELECT carrier FROM carry_offers_sent WHERE envelope_id = ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([id.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    rows.map(|r| arr32(r.map_err(db)?)).collect()
}

/// Bytes e itens em carry, no total e vindos de um contato.
pub fn carry_usage(conn: &Connection, from_contact: &[u8; 32]) -> Result<(u64, u64)> {
    let total: i64 = conn
        .query_row("SELECT COALESCE(SUM(size), 0) FROM carry_queue", [], |r| {
            r.get(0)
        })
        .map_err(db)?;
    let from: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(size), 0) FROM carry_queue WHERE from_contact = ?1",
            [from_contact.as_slice()],
            |r| r.get(0),
        )
        .map_err(db)?;
    Ok((total as u64, from as u64))
}

/// Número de itens guardados para carregar.
pub fn carry_count(conn: &Connection) -> Result<u64> {
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM carry_queue", [], |r| r.get(0))
        .map_err(db)?;
    Ok(n as u64)
}

/// Guarda um envelope de terceiros para entregar a `recipient`. `false` se já existia.
pub fn carry_insert(
    conn: &Connection,
    env: &Envelope,
    recipient: &[u8; 32],
    from_contact: &[u8; 32],
    now_ms: u64,
    ttl_ms: u64,
) -> Result<bool> {
    let raw = postcard::to_stdvec(env)?;
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO carry_queue (envelope_id, recipient, from_contact, raw, size, accepted_at, expires_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                env.id.as_slice(),
                recipient.as_slice(),
                from_contact.as_slice(),
                raw,
                raw.len() as i64,
                now_ms as i64,
                now_ms.saturating_add(ttl_ms) as i64,
            ],
        )
        .map_err(db)?;
    Ok(n == 1)
}

/// Guardo `id` para este destinatário?
pub fn carry_has_for(conn: &Connection, id: &[u8; 32], recipient: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM carry_queue WHERE envelope_id = ?1 AND recipient = ?2",
        params![id.as_slice(), recipient.as_slice()],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(db)
}

#[cfg(test)]
pub fn carry_has(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM carry_queue WHERE envelope_id = ?1",
        [id.as_slice()],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(db)
}

/// Envelopes que carrego para `recipient`, do mais antigo ao mais novo.
pub fn carry_for(conn: &Connection, recipient: &[u8; 32]) -> Result<Vec<Envelope>> {
    let mut stmt = conn
        .prepare(
            "SELECT raw FROM carry_queue WHERE recipient = ?1 ORDER BY accepted_at, envelope_id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([recipient.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(postcard::from_bytes(&r.map_err(db)?)?);
    }
    Ok(out)
}

/// Remove o que passou do prazo (carry e recibos).
pub fn expire(conn: &Connection, now_ms: u64) -> Result<(usize, usize)> {
    let c = conn
        .execute(
            "DELETE FROM carry_queue WHERE expires_at <= ?1",
            [now_ms as i64],
        )
        .map_err(db)?;
    let r = conn
        .execute(
            "DELETE FROM receipts WHERE expires_at <= ?1",
            [now_ms as i64],
        )
        .map_err(db)?;
    Ok((c, r))
}

// ----------------------------------------------------------------- recibos

/// Guarda um recibo verificado e expurga o que ele confirma. Devolve `(novo, expurgados)`.
pub fn receipt_store(
    conn: &mut Connection,
    receipt: &Envelope,
    now_ms: u64,
    ttl_ms: u64,
) -> Result<(bool, usize)> {
    let raw = postcard::to_stdvec(receipt)?;
    let tx = conn.transaction().map_err(db)?;
    let n = tx
        .execute(
            "INSERT OR IGNORE INTO receipts (id, author, raw, created_at, expires_at) VALUES (?1,?2,?3,?4,?5)",
            params![
                receipt.id.as_slice(),
                receipt.author.as_slice(),
                raw,
                now_ms as i64,
                now_ms.saturating_add(ttl_ms) as i64
            ],
        )
        .map_err(db)?;
    let mut purged = 0;
    for acked in &receipt.parents {
        tx.execute(
            "INSERT OR IGNORE INTO receipt_acks (envelope_id, author, receipt_id) VALUES (?1,?2,?3)",
            params![acked.as_slice(), receipt.author.as_slice(), receipt.id.as_slice()],
        )
        .map_err(db)?;
        purged += tx
            .execute(
                "DELETE FROM carry_queue WHERE envelope_id = ?1 AND recipient = ?2",
                params![acked.as_slice(), receipt.author.as_slice()],
            )
            .map_err(db)?;
        tx.execute(
            "DELETE FROM pending_delivery WHERE envelope_id = ?1 AND recipient = ?2",
            params![acked.as_slice(), receipt.author.as_slice()],
        )
        .map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok((n == 1, purged))
}

/// `id` já foi confirmado por um recibo do destinatário `author`?
pub fn is_acked(conn: &Connection, id: &[u8; 32], author: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM receipt_acks WHERE envelope_id = ?1 AND author = ?2",
        params![id.as_slice(), author.as_slice()],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(db)
}

/// Os recibos mais recentes (para espalhar aos pares).
pub fn receipts_recent(conn: &Connection, limit: usize) -> Result<Vec<Envelope>> {
    let mut stmt = conn
        .prepare("SELECT raw FROM receipts ORDER BY created_at DESC, id LIMIT ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([limit as i64], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(postcard::from_bytes(&r.map_err(db)?)?);
    }
    Ok(out)
}

// ------------------------------------------------------ contatos em comum

pub fn common_set(conn: &mut Connection, peer: &[u8; 32], others: &[[u8; 32]]) -> Result<()> {
    let tx = conn.transaction().map_err(db)?;
    tx.execute(
        "DELETE FROM common_contacts WHERE peer = ?1",
        [peer.as_slice()],
    )
    .map_err(db)?;
    for o in others {
        tx.execute(
            "INSERT OR IGNORE INTO common_contacts (peer, other) VALUES (?1,?2)",
            params![peer.as_slice(), o.as_slice()],
        )
        .map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok(())
}

pub fn common_get(conn: &Connection, peer: &[u8; 32]) -> Result<Vec<[u8; 32]>> {
    let mut stmt = conn
        .prepare("SELECT other FROM common_contacts WHERE peer = ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([peer.as_slice()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    rows.map(|r| arr32(r.map_err(db)?)).collect()
}

// ----------------------------------------------------------- confiabilidade

/// Registra uma tentativa de falar com `pk` (e o papel visto, se conectou).
pub fn reliability_record(
    conn: &Connection,
    pk: &[u8; 32],
    ok: bool,
    role: Option<crate::Role>,
    now_ms: u64,
) -> Result<()> {
    let role_i = role.map(|r| r as i64);
    conn.execute(
        "INSERT INTO contact_reliability (identity_pk, attempts, successes, last_seen, last_role)
         VALUES (?1, 1, ?2, ?3, ?4)
         ON CONFLICT(identity_pk) DO UPDATE SET
            attempts = attempts + 1,
            successes = successes + ?2,
            last_seen = CASE WHEN ?2 = 1 THEN ?3 ELSE last_seen END,
            last_role = COALESCE(?4, last_role)",
        params![pk.as_slice(), ok as i64, now_ms as i64, role_i],
    )
    .map_err(db)?;
    Ok(())
}

/// Guarda o papel anunciado por um contato sem contar como tentativa.
pub fn reliability_set_role(
    conn: &Connection,
    pk: &[u8; 32],
    role: crate::Role,
    now_ms: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO contact_reliability (identity_pk, attempts, successes, last_seen, last_role)
         VALUES (?1, 0, 0, ?2, ?3)
         ON CONFLICT(identity_pk) DO UPDATE SET last_role = ?3",
        params![pk.as_slice(), now_ms as i64, role as i64],
    )
    .map_err(db)?;
    Ok(())
}

/// `(tentativas, sucessos, último papel)` de um contato.
pub fn reliability_get(
    conn: &Connection,
    pk: &[u8; 32],
) -> Result<(u64, u64, Option<crate::Role>)> {
    let r: Option<(i64, i64, Option<i64>)> = conn
        .query_row(
            "SELECT attempts, successes, last_role FROM contact_reliability WHERE identity_pk = ?1",
            [pk.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db)?;
    Ok(r.map_or((0, 0, None), |(a, s, role)| {
        (a as u64, s as u64, role.and_then(role_from_i))
    }))
}

fn role_from_i(i: i64) -> Option<crate::Role> {
    use crate::Role::*;
    [Leaf, LeafPlus, Peer, Relay, SuperPeer]
        .get(usize::try_from(i).ok()?)
        .copied()
}

// ----------------------------------------------------------------- arquivos

/// De onde veio um blob que temos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobOrigin {
    /// Eu enviei: servir é uso normal, não contribuição.
    Own,
    /// Baixei de um contato e passei a semear.
    Downloaded,
    /// Replicado por ser carrier (tem prazo).
    Prefetched,
}

impl BlobOrigin {
    fn to_i(self) -> i64 {
        match self {
            Self::Own => 0,
            Self::Downloaded => 1,
            Self::Prefetched => 2,
        }
    }
    fn from_i(i: i64) -> Self {
        match i {
            0 => Self::Own,
            1 => Self::Downloaded,
            _ => Self::Prefetched,
        }
    }
}

/// Registro de um blob (raiz) que temos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRow {
    pub hash: [u8; 32],
    pub origin: BlobOrigin,
    pub plain_size: u64,
    pub enc_size: u64,
    pub name: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

/// Insere ou atualiza (a origem `Own` nunca é rebaixada).
pub fn blob_upsert(conn: &Connection, b: &BlobRow) -> Result<()> {
    conn.execute(
        "INSERT INTO blobs (hash, origin, plain_size, enc_size, name, created_at, expires_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(hash) DO UPDATE SET
            origin = MIN(origin, ?2),
            expires_at = CASE WHEN MIN(origin, ?2) = 2 THEN COALESCE(?7, expires_at) ELSE NULL END",
        params![
            b.hash.as_slice(),
            b.origin.to_i(),
            b.plain_size as i64,
            b.enc_size as i64,
            b.name,
            b.created_at as i64,
            b.expires_at.map(|e| e as i64),
        ],
    )
    .map_err(db)?;
    Ok(())
}

/// Colunas de `blobs`: hash, origem, tamanho em claro, tamanho cifrado, nome, criação, validade.
type BlobCols = (Vec<u8>, i64, i64, i64, String, i64, Option<i64>);

fn blob_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BlobCols> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
    ))
}

fn blob_make(t: BlobCols) -> Result<BlobRow> {
    Ok(BlobRow {
        hash: arr32(t.0)?,
        origin: BlobOrigin::from_i(t.1),
        plain_size: t.2 as u64,
        enc_size: t.3 as u64,
        name: t.4,
        created_at: t.5 as u64,
        expires_at: t.6.map(|e| e as u64),
    })
}

pub fn blob_get(conn: &Connection, hash: &[u8; 32]) -> Result<Option<BlobRow>> {
    conn.query_row(
        "SELECT hash, origin, plain_size, enc_size, name, created_at, expires_at FROM blobs WHERE hash = ?1",
        [hash.as_slice()],
        blob_from_row,
    )
    .optional()
    .map_err(db)?
    .map(blob_make)
    .transpose()
}

pub fn blob_list(conn: &Connection) -> Result<Vec<BlobRow>> {
    let mut stmt = conn
        .prepare("SELECT hash, origin, plain_size, enc_size, name, created_at, expires_at FROM blobs ORDER BY created_at")
        .map_err(db)?;
    let rows = stmt.query_map([], blob_from_row).map_err(db)?;
    rows.map(|r| blob_make(r.map_err(db)?)).collect()
}

/// Raízes com prazo vencido.
pub fn blobs_expired(conn: &Connection, now_ms: u64) -> Result<Vec<[u8; 32]>> {
    let mut stmt = conn
        .prepare("SELECT hash FROM blobs WHERE expires_at IS NOT NULL AND expires_at <= ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([now_ms as i64], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    rows.map(|r| arr32(r.map_err(db)?)).collect()
}

/// Bytes de ciphertext replicados como carrier e ainda dentro do prazo.
pub fn blobs_prefetched_bytes(conn: &Connection, now_ms: u64) -> Result<u64> {
    let n: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(enc_size), 0) FROM blobs WHERE origin = 2 AND (expires_at IS NULL OR expires_at > ?1)",
            [now_ms as i64],
            |r| r.get(0),
        )
        .map_err(db)?;
    Ok(n as u64)
}

pub fn blob_delete(conn: &Connection, hash: &[u8; 32]) -> Result<()> {
    conn.execute("DELETE FROM blobs WHERE hash = ?1", [hash.as_slice()])
        .map_err(db)?;
    Ok(())
}

/// Envelopes `FileRef` da conversa (raw + plaintext), do mais antigo ao mais novo.
pub fn filerefs_in_conversation(conn: &Connection, conv: &[u8; 32]) -> Result<Vec<StoredEnvelope>> {
    let mut stmt = conn
        .prepare(
            "SELECT raw, plaintext, outgoing, received_at FROM envelopes
             WHERE conversation_id = ?1 AND kind = ?2 ORDER BY hlc_ms, hlc_ctr, id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map(
            params![conv.as_slice(), crate::crypto::EnvelopeKind::FileRef as i64],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Option<Vec<u8>>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(db)?;
    let mut out = Vec::new();
    for row in rows {
        let (raw, plaintext, outgoing, received_at) = row.map_err(db)?;
        out.push(StoredEnvelope {
            envelope: postcard::from_bytes(&raw)?,
            plaintext,
            outgoing: outgoing != 0,
            received_at: received_at as u64,
        });
    }
    Ok(out)
}

// ------------------------------------------------------------ contribuição

/// Soma ao que servi no mês (`AAAAMM`) para o contato.
/// Grava o retrato se for mais novo (`seq` maior) que o guardado. Devolve `true` se aplicou.
pub fn group_put(
    conn: &Connection,
    snap: &crate::groups::GroupSnapshot,
    me: &[u8; 32],
    now_ms: u64,
) -> Result<bool> {
    let id = snap.id();
    let current: Option<i64> = conn
        .query_row(
            "SELECT seq FROM groups WHERE group_id = ?1",
            [&id[..]],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if current.is_some_and(|c| c >= snap.seq as i64) {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR REPLACE INTO groups (group_id, creator, seq, active, snapshot, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            &id[..],
            &snap.creator[..],
            snap.seq as i64,
            snap.has(me),
            snap.encode()?,
            now_ms as i64
        ],
    )
    .map_err(db)?;
    Ok(true)
}

pub fn group_exists(conn: &Connection, id: &[u8; 32]) -> Result<bool> {
    conn.query_row(
        "SELECT 1 FROM groups WHERE group_id = ?1",
        [&id[..]],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(db)
}

pub fn groups_load(conn: &Connection) -> Result<Vec<crate::groups::StoredGroup>> {
    let mut stmt = conn
        .prepare("SELECT snapshot, active, updated_at FROM groups ORDER BY updated_at, group_id")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(db)?;
    let mut out = Vec::new();
    for row in rows {
        let (blob, active, updated_at) = row.map_err(db)?;
        out.push(crate::groups::StoredGroup {
            snapshot: crate::groups::GroupSnapshot::decode(&blob)?,
            active,
            updated_at: updated_at as u64,
        });
    }
    Ok(out)
}

pub fn contribution_add(
    conn: &Connection,
    month: u32,
    contact: &[u8; 32],
    bytes: u64,
    items: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO contribution_stats (month, contact, bytes_served, items) VALUES (?1,?2,?3,?4)
         ON CONFLICT(month, contact) DO UPDATE SET
            bytes_served = bytes_served + ?3, items = items + ?4",
        params![month as i64, contact.as_slice(), bytes as i64, items as i64],
    )
    .map_err(db)?;
    Ok(())
}

/// Total servido no mês, em bytes.
pub fn contribution_month_bytes(conn: &Connection, month: u32) -> Result<u64> {
    let n: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(bytes_served), 0) FROM contribution_stats WHERE month = ?1",
            [month as i64],
            |r| r.get(0),
        )
        .map_err(db)?;
    Ok(n as u64)
}

/// Por contato, no mês: `(contato, bytes, itens)`.
pub fn contribution_by_contact(conn: &Connection, month: u32) -> Result<Vec<([u8; 32], u64, u64)>> {
    let mut stmt = conn
        .prepare("SELECT contact, bytes_served, items FROM contribution_stats WHERE month = ?1 ORDER BY bytes_served DESC")
        .map_err(db)?;
    let rows = stmt
        .query_map([month as i64], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(db)?;
    let mut out = Vec::new();
    for r in rows {
        let (c, b, i) = r.map_err(db)?;
        out.push((arr32(c)?, b as u64, i as u64));
    }
    Ok(out)
}

impl StoredEnvelope {
    pub fn order_item(&self) -> OrderItem {
        OrderItem {
            id: self.envelope.id,
            hlc: self.envelope.hlc,
            parents: self.envelope.parents.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{dm_conversation_id, EnvelopeKind, Recipient},
        identity::Identity,
    };

    fn env(
        from: &Identity,
        to: &Identity,
        hlc: Hlc,
        parents: Vec<[u8; 32]>,
        text: &str,
    ) -> Envelope {
        Envelope::create(
            from,
            dm_conversation_id(&from.identity_pk(), &to.identity_pk()),
            hlc,
            parents,
            &[Recipient {
                identity_pk: to.identity_pk(),
                enc_pk: to.enc_pk(),
            }],
            EnvelopeKind::Text,
            text.as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
    }

    /// Um banco da Fase 4 (v3) com mensagens pendentes e cópias de carry migra sem perder nada.
    #[test]
    fn v3_database_migrates_to_per_recipient_keys_keeping_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let mut conn = Connection::open(&path).unwrap();
            for (i, sql) in MIGRATIONS.iter().take(3).enumerate() {
                conn.execute_batch(sql).unwrap();
                conn.pragma_update(None, "user_version", (i + 1) as i64)
                    .unwrap();
            }
            conn.execute(
                "INSERT INTO pending_delivery (envelope_id, recipient, created_at) VALUES (x'01', x'0a', 5)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO carry_queue (envelope_id, recipient, from_contact, raw, size, accepted_at, expires_at)
                 VALUES (x'02', x'0b', x'0c', x'dd', 7, 1, 99)",
                [],
            )
            .unwrap();
            migrate(&mut conn).unwrap();
            assert_eq!(
                conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap() as usize,
                MIGRATIONS.len()
            );
            let pending: i64 = conn
                .query_row("SELECT COUNT(*) FROM pending_delivery", [], |r| r.get(0))
                .unwrap();
            let carried: i64 = conn
                .query_row(
                    "SELECT size FROM carry_queue WHERE recipient = x'0b'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!((pending, carried), (1, 7));
            // Agora o mesmo envelope pode estar pendente para dois destinatários.
            conn.execute(
                "INSERT INTO pending_delivery (envelope_id, recipient, created_at) VALUES (x'01', x'0e', 6)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 99i64).unwrap();
        drop(c);
        assert!(Store::open(&path).is_err());
    }

    #[test]
    fn insert_dedups_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let (a, b) = (Identity::generate(), Identity::generate());
        let e = env(&a, &b, Hlc::new(10, 0), vec![], "oi");
        {
            let s = Store::open(&path).unwrap();
            s.with(|c| {
                assert!(envelope_insert(c, &e, Some(b"oi"), true, 1)?);
                assert!(!envelope_insert(c, &e, Some(b"oi"), true, 2)?, "duplicata");
                Ok(())
            })
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let all = s
            .with(|c| conversation_envelopes(c, &e.conversation_id))
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].envelope, e);
        assert_eq!(all[0].plaintext.as_deref(), Some(&b"oi"[..]));
        assert!(all[0].outgoing);
    }

    #[test]
    fn heads_follow_parent_links() {
        let (a, b) = (Identity::generate(), Identity::generate());
        let e1 = env(&a, &b, Hlc::new(1, 0), vec![], "1");
        let e2 = env(&b, &a, Hlc::new(2, 0), vec![e1.id], "2");
        let e3 = env(&a, &b, Hlc::new(3, 0), vec![e2.id], "3");
        let e3b = env(&b, &a, Hlc::new(3, 1), vec![e2.id], "3b");
        let s = Store::open_in_memory().unwrap();
        s.with(|c| {
            for e in [&e1, &e2, &e3, &e3b] {
                envelope_insert(c, e, None, false, 0)?;
            }
            let h = heads(c, &e1.conversation_id)?;
            let mut want = vec![e3.id, e3b.id];
            want.sort();
            let mut got = h;
            got.sort();
            assert_eq!(got, want);
            let (count, max) = conversation_summary(c, &e1.conversation_id)?;
            assert_eq!((count, max), (4, Hlc::new(3, 1)));
            assert_eq!(max_hlc(c)?, Hlc::new(3, 1));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn ids_since_pages_without_skipping_ties() {
        let (a, b) = (Identity::generate(), Identity::generate());
        let s = Store::open_in_memory().unwrap();
        let conv = dm_conversation_id(&a.identity_pk(), &b.identity_pk());
        // 7 envelopes, vários com o mesmo HLC (autores diferentes).
        let mut all = Vec::new();
        for i in 0..7u64 {
            let (f, t) = if i % 2 == 0 { (&a, &b) } else { (&b, &a) };
            all.push(env(f, t, Hlc::new(1 + i / 3, 0), vec![], &format!("m{i}")));
        }
        s.with(|c| {
            for e in &all {
                envelope_insert(c, e, None, false, 0)?;
            }
            let mut got = Vec::new();
            let mut since = Hlc::ZERO;
            loop {
                let (ids, next) = ids_since(c, &conv, since, 3)?;
                got.extend(ids);
                match next {
                    Some(n) => since = n,
                    None => break,
                }
            }
            let mut want: Vec<_> = all.iter().map(|e| e.id).collect();
            want.sort();
            got.sort();
            assert_eq!(got, want);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn carry_queue_quota_expiry_and_receipts() {
        let (a, b) = (Identity::generate(), Identity::generate());
        let s = Store::open_in_memory().unwrap();
        let old = env(&a, &b, Hlc::new(1, 0), vec![], "velha");
        let fresh = env(&a, &b, Hlc::new(2, 0), vec![], "nova");
        s.with(|c| {
            // A primeira expira em 1 s, a segunda em 1 h.
            assert!(carry_insert(
                c,
                &old,
                &b.identity_pk(),
                &a.identity_pk(),
                1_000,
                1_000
            )?);
            assert!(carry_insert(
                c,
                &fresh,
                &b.identity_pk(),
                &a.identity_pk(),
                1_000,
                3_600_000
            )?);
            assert!(
                !carry_insert(
                    c,
                    &fresh,
                    &b.identity_pk(),
                    &a.identity_pk(),
                    1_000,
                    3_600_000
                )?,
                "duplicata"
            );
            let (total, from_a) = carry_usage(c, &a.identity_pk())?;
            assert!(total > 0 && total == from_a);
            assert_eq!(carry_usage(c, &[9; 32])?.1, 0);
            assert_eq!(carry_count(c)?, 2);

            // Passado o prazo, só a primeira some.
            assert_eq!(expire(c, 5_000)?, (1, 0));
            assert_eq!(carry_count(c)?, 1);
            assert!(carry_has(c, &fresh.id)? && !carry_has(c, &old.id)?);
            assert_eq!(carry_for(c, &b.identity_pk())?, vec![fresh.clone()]);
            assert!(carry_for(c, &a.identity_pk())?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn receipts_purge_only_the_authors_own_messages() {
        let (a, b, m) = (
            Identity::generate(),
            Identity::generate(),
            Identity::generate(),
        );
        let s = Store::open_in_memory().unwrap();
        let e = env(&a, &b, Hlc::new(1, 0), vec![], "x");
        let rec = |from: &Identity| {
            Envelope::create(
                from,
                dm_conversation_id(&from.identity_pk(), &a.identity_pk()),
                Hlc::new(5, 0),
                vec![e.id],
                &[crate::crypto::Recipient {
                    identity_pk: a.identity_pk(),
                    enc_pk: a.enc_pk(),
                }],
                EnvelopeKind::Receipt,
                b"",
            )
            .unwrap()
        };
        s.with(|c| {
            carry_insert(c, &e, &b.identity_pk(), &a.identity_pk(), 0, 1_000_000)?;
            pending_add(c, &e.id, &b.identity_pk(), 0)?;
            // Recibo de quem NÃO é o destinatário: guardado, mas não expurga nada.
            let (new, purged) = receipt_store(c, &rec(&m), 10, 1_000_000)?;
            assert!((new, purged) == (true, 0));
            assert!(carry_has(c, &e.id)? && pending_list(c, &b.identity_pk())?.len() == 1);
            assert!(!is_acked(c, &e.id, &b.identity_pk())?);
            // Recibo do destinatário: expurga a cópia e a pendência.
            let r = rec(&b);
            assert_eq!(receipt_store(c, &r, 10, 1_000_000)?, (true, 1));
            assert_eq!(
                receipt_store(c, &r, 11, 1_000_000)?,
                (false, 0),
                "idempotente"
            );
            assert!(!carry_has(c, &e.id)? && pending_list(c, &b.identity_pk())?.is_empty());
            assert!(is_acked(c, &e.id, &b.identity_pk())?);
            assert_eq!(receipts_recent(c, 10)?.len(), 2);
            // O recibo vence junto com o TTL.
            assert_eq!(expire(c, 2_000_000)?, (0, 2));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn blob_registry_keeps_own_origin_and_expires_prefetched() {
        let s = Store::open_in_memory().unwrap();
        s.with(|c| {
            let row = |h: u8, origin, exp| BlobRow {
                hash: [h; 32],
                origin,
                plain_size: 10,
                enc_size: 60,
                name: format!("f{h}"),
                created_at: 1,
                expires_at: exp,
            };
            blob_upsert(c, &row(1, BlobOrigin::Prefetched, Some(1000)))?;
            blob_upsert(c, &row(2, BlobOrigin::Downloaded, None))?;
            assert_eq!(blobs_expired(c, 999)?, Vec::<[u8; 32]>::new());
            assert_eq!(blobs_expired(c, 1000)?, vec![[1; 32]]);
            // Baixar de verdade um blob que só estava replicado o promove e tira o prazo.
            blob_upsert(c, &row(1, BlobOrigin::Downloaded, None))?;
            assert_eq!(
                blob_get(c, &[1; 32])?.unwrap().origin,
                BlobOrigin::Downloaded
            );
            assert_eq!(blob_get(c, &[1; 32])?.unwrap().expires_at, None);
            assert!(blobs_expired(c, u64::MAX / 2)?.is_empty());
            // Quem enviou nunca é rebaixado por um download posterior.
            blob_upsert(c, &row(3, BlobOrigin::Own, None))?;
            blob_upsert(c, &row(3, BlobOrigin::Downloaded, None))?;
            assert_eq!(blob_get(c, &[3; 32])?.unwrap().origin, BlobOrigin::Own);
            assert_eq!(blob_list(c)?.len(), 3);
            blob_delete(c, &[3; 32])?;
            assert!(blob_get(c, &[3; 32])?.is_none());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn contribution_and_reliability_accounting() {
        let s = Store::open_in_memory().unwrap();
        s.with(|c| {
            let (x, y) = ([1u8; 32], [2u8; 32]);
            contribution_add(c, 202610, &x, 1000, 2)?;
            contribution_add(c, 202610, &x, 500, 1)?;
            contribution_add(c, 202610, &y, 100, 1)?;
            contribution_add(c, 202611, &x, 9999, 9)?;
            assert_eq!(contribution_month_bytes(c, 202610)?, 1600);
            assert_eq!(contribution_month_bytes(c, 202612)?, 0);
            assert_eq!(
                contribution_by_contact(c, 202610)?,
                vec![(x, 1500, 3), (y, 100, 1)]
            );

            assert_eq!(reliability_get(c, &x)?, (0, 0, None));
            reliability_record(c, &x, true, Some(crate::Role::Relay), 10)?;
            reliability_record(c, &x, false, None, 20)?;
            assert_eq!(
                reliability_get(c, &x)?,
                (2, 1, Some(crate::Role::Relay)),
                "papel persiste"
            );
            reliability_set_role(c, &x, crate::Role::Peer, 30)?;
            assert_eq!(reliability_get(c, &x)?.2, Some(crate::Role::Peer));

            common_set(c, &x, &[y, [3; 32]])?;
            let mut got = common_get(c, &x)?;
            got.sort();
            assert_eq!(got, vec![y, [3; 32]]);
            common_set(c, &x, &[])?;
            assert!(common_get(c, &x)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn contacts_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let id = Identity::generate();
        let addr = EndpointAddr::new(id.endpoint_id()).with_ip_addr("192.0.2.1:1".parse().unwrap());
        let t = id.invite(addr, "ana", 1000, 3600).unwrap();
        let c = Contact {
            identity_pk: t.identity_pk,
            enc_pk: t.enc_pk,
            device_cert: t.device_cert.clone(),
            node_addr: t.node_addr.clone(),
            display_name: "ana".into(),
            status: ContactStatus::Mutual,
            added_at: 5,
        };
        s.with(|conn| {
            contact_upsert(conn, &c)?;
            assert_eq!(contacts_load(conn)?, vec![c.clone()]);
            contact_remove(conn, &c.identity_pk)?;
            assert!(contacts_load(conn)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
