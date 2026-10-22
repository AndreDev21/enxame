//! Cifragem de arquivos em streaming (spec 10, ADR 0007).
//!
//! Construção **STREAM** (Hoang, Reyhanitabar, Rogaway, Vizár) sobre XChaCha20-Poly1305: o arquivo
//! é dividido em blocos de 64 KiB e cada bloco é cifrado com o nonce
//! `prefixo(19) ‖ contador(u32 BE) ‖ último(u8)`. O bit "último" impede truncar o arquivo e o
//! contador impede reordenar ou repetir blocos. A chave é aleatória por arquivo.
//!
//! Formato do ciphertext:
//!
//! ```text
//! "ENXF" | versão(1) | tam_bloco(u32 BE) | prefixo_nonce(19)        ← cabeçalho, 28 bytes
//! bloco_0 (tam_bloco + 16) | bloco_1 | ... | bloco_n (1..=tam_bloco + 16, ou 16 se vazio)
//! ```
//!
//! Só se usa o AEAD do crate auditado; nenhuma primitiva é implementada aqui.

use chacha20poly1305::{
    aead::{Aead, Generate, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

use crate::{Error, Result};

/// Tamanho do bloco em claro.
pub const CHUNK_SIZE: usize = 64 * 1024;
const TAG: usize = 16;
const MAGIC: &[u8; 4] = b"ENXF";
const VERSION: u8 = 1;
const PREFIX_LEN: usize = 19;
/// Tamanho do cabeçalho.
pub const HEADER_LEN: usize = 4 + 1 + 4 + PREFIX_LEN;

fn crypto(msg: &str) -> Error {
    Error::Crypto(msg.into())
}

fn nonce(prefix: &[u8; PREFIX_LEN], counter: u32, last: bool) -> Result<XNonce> {
    let mut n = [0u8; 24];
    n[..PREFIX_LEN].copy_from_slice(prefix);
    n[PREFIX_LEN..PREFIX_LEN + 4].copy_from_slice(&counter.to_be_bytes());
    n[23] = u8::from(last);
    XNonce::try_from(&n[..]).map_err(|_| crypto("nonce"))
}

/// Tamanho do ciphertext para `plain_len` bytes em claro.
pub fn encrypted_len(plain_len: u64) -> u64 {
    let cs = CHUNK_SIZE as u64;
    let chunks = if plain_len == 0 {
        1
    } else {
        plain_len.div_ceil(cs)
    };
    HEADER_LEN as u64 + plain_len + chunks * TAG as u64
}

/// Cifrador incremental. Chame [`Encryptor::push`] com os dados e [`Encryptor::finish`] no fim.
pub struct Encryptor {
    cipher: XChaCha20Poly1305,
    prefix: [u8; PREFIX_LEN],
    counter: u32,
    buf: Vec<u8>,
    started: bool,
}

impl Encryptor {
    pub fn new(key: &[u8; 32]) -> Result<Self> {
        let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| crypto("chave"))?;
        let rnd = XNonce::generate();
        let mut prefix = [0u8; PREFIX_LEN];
        prefix.copy_from_slice(&rnd[..PREFIX_LEN]);
        Ok(Self {
            cipher,
            prefix,
            counter: 0,
            buf: Vec::with_capacity(CHUNK_SIZE * 2),
            started: false,
        })
    }

    fn header(&self) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(MAGIC);
        h.push(VERSION);
        h.extend_from_slice(&(CHUNK_SIZE as u32).to_be_bytes());
        h.extend_from_slice(&self.prefix);
        h
    }

    fn seal(&mut self, plain: &[u8], last: bool, out: &mut Vec<u8>) -> Result<()> {
        let n = nonce(&self.prefix, self.counter, last)?;
        let ct = self
            .cipher
            .encrypt(
                &n,
                Payload {
                    msg: plain,
                    aad: b"",
                },
            )
            .map_err(|_| crypto("falha ao cifrar bloco"))?;
        out.extend_from_slice(&ct);
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| crypto("arquivo grande demais"))?;
        Ok(())
    }

    /// Acrescenta dados em claro; o ciphertext pronto é anexado a `out`.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<()> {
        if !self.started {
            out.extend_from_slice(&self.header());
            self.started = true;
        }
        self.buf.extend_from_slice(data);
        // Só emite um bloco "não último" quando sobra ao menos 1 byte depois dele.
        while self.buf.len() > CHUNK_SIZE {
            let chunk: Vec<u8> = self.buf.drain(..CHUNK_SIZE).collect();
            self.seal(&chunk, false, out)?;
        }
        Ok(())
    }

    /// Fecha o arquivo emitindo o último bloco (vazio só se o arquivo for vazio).
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<()> {
        if !self.started {
            out.extend_from_slice(&self.header());
        }
        let rest = std::mem::take(&mut self.buf);
        self.seal(&rest, true, out)
    }
}

