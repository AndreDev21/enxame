//! Fronteira FFI para o Flutter (flutter_rust_bridge v2). Toda a lógica fica em `enxame-core`;
//! aqui só há tipos simples (strings, números, listas) e funções que o Dart chama.
//!
//! Regenerar a ponte: `scripts/app/gen-bridge.sh` (precisa de `flutter_rust_bridge_codegen 2.13.0`).

pub mod api;
// Código gerado: usa `unsafe` (símbolos `extern "C"`) e não segue os lints do workspace.
#[allow(
    unsafe_code,
    clippy::all,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod frb_generated;
