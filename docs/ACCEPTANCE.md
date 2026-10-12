# Criterios de aceptación — etapa 6

- [x] Se conserva el proyecto de las etapas 1–5.
- [x] Nuevo módulo `layout_validation.rs` separado del evaluador/constructor de árboles.
- [x] Entrada de layouts con tablero físico indexado y patrones repetibles.
- [x] Control de consumo de stock, identificadores duplicados, referencias y compatibilidad de material.
- [x] Verificación independiente del kerf, cortes, rotaciones, etapas, veta, medidas y áreas.
- [x] Detección de solapes y regiones fuera del tablero por barrido geométrico.
- [x] Conciliación del pedido completo: faltantes, excedentes y áreas globales.
- [x] Errores estructurados `path` / `code` / `message`, sin bloquear la devolución del informe.
- [x] Nuevos fixtures válidos/inválidos y tests unitarios/integración/CLI escritos.
- [x] Verificación estática de ejemplos y ZIP preparado.
- [ ] Compilación real con Rust/Cargo ejecutada en este entorno (herramientas no disponibles).
- [ ] Tests Rust ejecutados y confirmados aquí (pendientes de toolchain).

## Verificación local / CI

```bash
python3 scripts/check_fixtures.py
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

## No incluido

No genera planos, no optimiza tableros, no planifica producción, no optimiza ni valida operaciones físicas de apilado. Tampoco certifica que un árbol geométrico resulte ejecutable en cualquier máquina real. La etapa 7 (generador de 2 etapas) requiere autorización.


### Etapa 8: criterios de aceptación

- Los ejemplos de V-H-V y H-V-H producen una solución con `stages_used == 3` cuando se ejecuta Rust.
- El árbol cumple `evaluate_pattern`, con conservación de área, kerf y cantidades.
- El beam limita estado, candidatos, tiempo y expansiones.
- La operación JSON antigua continúa aceptando los mismos contratos.
- `cargo test --workspace` y `cargo clippy` deben pasar antes de aprobar definitivamente la etapa.

## Etapa 9: criterios de aceptación

- [x] Integración de `generate_multi_board` con CLI, contrato JSON y núcleo Rust.
- [x] Asignación por stock-unit index y selección de patrones V-H/H-V/V-H-V/H-V-H.
- [x] Reconciliación de cantidades y reporte de sobrantes pendientes.
- [x] Múltiples reintentos acotados y evaluación económica preliminar.
- [x] Validador independiente etapa 6 aplicado a la solución global.
- [x] Pruebas Rust de inventario, costo, límite, material, salida parcial y CLI escritas.
- [x] JSON/TOML/fixtures revisados estáticamente; integridad del ZIP verificable.
- [ ] `cargo build --workspace` ejecutado y confirmado con toolchain disponible.
- [ ] `cargo test --workspace` ejecutado y confirmado con toolchain disponible.
- [ ] Benchmarks con pedidos reales / confirmación de desempeño industrial.

**Siguiente: etapa 10, apilados de tableros completos; requiere aprobación.**

## Etapa 10: criterios de aceptación

- [x] Planificador de apilados sobre layouts verificados de la etapa 6.
- [x] Identidad estructural de patrón independiente de IDs de nodos.
- [x] Restricción conservadora de mismo stock, orientación, tipo y cortes.
- [x] Límite simultáneo de altura total y miembros por paquete.
- [x] Distingue retazos y máquinas sin apilados de tableros completos.
- [x] Rechaza combinaciones cuyo ahorro de líneas no compensa el costo proxy.
- [x] Garantía estructural de no duplicar ni perder tableros asignados.
- [x] Operaciones JSON directas y compuestas, ejemplos, pruebas Rust escritas.
- [x] Ficheros TOML/JSON y scripts revisados estáticamente.
- [ ] Compilación Rust y todas las pruebas ejecutadas con Cargo.
- [ ] Secuencias físicas de apilado y sujeción validadas para máquinas reales.

**Siguiente: etapa 11 (simulador de producción), solo tras aprobación.**

## Etapa 11: criterios de aceptación

- [x] Simulador físico con estado por unidad, paquete, estación, genealogía y stock.
- [x] Operaciones explícitas: `stack`, `unstack`, `cut_workpiece`, `cut_stack`, `rotate_workpiece`, `rotate_stack`, `move_workpiece`, `move_stack`, `finish`, `scrap`.
- [x] Kerf por capa, pasadas de sierra compartidas y balance de área después de cada operación.
- [x] Control de altura, número de capas, compatibilidad, stock duplicado y referencias consumidas.
- [x] API `simulate_production` y CLI, ejemplos correctos e inválidos.
- [x] Pruebas Rust escritas y fixtures independientes revisados estáticamente.
- [ ] Compilación Rust del proyecto acumulativo con Cargo.
- [ ] Ejecución de toda la batería Rust; pruebas físicas de taller y certificación por máquina.

**Siguiente: etapa 12, apilados de franjas. No iniciada.**


## Entrega etapa 13 (estado actualizado)

El motor añade `dynamic_stacking.rs` con rutas binarias por franja y reproducción completa de operaciones mediante `production_simulator.rs`. Puede mantener paquetes, cortar franjas simultáneamente, separar rutas divergentes y reagrupar material procedente de distintos tableros. El resultado es una **propuesta de producción simulada, no un programa certificado de máquina**. La compilación y ejecución de tests Rust están pendientes por falta de toolchain en este entorno. La etapa 14 no ha comenzado.
