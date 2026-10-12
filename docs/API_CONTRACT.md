# Contrato JSON — versión 0.15.0 (etapa 12)

`schema_version: "1.0"` permanece. `engine_version` es `0.15.0`. Se conservan `system_check`, `validate_model` y `preprocess_model`.

## Cortar un rectángulo

`examples/geometry-vertical-split.json`:

```json
{
  "schema_version": "1.0",
  "job_id": "geometria-corte-vertical-001",
  "options": { "seed": 2026, "max_runtime_ms": 30000 },
  "operation": {
    "type": "geometry_split",
    "rect": { "x": 100, "y": 200, "width": 1000, "height": 600 },
    "direction": "vertical",
    "at": 400,
    "kerf": 32
  }
}
```

La salida completa `examples/expected-geometry-vertical-split.json` contiene: `first`, `saw_path`, `second`. `32` unidades de `kerf` representan 3.2 mm.

## Rectángulo útil

`examples/geometry-trim.json` recibe un `rect` más `margins` con `top/right/bottom/left`. Devuelve `output.type = "geometry_trim"` y `output.usable`. Ver `examples/expected-geometry-trim.json`.

## Rechazos y estabilidad

- JSON desconocido o inválido -> error CLI 3.
- Ceros, desbordes y cortes sin dos regiones positivas -> error CLI 3.
- Estos errores también se comprueban con `validate --input`.
- Las operaciones de generación anuncian `optimization_available: true`, sin garantía de optimalidad.
- Las operaciones de las etapas anteriores se mantienen sin cambios semánticos.

Ver `GEOMETRY.md` y `PREPROCESSING.md`.


## Árbol de cortes guillotina (etapa 5)

Nueva operación `guillotine_evaluate`, con `model` (modelo de etapa 2), `pattern_id` y `board_rotated_90` (por defecto false).

```json
{
  "schema_version": "1.0",
  "job_id": "guillotine-demo",
  "operation": {
    "type": "guillotine_evaluate",
    "model": { "...": "ver examples/guillotine-vhv.json" },
    "pattern_id": "pattern-01",
    "board_rotated_90": false
  }
}
```

La salida es `output.type = "guillotine_evaluation"` y `output.evaluated = {pattern_id, stock_id, board_rotated_90, mode, usable_rect, root, geometric_cuts, pieces, residuals, summary}`. Los campos de área de `summary` son strings decimales exactos. Este ejemplo es **ilustrativo**, el JSON de `model` debe ser un modelo completo.

`validate_model` conserva su alcance estructural histórico; `guillotine_evaluate` incorpora la comprobación geométrica del patrón seleccionado. Ni esta operación ni su respuesta garantizan ejecución en máquina o que todo el pedido esté completo.

Más detalles: `docs/GUILLOTINE.md`.


## Validación de plano completo (etapa 6)

Operación nueva: `validate_layout`. Mantiene `schema_version: "1.0"` y entrega `output.type: "layout_validation"`.

```json
{
  "schema_version": "1.0",
  "job_id": "ejemplo-plan-completo",
  "operation": {
    "type": "validate_layout",
    "model": { "materials": [], "parts": [], "stocks": [], "machine": {} },
    "layout": {
      "boards": [
        {
          "board_id": "tablero-1",
          "stock_id": "stock-2440",
          "stock_unit_index": 0,
          "pattern_id": "patron-A",
          "board_rotated_90": false
        }
      ]
    }
  }
}
```

**Nota:** el `model` anterior es un marcador abreviado, no un ejemplo ejecutable. Utiliza `examples/layout-valid.json` para una solicitud completa.

- `stock_unit_index` empieza en **cero**, por cada `stock_id`. Identifica una hoja física; repetir el par `(stock_id, stock_unit_index)` invalida el plano.
- `pattern_id` referencia un patrón existente, repetible sobre distintos tableros.
- `report.valid` indica si **todo el pedido** se encuentra colocado exactamente y si todos los tableros/árboles se han validado.
- `report.summary` contiene recuentos globales, faltantes y excedentes por producto, stock utilizado y áreas exactas como strings decimales.
- Problemas de geometría o cantidades devuelven una **respuesta JSON normal**, con `report.valid: false`, `issues` y exit code **0** en `optimizer-cli run`. Los consumidores deben inspeccionar `report.valid`.
- Errores de contrato o JSON se rechazan con código de salida **3**. `optimizer-cli validate` comprueba el contrato, no es sustituto de `run`.
- `geometry_validated=true` significa consistencia geométrica **del conjunto completo**, no factibilidad mecánica. `production_validated=false` siempre.

Ejemplos: `layout-valid.json`, `layout-invalid-missing.json`, `layout-invalid-excess.json`, `layout-invalid-duplicate-stock.json`, `layout-invalid-piece-size.json`, entre otros.

## Generación de un tablero (`generate_two_stage`)

Ver `TWO_STAGE.md` y `examples/generate-two-stage.json`. La salida devuelve `two_stage_generation` y `result` con patrón y evaluación válidos o razones de no colocación. No es optimización global del pedido.

## Etapa 8 — `generate_three_stage`

