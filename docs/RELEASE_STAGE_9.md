# Entrega etapa 9 de 18 — multitablero

## Nuevo

- `crates/optimizer-core/src/multi_board.rs`: selección y reserva de stock
  físico; múltiples patrones, reintentos y conciliación de pedidos.
- Operación JSON `generate_multi_board`; mantiene esquema `1.0`.
- `engine_version = 0.9.0`, `capabilities.stage = 9`.
- Generación de layouts listos para el validador independiente etapa 6.
- Pruebas nuevas en `tests/multi_board_generation.rs` y
  `optimizer-cli/tests/multi_board_cli.rs`.
- Ejemplos de inventario suficiente, insuficiente, mixto y tres etapas.

## Criterios de aceptación

- [x] Implementado sobre el proyecto acumulativo de las etapas 1–8.
- [x] Soporte de múltiples tableros físicos de diversos tipos de stock.
- [x] Patrón único por tablero, índices de inventario sin duplicación.
- [x] Se descuenta la cantidad de piezas colocadas sin exceder pedidos.
- [x] Revalidación independiente y respuesta completa o parcial.
- [x] Búsqueda con tiempo/recursos acotados y múltiples intentos.
- [x] Compatibilidad sintáctica estática de JSON/TOML y scripts.
- [ ] Compilación local con `cargo build --workspace` (Rust no instalado aquí).
- [ ] Ejecución de `cargo test --workspace` (Rust no instalado aquí).
- [ ] Medición con pedidos reales de eCarpintero (requiere muestras y aprobación).

## No incluido

La etapa 10 introducirá optimización de **apilado de tableros completos**.
No se declara que los planos sean ejecutables por una seccionadora específica
sin verificar pinzas, secuencias y perfiles concretos.