/// Decifrador incremental: valida cada bloco e rejeita truncamento, reordenação e lixo no fim.
pub struct Decryptor {
    cipher: XChaCha20Poly1305,
    prefix: Option<[u8; PREFIX_LEN]>,
    counter: u32,
    buf: Vec<u8>,
    chunk_size: usize,
}

impl Decryptor {
    pub fn new(key: &[u8; 32]) -> Result<Self> {
        Ok(Self {
            cipher: XChaCha20Poly1305::new_from_slice(key).map_err(|_| crypto("chave"))?,
            prefix: None,
            counter: 0,
            buf: Vec::new(),
            chunk_size: CHUNK_SIZE,
        })
    }

    fn open(&mut self, ct: &[u8], last: bool, out: &mut Vec<u8>) -> Result<()> {
        let prefix = self.prefix.ok_or_else(|| crypto("sem cabeçalho"))?;
        let n = nonce(&prefix, self.counter, last)?;
        let plain = self
            .cipher
            .decrypt(&n, Payload { msg: ct, aad: b"" })
            .map_err(|_| crypto("arquivo adulterado, truncado ou fora de ordem"))?;
        out.extend_from_slice(&plain);
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| crypto("arquivo grande demais"))?;
        Ok(())
    }

    /// Acrescenta ciphertext; o texto em claro dos blocos já completos é anexado a `out`.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<()> {
        self.buf.extend_from_slice(data);
        if self.prefix.is_none() {
            if self.buf.len() < HEADER_LEN {
                return Ok(());
            }
            let h: Vec<u8> = self.buf.drain(..HEADER_LEN).collect();
            if &h[..4] != MAGIC || h[4] != VERSION {
                return Err(crypto("não é um arquivo do Enxame (cabeçalho)"));
            }
            let cs = u32::from_be_bytes([h[5], h[6], h[7], h[8]]) as usize;
            if cs != CHUNK_SIZE {
                return Err(crypto("tamanho de bloco não suportado"));
            }
            self.chunk_size = cs;
            let mut p = [0u8; PREFIX_LEN];
            p.copy_from_slice(&h[9..]);
            self.prefix = Some(p);
        }
        let full = self.chunk_size + TAG;
        // Um bloco completo só é "não último" se houver ao menos 1 byte depois dele.
        while self.buf.len() > full {
            let ct: Vec<u8> = self.buf.drain(..full).collect();
            self.open(&ct, false, out)?;
        }
        Ok(())
    }

    /// Fecha: o que sobrou é o último bloco. Falha se o arquivo estiver truncado.
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<()> {
        if self.prefix.is_none() {
            return Err(crypto("arquivo cifrado vazio ou sem cabeçalho"));
        }
        let rest = std::mem::take(&mut self.buf);
        if rest.len() < TAG {
            return Err(crypto("arquivo truncado"));
        }
        self.open(&rest, true, out)
    }
}

