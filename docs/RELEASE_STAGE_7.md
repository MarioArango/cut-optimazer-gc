# Etapa 7 — Entrega

**Incluye**: librería Rust de generación automática para un solo tablero (V-H / H-V), heurísticas, interfaz JSON, integración CLI/Node y tests nuevos. Mantiene el código de etapas 1–6. No implementa algoritmos de 3 etapas, optimización multitablero ni apilados.

**Limitación de verificación de esta entrega**: Este entorno no dispone de `rustc` ni `cargo`, por lo que no es posible declarar compilación ni ejecución de pruebas Rust completadas. Se incluyen las pruebas y configuración de CI para ejecutarlas en un equipo con Rust instalado.

## Comandos

```bash
cargo build --workspace
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/generate-two-stage.json
```

No avanzar a la etapa 8 sin aprobación.
