# Entrega de etapa 13 / 18

## Se añadió

- `crates/optimizer-core/src/dynamic_stacking.rs`: planificador dinámico por árboles de cortes pendientes, con mantenimiento, separación y reagrupación de paquetes de franjas.
- `plan_dynamic_stacks` en el contrato Rust/JSON (`JobOperation`, `JobOutput`) y capacidades de la CLI.
- Ejemplos para continuidad por dos cortes, rutas divergentes, recuperación/reagrupación, restricciones de máquina, costos desfavorables y corte inválido.
- Pruebas unitarias y de CLI para los principales casos, casos límite y determinismo.
- Documentación detallada en `docs/DYNAMIC_STACKING.md`.

## Estado de verificación

- Se verificaron sintácticamente los archivos JSON y TOML, y los scripts del proyecto.
- Las pruebas Rust están redactadas, **pero no ejecutadas**: Cargo/Rustc no están instalados y no fue posible descargarlos por bloqueo de DNS de este entorno.
- No se declara como compilado o seguro para producción. Es imprescindible ejecutar `cargo fmt --all -- --check`, `cargo build --workspace` y `cargo test --workspace` en un entorno Rust conectado.
- No se implementa todavía el planificador industrial de máquina (etapa 14).
