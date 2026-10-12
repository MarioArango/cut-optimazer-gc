# Etapa 7 — Generador automático de dos etapas (Rust)

## Alcance

La nueva operación `generate_two_stage` construye **un tablero** con un árbol guillotina real. Soporta los modos `V-H` y `H-V`, con franjas de anchura fija y cortes ortogonales para separar piezas. El motor explora cinco ordenaciones distintas de piezas (`longest_first`, `shortest_first`, `largest_area_first`, `best_fit` y `priority_first`), ambos sentidos si están autorizados y la rotación del tablero si es viable.

**No** asigna todos los tableros necesarios para el pedido (etapa 9), **no** decide el apilado (etapas 10–14), y **no** garantiza un óptimo global (etapa 15). La capacidad anunciada de optimización es `true` únicamente para la generación de un tablero de dos etapas.

La salida incluye `found`, `complete_order`, `candidates_evaluated`, `search_exhausted`, `remaining_part_counts`, y cuando hay solución: un `board` con el patrón autocontenido (`pattern`) y su evaluación geométrica (`evaluated`). La evaluación usa el motor guillotina de la etapa 5, con conservación exacta de área, espesores de sierra, veta, orientaciones y límites de longitud de corte. **La validación de producción y manipulación continúa pendiente**.

## Ejemplo

```bash
cargo run -p optimizer-cli -- run --input examples/generate-two-stage.json
```

Para una máquina que solo acepta `H-V`:

```bash
cargo run -p optimizer-cli -- run --input examples/generate-two-stage-hv.json
```

Para un caso sin orientaciones válidas de veta:

```bash
cargo run -p optimizer-cli -- run --input examples/generate-two-stage-no-fit.json
```

Contrato de entrada:

```json
{
  "schema_version": "1.0",
  "job_id": "two-stage-job",
  "options": { "seed": 42, "max_runtime_ms": 30000 },
  "operation": {
    "type": "generate_two_stage",
    "model": { "...": "modelo completo CuttingJobModel" },
    "stock_id": "board1000"
  }
}
```

`stock_id` es opcional; si no se suministra, el motor examina todos los tipos de stock (sin consumirlos). La mejor solución de esta etapa se elige primero por mayor **área colocada**, después mayor aprovechamiento del tablero y finalmente mayor número de piezas colocadas. La selección de stock según costo total corresponde a la etapa 9.

## Restricciones y trazabilidad

- Dimensiones en enteros de **0.1 mm**; `kerf` se descuenta físicamente en cada corte.
- Cada hoja `piece` corresponde a una unidad, y nunca supera la cantidad pedida para esa referencia.
- La asignación en un solo tablero no demuestra que todo el pedido esté resuelto; las referencias no colocadas aparecen en `remaining_part_counts`.
- La veta de piezas y tableros se alinea con las rotaciones admitidas.
- Cada patrón se representa por el mismo tipo `CuttingPattern` que usa el validador anterior.
- Las franjas tienen el mismo ancho (o alto) que las piezas que incluyen, con cortes internos ortogonales.
- Las regiones residuales se marcan inicialmente `reusable: false`; la valoración y clasificación de retazos reutilizables corresponde a la etapa 16.
- La búsqueda está acotada a 96 piezas por candidato, 96 franjas y un máximo de 48 anchuras candidatas. Puede devolver una solución parcial por presupuesto de búsqueda; `search_exhausted` indica expiración del límite entre candidatos, no demostración de optimalidad.
- El valor `utilization_basis_points` indica rendimiento sobre el **área útil** después de márgenes. Ejemplo: `8000` significa 80%.
- `stock_unit_index=0` es una referencia provisional de tablero único; no reserva existencias.
- El validador completo de pedido de etapa 6 continuará indicando piezas faltantes si se le presenta únicamente un tablero parcial.

## Pruebas

```bash
cargo fmt --all -- --check
cargo test --workspace
python3 scripts/check_fixtures.py
node --check integrations/node/two-stage-example.mjs
```

Es obligatorio ejecutar las pruebas Rust para declarar el comportamiento compilado verificado. Las comprobaciones estáticas no sustituyen `cargo test`.
