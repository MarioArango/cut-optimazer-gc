# Entrega — Etapa 8 / 18

**Módulo nuevo:** `crates/optimizer-core/src/three_stage.rs`.

**Contrato añadido:** operación `generate_three_stage`, respuesta `three_stage_generation`.

**Algoritmo:** Beam Search acotado para un solo tablero V-H-V o H-V-H. Generación de anchos de franjas, filas y divisiones finales con kerf, rotación y veta; evaluación geométrica independiente del árbol generado.

**Compatibilidad:** se conserva la API JSON `1.0` y todas las operaciones de las etapas 1–7. `engine_version` pasa a `0.8.0`; capacidades declara `stage: 8`.

**Ejemplos:** `generate-three-stage-vhv.json`, `generate-three-stage-hvh.json`, `generate-three-stage-no-fit.json`.

**Tests nuevos:** integración Rust del núcleo y CLI para patrón real de tres etapas, modos, cantidades pendientes, reproducción, errores de configuración y casos de no encaje.

**Compilación:** no verificada en el entorno de preparación por ausencia de Rust/Cargo y DNS externo. La revisión estática de JSON/TOML y estructura no sustituye `cargo test`.

**Fuera de alcance:** selección y consolidación multitablero, apilados, simulación de producción, optimización global, costos y validación física específica de máquina.
