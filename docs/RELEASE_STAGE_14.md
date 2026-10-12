# Entrega — Etapa 14 / 18

**Objeto:** incorporar planificación industrial de un conjunto de operaciones físicas existentes, compatible con el simulador y las etapas guillotina.

**Módulo añadido:** `crates/optimizer-core/src/industrial_scheduler.rs`.

**Nuevos contratos:** `schedule_production` y `industrial_scheduling`.

**Pruebas nuevas:** `crates/optimizer-core/tests/industrial_scheduler.rs` y `crates/optimizer-cli/tests/industrial_scheduler_cli.rs`.

**Fixtures:** `examples/schedule-production*.json` (ordenamiento, plan terminal, ciclo y etapa guillotina imposible).

**Pruebas cubiertas:** consistencia del material, restricciones por rama, dependencias, errores de ciclo, costos configurables, reproducción determinista y respuesta JSON de CLI.

**Verificación pendiente:** Cargo/Rustc no disponibles en el entorno de desarrollo. El proyecto se entrega con verificaciones estáticas y pruebas Rust **escritas, pero no ejecutadas**. La compilación y los tests completos son requisito antes de cualquier integración en producción.

**No incluido:** optimización LNS global (etapa 15), CNC vendor-specific, modelo detallado de mordazas/posicionamiento, predicción calibrada de tiempos.
