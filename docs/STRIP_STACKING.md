# Etapa 12 — Apilados de franjas (Rust)

## Contrato

`plan_strip_stacks` acepta un `model`, un `plan` que contiene las operaciones físicas *ya realizadas*, un arreglo `next_cuts` con el próximo corte de cada franja candidata y `stack_options` opcional.

- **Unidades:** décimas de mm para medidas y posiciones de corte; costos en unidades monetarias menores (ej. céntimos).
- **Candidato:** cualquier franja/región **activa** de la simulación de prefijo que tenga un próximo corte válido. No se admite directamente un tablero completo.
- **Compatibilidad:** material, espesor, ancho, alto, veta, orientación, borde de registro, estación y dirección/posición del corte deben coincidir. No se mezclarán capas de distinto tamaño.
- **Paquetes ya formados:** si todas las capas del paquete tienen el mismo próximo corte, el paquete se mantiene; no se genera un `stack` redundante.
- **Paquetes nuevos:** las franjas libres compatibles se agrupan hasta los límites de altura/capas de la máquina. Solo si `stacking.strips=true` y `stacking.restacking=true`.
- **Preferencia:** mayor ahorro marginal de operaciones. El ahorro es estimado: `(capas-1) * saw_pass_cost - stack_handling_cost - followup_handling_cost`. Se omite el costo inicial de apilado si el paquete ya existe. Puede ignorarse el filtro de rentabilidad mediante `require_positive_savings=false`.
- **Operaciones:** `stack` y `cut_stack`. Los dos subpaquetes resultantes conservan el orden original de las capas. Las nuevas franjas se pueden seguir procesando en la siguiente llamada a la simulación.
- **Reproducción:** se validan todas las operaciones concatenadas a la secuencia original mediante el simulador físico de la etapa 11, que comprueba el balance de área por capa, trazabilidad, altura y límites geométricos.

`result.groups` describe la propuesta; `result.operations_added` es un sufijo de operaciones para agregar al plan de entrada; `result.base_simulation` y `result.projected_simulation` permiten comparar antes y después; `result.ungrouped_workpiece_ids` indica las franjas propuestas que siguen sin agruparse.

## Ejemplos

```bash
cargo run -p optimizer-cli -- run --input examples/plan-strip-stacks.json
cargo run -p optimizer-cli -- run --input examples/plan-strip-stacks-existing.json
cargo run -p optimizer-cli -- run --input examples/plan-strip-stacks-disabled.json
```

El primer ejemplo corta tres tableros individualmente, forma un paquete de tres franjas izquierda y otro de dos franjas derecha (de distintos tableros) y aplica un corte horizontal compartido en cada uno.

## Límites

El algoritmo comparte únicamente **el próximo corte**, y no resuelve los árboles guillotina completos ni la selección temporal óptima de apilado/desapilado. La planificación industrial, etapas de corte, pinzas y posibles colisiones se desarrollarán después. `production_certified=false` sigue siendo correcto. La salida no es código CNC.
