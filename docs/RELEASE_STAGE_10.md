# Entrega 10 / 18 — Apilado de tableros completos

- Versión del workspace: `0.10.0`; contrato `schema_version=1.0`.
- Nuevos módulos: `full_board_stacking.rs` y `stacked_multi_board.rs`.
- Nuevas operaciones: `plan_full_board_stacks` y `generate_stacked_multi_board`.
- Conserva las operaciones de etapas 1–9 y todos los ejemplos anteriores.
- Análisis exacto de identidad de patrón, control de altura/miembros e
  identidad de tableros; evaluación de conveniencia con costo proxy.
- Por diseño **no** certifica ni ejecuta secuencia física ni cortes de franjas.
- No se ha compilado ni ejecutado `cargo test` en este entorno. Realizar:

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_fixtures.py
```

Más detalles: `docs/FULL_BOARD_STACKING.md`.
