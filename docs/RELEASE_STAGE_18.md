# Etapa 18 — Herramientas de validación, rendimiento y aceptación

## Alcance entregado

- Se mantiene el proyecto en Rust sin Node.js.
- Contrato HTTP JSON independiente; petición industrial real en `examples/postman-industrial-real.json`.
- `optimizer-bench` mide `optimizer-core` en proceso y, opcionalmente, el endpoint HTTP con concurrencia configurable.
- Las pruebas de aceptación HTTP cubren: JSON industrial, validación negativa, regresión multitablero y capacidades.
- CI extendido con un benchmark smoke de solo 1 iteración, que **no representa una medición del rendimiento real**.
- Integridad de contratos JSON, TOML y ZIP verificable sin Cargo.

## Estado de verificación de esta entrega

Se realizaron validaciones estáticas de estructura, JSON y TOML. El entorno generador NO tiene `rustc` ni `cargo`, ni puede acceder a los servidores de Rust; por lo tanto, **no se compiló el código, no se ejecutaron las pruebas y no se obtuvieron benchmarks reales**.

Para cerrar el criterio de aceptación es necesario ejecutar en un entorno con Rust y red para resolver dependencias:

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo build --release --workspace
cargo run --release -p optimizer-bench -- --fixture examples/generate-multi-board.json --iterations 20 --warmup 3
cargo run --release -p optimizer-api
```

En otra terminal, ejecutar desde Postman el caso **18 - Pedido real melamina 2440 x 1830**.

## Criterios de aceptación funcional que faltan

1. Todos los tests pasan en macOS y/o Linux.
2. Las solicitudes REST validan `schema_version`, ID, unidades y cantidades.
3. El resultado generado pasa el validador geométrico y no utiliza stock duplicado.
4. Se validan físicamente al menos dos secuencias de corte y apilado con la máquina de destino.
5. Los benchmarks registran p50/p95, memoria y CPU del equipo real, con referencia basal y comparación bajo el mismo presupuesto.

No se declara el motor listo para producción ni certificado para una máquina concreta hasta que estos criterios se satisfagan.
