use std::path::PathBuf;

#[cfg(feature = "os-keyring")]
use crate::Error;
use crate::{Config, Result};

/// Armazenamento do material de chaves. Implementações específicas de SO ficam fora da
/// lógica central (spec 17.1).
pub trait KeyStore: Send + Sync {
    /// Lê o blob guardado, se existir.
    fn load(&self) -> Result<Option<Vec<u8>>>;
    /// Substitui o blob guardado.
    fn save(&self, bytes: &[u8]) -> Result<()>;
    /// Nome legível do backend, para `diag` e logs.
    fn describe(&self) -> &'static str;
}

/// Fallback em arquivo com permissão restrita (0600 em Unix). Em Windows depende das ACLs do
/// perfil do usuário. Limitação documentada na spec 6.1.
#[derive(Debug, Clone)]
pub struct FileKeyStore {
    path: PathBuf,
}

impl FileKeyStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl KeyStore for FileKeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>> {
        match std::fs::read(&self.path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "arquivo (permissão restrita)"
    }
}

/// Keystore do SO via `keyring`: Secret Service (Linux), Credential Manager (Windows),
/// Keychain (macOS).
#[cfg(feature = "os-keyring")]
pub struct OsKeyStore {
    entry: keyring::Entry,
}

#[cfg(feature = "os-keyring")]
impl OsKeyStore {
    /// `profile` distingue múltiplos diretórios de dados na mesma conta de usuário.
    pub fn new(profile: &str) -> Result<Self> {
        let entry = keyring::Entry::new("enxame", &format!("identity:{profile}"))
            .map_err(|e| Error::KeyStore(e.to_string()))?;
        Ok(Self { entry })
    }
}

#[cfg(feature = "os-keyring")]
impl KeyStore for OsKeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>> {
        match self.entry.get_secret() {
            Ok(b) => Ok(Some(b)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::KeyStore(e.to_string())),
        }
    }

    fn save(&self, bytes: &[u8]) -> Result<()> {
        self.entry
            .set_secret(bytes)
            .map_err(|e| Error::KeyStore(e.to_string()))
    }

    fn describe(&self) -> &'static str {
        "keystore do SO"
    }
}

/// Escolhe o backend: keystore do SO se disponível; senão arquivo em `<data_dir>/identity.key`.
pub fn open_keystore(cfg: &Config) -> Box<dyn KeyStore> {
    #[cfg(feature = "os-keyring")]
    {
        // Sonda o backend: sem Secret Service (WM puro sem gnome-keyring/kwallet) cai no arquivo.
        match OsKeyStore::new(&cfg.data_dir.display().to_string()).and_then(|s| s.load().map(|_| s))
        {
            Ok(store) => return Box::new(store),
            Err(e) => {
                tracing::warn!(
                    "keystore do SO indisponível ({e}); usando arquivo com permissão restrita"
                )
            }
        }
    }
    Box::new(FileKeyStore::new(cfg.data_dir.join("identity.key")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new(dir.path().join("sub/identity.key"));
        assert!(store.load().unwrap().is_none());
        store.save(b"segredo").unwrap();
        assert_eq!(store.load().unwrap().unwrap(), b"segredo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("sub/identity.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
