# Etapa 10 — Apilados de tableros completos

## Alcance

El módulo `full_board_stacking.rs` reconoce **patrones completos idénticos**
entre tableros físicos que ya han superado la validación independiente de la
etapa 6. Propone paquetes sin cambiar la geometría del plano.

Una identidad de patrón exige: mismo `stock_id` (mismo material, espesor,
dimensiones, veta y márgenes), orientación del tablero, tipo de patrón
(V-H, H-V, V-H-V, H-V-H) y árbol completo de cortes (posiciones,
direcciones, piezas, rotación y manejo de residuales). Los identificadores
internos (`node_id` y `pattern_id`) no influyen: se codifica una
representación binaria canónica del árbol para agruparlo sin falsos positivos.
Los retazos `remnant` no se agrupan como tableros completos.

Las restricciones aplicadas son `machine.stacking.full_boards`,
`machine.max_stack_members`, `machine.max_stack_height` y el espesor
real de cada tablero. La máquina **todavía no** tiene planificación física
de pinzas, rotaciones, velocidades ni acceso a las franjas: el plan
es una **oportunidad de apilado**, no una orden de producción certificada.

## Algoritmo

1. Validar modelo y plano multitablero con el validador independiente.
2. Evaluar la geometría de cada patrón y contar líneas de corte.
3. Agrupar solo tableros compatibles y geométricamente idénticos.
4. Calcular miembros máximos = `min(max_stack_members,
   floor(max_stack_height / thickness))`.
5. Formar grupos de tamaño máximo, sin reutilizar tableros físicos.
6. Rechazar paquetes si `pasadas_ahorradas * cut_cost_units <=
   stack_setup_cost_units + stack_release_cost_units`.
7. Devolver paquetes y tableros sin apilar, con estimaciones en unidades
   abstractas de trabajo y advertencia explícita de validación física pendiente.

Las `estimated_saw_passes_*` son número de **líneas geométricas** por patrón:
no representan necesariamente pasadas de máquina, tiempos reales, cortes
simultáneos de franjas ni manipulación. La función de costo es conservadora
al cobrar una preparación y una liberación por paquete, pero sigue siendo
una aproximación sin calibración. Nunca confundirla con una estimación de
minutos garantizada.

## Contrato JSON

### `plan_full_board_stacks`

Usa un `model` con patrones y un `layout` existente:

```bash
cargo run -p optimizer-cli -- run --input examples/plan-full-board-stacks.json
```

La respuesta `output.type=full_board_stack_planning` contiene `stacks`,
`unstacked_board_ids`, `estimated_saw_passes_saved`,
`estimated_work_units_saved`, `geometry_validated=true` y
`physical_execution_validated=false`.

### `generate_stacked_multi_board`

Genera primero un plano multitablero mediante la **etapa 9**; después
planifica paquetes sin reoptimizar la geometría:

```bash
cargo run -p optimizer-cli -- run --input examples/generate-stacked-multi-board.json
```

Devuelve `output.result.allocation` y `output.result.full_board_stacking`.
Las dos operaciones son independientes. La búsqueda conjunta de mejor
patrón + mejores paquetes llegará en la etapa 15, con simulación física
intermedia en etapas 11–14.

## Casos cubiertos por pruebas

- Patrones equivalentes con IDs de árbol distintos.
- Límite de altura y máximo de tableros por paquete.
- Capacidad deshabilitada y costo de manipulación elevado.
- Separación conservadora de SKUs aunque tengan dimensiones iguales.
- Patrón modificado, stock duplicado y geometría inválida.
- Resultados parciales del optimizador multitablero.
- CLI, contratos JSON y API Rust pública.
