# Entrega — Etapa 16 de 18

**Entregado en código fuente:** módulo `economics`, operación CLI/API `evaluate_economics`, comparación `compare_economic_layouts`, configuración de costos y recuperabilidad, cálculo de oportunidades de apilado de tableros completos, modelos de salida auditables, ejemplos JSON, pruebas Rust (pendientes de ejecución), comprobaciones estáticas y documentación.

**No entregado ni afirmado:** ejecución de Cargo/Rustc en este entorno, certificación de producción, precios reales calibrados, costeo de apilado de franjas, integración con React/ERP (etapa 17), benchmarks (etapa 18).

**Validación recomendada en equipo con Rust:**

```bash
cargo fmt --all
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_fixtures.py
```

No se debe avanzar a la etapa 17 sin autorización del usuario.
