//! Fronteira FFI para o Flutter. Vazia até a Fase 6; apenas reexporta a versão.

/// Versão do core, para checagem de compatibilidade do app.
pub fn core_version() -> String {
    enxame_core::Enxame::version().to_owned()
}
