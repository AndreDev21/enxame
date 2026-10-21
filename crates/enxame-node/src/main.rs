fn main() -> anyhow::Result<()> {
    println!(
        "enxame-node {}: esqueleto da Fase 0; o daemon chega na Fase 3.",
        enxame_core::Enxame::version()
    );
    Ok(())
}
