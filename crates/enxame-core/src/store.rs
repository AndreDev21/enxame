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