Nueva operación de un solo tablero con `model`, `stock_id?`, `beam_width?` (1..64, default 12), `max_expansions?` (1..20000, default 1500) y opciones comunes `seed` y `max_runtime_ms`. La salida `three_stage_generation.result` incluye `found`, `complete_order`, `board.pattern`, `board.evaluated`, conteos pendientes, `candidates_evaluated`, `expanded_states`, `search_exhausted` y `beam_width`. Solo son admisibles patrones `V-H-V` y `H-V-H` configurados en la máquina. Consultar `docs/THREE_STAGE.md`.


## Generación automática multitablero (etapa 9)

Se añade `generate_multi_board` con `model`, `max_boards` (por defecto
128), `restarts` (por defecto 3), `beam_width` (12) y `max_expansions`
(1500). La respuesta es `output.type: "multi_board_generation"`.

El resultado contiene `complete_order`, `search_exhausted`, `boards`,
`layout`, `validation`, `remaining_part_counts`, `stock_usage`,
`stock_cost_minor` y `placed_piece_area_sq_units`. Los dos últimos son
strings enteros para que JavaScript no pierda exactitud.

`complete_order` indica que todas las cantidades fueron colocadas **y** la
validación independiente aceptó la solución. Si faltan piezas, se devuelve
un resultado parcial con `validation.valid=false`; no se fabrica una
declaración artificial de inviabilidad. Ver `docs/MULTI_BOARD.md` y
`examples/generate-multi-board.json`.


## Etapa 10: apilados de tableros completos

Nuevas operaciones: `plan_full_board_stacks` (modelo + layout ya existente +
parámetros `costs` opcionales) y `generate_stacked_multi_board` (parámetros
de `generate_multi_board` + `costs`). Ejemplos completos en
`examples/plan-full-board-stacks.json` y
`examples/generate-stacked-multi-board.json`.
El resultado identifica grupos de tableros iguales, tableros sueltos,
limitación por altura y miembros, y estima ahorro de líneas de corte.
`physical_execution_validated` siempre es `false` en esta etapa.


## Etapa 11 — `simulate_production`

Entrada: `operation: {"type":"simulate_production", "model": CuttingJobModel, "plan": ProductionPlan}`. Salida exitosa: `output: {"type":"production_simulation", "result": ProductionSimulation}`. Los errores de secuencia invalidan la solicitud e identifican `production operation N [codigo]`.

Ejemplos en `examples/simulate-production-stack-cut.json` y `examples/simulate-production-single.json`; especificación en `docs/PRODUCTION_SIMULATOR.md`. El motor **simula operaciones suministradas**, sin afirmar ejecución industrial certificada.

## Operación `plan_strip_stacks` (etapa 12)

`operation: {type:"plan_strip_stacks", model, plan, next_cuts, stack_options?}`. Cada entrada de `next_cuts` indica `workpiece_id`, `direction` y `at` (inicio del canal de sierra en décimas de mm). `plan` es el prefijo de operaciones físicas de la etapa 11.

La salida `strip_stack_planning` incluye `groups`, `operations_added`, `ungrouped_workpiece_ids`, `base_simulation`, `projected_simulation`, `saw_passes_avoided` y `estimated_net_savings_minor`. La geometría propuesta se valida repitiendo la secuencia completa del simulador. No incluye una secuencia de CNC certificada.

## Operación agregada en etapa 13: `plan_dynamic_stacks`

Entrada: `{ "type": "plan_dynamic_stacks", "model": CuttingJobModel, "plan": ProductionPlan, "routes": WorkpieceRoute[], "dynamic_options"?: DynamicStackOptions }`.

Salida: `{ "type": "dynamic_stack_planning", "result": DynamicStackResult }`.

Cada `route` contiene `workpiece_id` y `route` (nodos `{kind:"done"}` o `{kind:"cut", direction,at,first,second}`). Las operaciones de salida son aditivas al prefijo; la simulación proyectada valida el registro físico y la conservación de área. Detalles y limitaciones en `docs/DYNAMIC_STACKING.md`.

## Etapa 14 — `schedule_production`

Entrada: `model`, `plan` con operaciones físicas no ordenadas, `dependencies?: [{ before, after }]` (índices base cero) y `schedule_options?`.

Salida `output.type = "industrial_scheduling"`, con `ordered_source_indices`, `scheduled_plan`, `steps`, `simulation`, métricas y costo orientativo. Consultar [el planificador](INDUSTRIAL_SCHEDULER.md). No genera instrucciones CNC listas para la máquina.

## Novedades de la etapa 16 (motor 0.16.0)

El `schema_version` sigue siendo `1.0`. `engine_version` cambia a `0.16.0`.

- `evaluate_economics`: `{ model, layout, economic_options? }`, salida `economic_evaluation.result`.
- `compare_economic_layouts`: `{ model, layouts, economic_options? }`, salida `economic_comparison.result`, con `selected_layout_index` indexado desde cero.

Consultar `docs/ECONOMICS.md` para fórmulas, unidades, parámetros y límites. Las operaciones previas permanecen disponibles.
