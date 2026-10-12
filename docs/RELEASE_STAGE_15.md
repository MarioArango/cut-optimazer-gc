# Entrega — Etapa 15 / 18

Añadidos: `global_optimizer.rs`, contrato `optimize_global`, documentación, JSON de demostración,
pruebas del núcleo y CLI. Se preservan todas las etapas 1–14.

**Implementado en fuente:** destrucción/reparación de varios tableros (LNS), reservas de inventario,
heurística de repetición de patrones, evaluación material + trabajo de apilado completo,
comprobación independiente, control de tiempo e informes de mejora.

**Verificaciones locales:** JSON/TOML, sintaxis de scripts, regresión estática de fixtures, integridad ZIP.

**Pendiente:** Cargo/Rustc no está instalado y el host de Rust no resuelve DNS, por tanto la compilación,
`cargo test --workspace`, `cargo clippy` y pruebas de rendimiento no se han ejecutado.
No se garantiza aún que el código compile o supere la totalidad de las pruebas.

**No incluido:** coste real calibrado (etapa 16), intercambio avanzado guiado por tiempo
físico de apilado/desapilado de franjas, exportadores industriales (etapa 17),
benchmarks y escalabilidad final (etapa 18).

La etapa 16 requiere autorización expresa del usuario.