/// Cifra um buffer inteiro (testes e arquivos pequenos).
#[cfg(test)]
pub fn encrypt_all(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>> {
    let mut enc = Encryptor::new(key)?;
    let mut out = Vec::new();
    enc.push(plain, &mut out)?;
    enc.finish(&mut out)?;
    Ok(out)
}

/// Decifra um buffer inteiro.
#[cfg(test)]
pub fn decrypt_all(key: &[u8; 32], ct: &[u8]) -> Result<Vec<u8>> {
    let mut dec = Decryptor::new(key)?;
    let mut out = Vec::new();
    dec.push(ct, &mut out)?;
    dec.finish(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const KEY: [u8; 32] = [7; 32];

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 + 7) as u8).collect()
    }

    #[test]
    fn roundtrip_at_every_interesting_size() {
        for n in [
            0,
            1,
            15,
            16,
            17,
            CHUNK_SIZE - 1,
            CHUNK_SIZE,
            CHUNK_SIZE + 1,
            2 * CHUNK_SIZE,
            2 * CHUNK_SIZE + 5,
            5 * CHUNK_SIZE + 123,
        ] {
            let plain = data(n);
            let ct = encrypt_all(&KEY, &plain).unwrap();
            assert_eq!(
                ct.len() as u64,
                encrypted_len(n as u64),
                "tamanho previsto para {n}"
            );
            assert_eq!(
                decrypt_all(&KEY, &ct).unwrap(),
                plain,
                "round-trip de {n} bytes"
            );
        }
    }

    #[test]
    fn streaming_in_odd_pieces_matches_one_shot() {
        let plain = data(3 * CHUNK_SIZE + 999);
        let ct = encrypt_all(&KEY, &plain).unwrap();
        for step in [1usize, 7, 4096, CHUNK_SIZE + 3, 1 << 20] {
            let mut dec = Decryptor::new(&KEY).unwrap();
            let mut out = Vec::new();
            for piece in ct.chunks(step) {
                dec.push(piece, &mut out).unwrap();
            }
            dec.finish(&mut out).unwrap();
            assert_eq!(out, plain, "passo {step}");
        }
    }

    #[test]
    fn wrong_key_is_rejected() {
        let ct = encrypt_all(&KEY, b"segredo").unwrap();
        assert!(decrypt_all(&[8; 32], &ct).is_err());
    }

    #[test]
    fn every_kind_of_tampering_is_detected() {
        let plain = data(3 * CHUNK_SIZE + 10);
        let ct = encrypt_all(&KEY, &plain).unwrap();
        let full = CHUNK_SIZE + TAG;

        // Bit trocado no meio de um bloco, na tag e no cabeçalho.
        for pos in [HEADER_LEN + 5, HEADER_LEN + full - 1, 2] {
            let mut t = ct.clone();
            t[pos] ^= 1;
            assert!(decrypt_all(&KEY, &t).is_err(), "bit em {pos}");
        }
        // Truncamento no fim de um bloco inteiro (o ataque que o bit "último" impede).
        let cut = HEADER_LEN + 2 * full;
        assert!(
            decrypt_all(&KEY, &ct[..cut]).is_err(),
            "truncado em fronteira de bloco"
        );
        // Truncamento no meio de um bloco e removendo só o último byte.
        assert!(decrypt_all(&KEY, &ct[..ct.len() - 1]).is_err());
        assert!(decrypt_all(&KEY, &ct[..HEADER_LEN + 100]).is_err());
        assert!(
            decrypt_all(&KEY, &ct[..HEADER_LEN]).is_err(),
            "só o cabeçalho"
        );
        // Lixo no fim.
        let mut t = ct.clone();
        t.push(0);
        assert!(decrypt_all(&KEY, &t).is_err(), "byte extra");
        // Reordenar dois blocos.
        let mut t = ct.clone();
        let (a, b) = (HEADER_LEN, HEADER_LEN + full);
        let first: Vec<u8> = t[a..a + full].to_vec();
        let second: Vec<u8> = t[b..b + full].to_vec();
        t[a..a + full].copy_from_slice(&second);
        t[b..b + full].copy_from_slice(&first);
        assert!(decrypt_all(&KEY, &t).is_err(), "blocos trocados");
        // Repetir um bloco no lugar de outro.
        let mut t = ct.clone();
        t.copy_within(a..a + full, b);
        assert!(decrypt_all(&KEY, &t).is_err(), "bloco repetido");
        // Sem nada.
        assert!(decrypt_all(&KEY, &[]).is_err());
        assert!(decrypt_all(&KEY, b"ENXF").is_err());
    }

    #[test]
    fn two_encryptions_of_the_same_file_differ() {
        let a = encrypt_all(&KEY, b"mesmo conteudo").unwrap();
        let b = encrypt_all(&KEY, b"mesmo conteudo").unwrap();
        assert_ne!(a, b, "o prefixo do nonce é aleatório");
    }

    proptest! {
        #[test]
        fn arbitrary_data_and_piece_sizes_roundtrip(
            n in 0usize..(3 * CHUNK_SIZE),
            step in 1usize..70_000,
        ) {
            let plain = data(n);
            let mut enc = Encryptor::new(&KEY).unwrap();
            let mut ct = Vec::new();
            for piece in plain.chunks(step.max(1)) {
                enc.push(piece, &mut ct).unwrap();
            }
            enc.finish(&mut ct).unwrap();
            prop_assert_eq!(ct.len() as u64, encrypted_len(n as u64));
            prop_assert_eq!(decrypt_all(&KEY, &ct).unwrap(), plain);
        }
    }
}
