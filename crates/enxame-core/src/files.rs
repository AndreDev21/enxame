//! `FileRef`: a referência a um arquivo que viaja dentro de um envelope E2E (spec 10).
//!
//! O arquivo em si fica cifrado nos blobs; só quem recebe o envelope tem a `file_key`.

use serde::{Deserialize, Serialize};

use crate::{filecrypt::encrypted_len, Error, Result};

/// Maior nome de arquivo aceito, em caracteres.
pub const MAX_NAME_CHARS: usize = 255;
const MAX_MIME_CHARS: usize = 127;

/// Conteúdo de um envelope `kind = FileRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Raiz (`HashSeq`) dos segmentos de ciphertext no `iroh-blobs`.
    pub blob_hash: [u8; 32],
    /// Tamanho do arquivo em claro, em bytes.
    pub size: u64,
    pub name: String,
    pub mime: String,
    /// Chave do arquivo (XChaCha20-Poly1305 em streaming). Só vai dentro do envelope E2E.
    pub file_key: [u8; 32],
}

impl FileRef {
    /// Tamanho do ciphertext que a raiz deve conter.
    pub fn enc_size(&self) -> u64 {
        encrypted_len(self.size)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
    }

    /// Decodifica e valida. `max_bytes` é o teto de tamanho aceito.
    pub fn decode(bytes: &[u8], max_bytes: u64) -> Result<Self> {
        let f: FileRef = postcard::from_bytes(bytes)
            .map_err(|e| Error::Protocol(format!("FileRef inválido: {e}")))?;
        f.validate(max_bytes)?;
        Ok(f)
    }

    pub fn validate(&self, max_bytes: u64) -> Result<()> {
        if self.size > max_bytes {
            return Err(Error::Protocol(format!(
                "arquivo de {} bytes excede o limite de {max_bytes}",
                self.size
            )));
        }
        let n = self.name.chars().count();
        if n == 0 || n > MAX_NAME_CHARS {
            return Err(Error::Protocol(
                "nome de arquivo com tamanho inválido".into(),
            ));
        }
        if self.mime.chars().count() > MAX_MIME_CHARS
            || !self.mime.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(Error::Protocol("tipo MIME inválido".into()));
        }
        Ok(())
    }

    /// Descrição curta para exibição ("📎 relatorio.pdf (1.2 MB)").
    pub fn describe(&self) -> String {
        format!(
            "📎 {} ({})",
            sanitize_name(&self.name),
            human_size(self.size)
        )
    }
}

/// Nome seguro para gravar em disco: sem separadores de caminho, controles nem pontos iniciais,
/// e nunca vazio. O nome original só é usado para exibição.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        "arquivo".to_owned()
    } else {
        trimmed.chars().take(MAX_NAME_CHARS).collect()
    }
}

/// "1.2 MB", "340 KB", "12 B".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Tipo MIME pela extensão (o suficiente para a UI; o conteúdo nunca é interpretado).
pub fn guess_mime(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("txt" | "md" | "log") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp3") => "audio/mpeg",
        Some("ogg" | "opus") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileRef {
        FileRef {
            blob_hash: [1; 32],
            size: 1234,
            name: "relatório.pdf".into(),
            mime: "application/pdf".into(),
            file_key: [2; 32],
        }
    }

    #[test]
    fn roundtrip_and_validation() {
        let f = sample();
        assert_eq!(FileRef::decode(&f.encode().unwrap(), 1 << 30).unwrap(), f);
        assert_eq!(f.enc_size(), encrypted_len(1234));
        // Acima do limite, nome vazio ou gigante, MIME com espaço ou controle.
        assert!(FileRef::decode(&f.encode().unwrap(), 1000).is_err());
        for bad in [
            FileRef {
                name: String::new(),
                ..sample()
            },
            FileRef {
                name: "a".repeat(256),
                ..sample()
            },
            FileRef {
                mime: "text plain".into(),
                ..sample()
            },
            FileRef {
                mime: "x\u{0}y".into(),
                ..sample()
            },
        ] {
            assert!(bad.validate(1 << 30).is_err(), "{bad:?}");
        }
        assert!(FileRef::decode(b"lixo", 1 << 30).is_err());
        assert!(FileRef::decode(&[], 1 << 30).is_err());
    }

    #[test]
    fn names_cannot_escape_the_download_directory() {
        for (input, want) in [
            ("../../etc/passwd", "_.._etc_passwd"),
            ("/abs/olute", "_abs_olute"),
            ("..\\win\\dir", "_win_dir"),
            (".bashrc", "bashrc"),
            ("...", "arquivo"),
            ("   ", "arquivo"),
            ("a\u{0}b\nc", "a_b_c"),
            ("normal.txt", "normal.txt"),
            ("relatório final.pdf", "relatório final.pdf"),
        ] {
            let got = sanitize_name(input);
            assert_eq!(got, want, "{input:?}");
            assert!(!got.contains('/') && !got.contains('\\') && !got.starts_with('.'));
        }
        assert_eq!(
            sanitize_name(&"x".repeat(400)).chars().count(),
            MAX_NAME_CHARS
        );
    }

    #[test]
    fn size_and_mime_helpers() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(guess_mime("foto.JPG"), "image/jpeg");
        assert_eq!(guess_mime("sem_extensao"), "application/octet-stream");
        assert!(sample().describe().starts_with("📎 relatório.pdf"));
    }
}
