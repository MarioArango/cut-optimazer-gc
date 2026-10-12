# Etapa 15 — Global Optimizer (LNS)

## Alcance real

LNS acotado sobre **varios tableros**. La solución inicial proviene del generador
multitablero de la etapa 9. El algoritmo:

1. Ordena tableros por aprovechamiento y selecciona un vecindario (o reinicio completo cada cuatro iteraciones).
2. Conserva los tableros no seleccionados, **reservando** su stock y descontando sus piezas del pedido.
3. Reconstruye el pedido parcial con los generadores guillotina existentes (`V-H`, `H-V`, `V-H-V`, `H-V-H`).
4. En ciertos vecindarios intenta duplicar un patrón compatible ya conservado; esto puede aumentar la probabilidad de apilados de tableros completos.
5. Reindexa las unidades físicas de stock por SKU y valida independientemente todos los árboles, piezas y cantidades usando la etapa 6.
6. Estima paquetes de tableros completos mediante el motor de la etapa 10, y solo acepta mejoras de la función objetivo.

El resultado siempre conserva por separado `initial` y `best`, la traza de mejoras y el estado de búsqueda.

## Función objetivo (provisional)

La comparación es **lexicográfica**:

1. Minimizar piezas pendientes (la demanda es prioritaria).
2. Minimizar `coste_de_stock_en_centimos + costes_abstractos_de_trabajo * operation_weight_minor`.
3. En empate, minimizar número de tableros y maximizar ahorro estimado de pasadas.

`StackCostModel` define costes **abstractos**, no precios calibrados en segundos o moneda. `operation_weight_minor` es una tasa de conversión configurable **provisional**. No interpretar `objective_minor` como cotización de fabricación validada. El modelo calibrado de costes pertenece a la etapa 16.

## Limitaciones explícitas

- LNS heurístico: no garantiza el óptimo ni prueba inviabilidad de un pedido incompleto.
- La oportunidad productiva que entra en la función objetivo es el **apilado conservador de patrones completos idénticos**. Aunque ya existen planificadores de franjas y apilado/desapilado dinámico (etapas 12–14), todavía no se invocan sobre cada nuevo patrón del LNS: faltaría sintetizar rutas físicas verificables y comparar esos planes completos. Esta etapa **no** afirma optimización conjunta completa de franjas.
- `best` representa planos geométricamente verificados, **no** programas certificados para una seccionadora. Sujeción, mesas, pinzas y movimientos específicos requieren un perfil industrial y pruebas físicas.
- Se respetan límites de tiempo y de iteraciones *best effort*: el tiempo de cada generador puede superar ligeramente el límite por la granularidad de las llamadas.
- La ejecución y compilación real de Rust sigue pendiente en el entorno de elaboración cuando Cargo no está disponible. La validación estática no sustituye `cargo test`.

## Ejecución

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/optimize-global.json
cargo run -p optimizer-cli -- run --input examples/optimize-global-partial.json
```

La operación `optimize_global` devuelve `output.type = "global_optimization"` y conserva la versión de esquema JSON `1.0`. Se añade `global_options` con límites de búsqueda, coste de operaciones y apilado.

## Verificaciones mínimas

- Ninguna pieza sobreproducida y ningún `(stock_id, stock_unit_index)` duplicado.
- Revalidación geométrica y de cantidades del plan global.
- Resultado `best` no empeora la prioridad lexicográfica frente a `initial`.
- Para pedidos parciales, no declarar el pedido completo.
- Apilado deshabilitado implica cero paquetes completos.
- Solicitudes inválidas se rechazan en el límite JSON.
