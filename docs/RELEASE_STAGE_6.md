# Entrega etapa 6 / 18 — validación de plano completo

Esta entrega es **acumulativa**: incorpora todo el código fuente de las etapas 1–5, más:

- `crates/optimizer-core/src/layout_validation.rs`: verificador independiente de layout de varios tableros, cortes, área, stock y cantidades.
- Contratos `JobOperation::ValidateLayout` y `JobOutput::LayoutValidation`.
- `examples/layout-valid.json` y diez ejemplos de errores, junto a `expected-layout-validation.json`.
- Tests Rust unitarios, de API y de la CLI. Comparación con respuesta de referencia.
- `integrations/node/layout-example.mjs` como ejemplo sin dependencias de Node.js.
- Documentación en `docs/LAYOUT_VALIDATION.md`, contrato y criterios de aceptación.

Verificación estática practicada: JSON, TOML, cálculos independientes en Python, sintaxis Bash y Node.js e integridad ZIP. **La compilación Rust y `cargo test` están pendientes porque Rust/Cargo no está instalado en este entorno.** Ejecutar `cargo test --workspace` en el equipo del desarrollador o en CI antes de usar en producción.

**No se implementó la etapa 7** ni un generador de cortes. No existe aún validación física de paquetes apilados ni compatibilidad completa de la seccionadora.
