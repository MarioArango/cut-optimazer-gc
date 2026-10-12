# Etapa 6 — Validador geométrico independiente

## Objetivo

Verificar un **conjunto concreto de tableros utilizados**, no solamente un patrón individual. El código `layout_validation.rs` analiza árboles `PatternNode` directamente, sin utilizar `guillotine::evaluate_pattern` ni `geometry::Rect::split`. Sirve como verificación independiente de los futuros generadores de planos.

## Entrada: `LayoutPlan`

`boards[]` tiene `board_id`, `stock_id`, `stock_unit_index` (cero a `quantity-1`), `pattern_id` y `board_rotated_90`. Se puede repetir el patrón en distintos tableros, pero nunca el tablero físico. Stock, pieza, máquina y árboles se entregan en `CuttingJobModel` de la etapa 2.

## Controles

1. Integridad estructural y referencias del modelo original.
2. Identificador único de tablero y uso único de `(stock_id, stock_unit_index)`.
3. Índices dentro de cantidad de stock y patrón perteneciente al material correspondiente.
4. Dimensiones útiles tras márgenes, incluyendo la rotación de 90°.
5. Cortes guillotina, espesor de sierra, longitud máxima y secuencia permitida por rama.
6. Material, orientación, veta, mínimo de pieza y dimensiones exactas en cada hoja.
7. Conservación de área y ausencia de solapes mediante barrido en X con intervalos Y.
8. Cantidades acumuladas en todos los tableros: detecta faltantes y excedentes.

**Áreas:** valores en unidades cuadradas `(0.1 mm)^2` codificados como cadenas para JavaScript. El área útil excluye saneamiento; los cortes de saneamiento no se incluyen aún en la secuencia física. Los contadores de piezas y áreas se acumulan **solo** de tableros que pasan toda la auditoría; si alguno falla, `count_reconciliation_complete=false`.

## Fuera de alcance

No prueba agarre mínimo real, posición de mordazas, etapas ejecutables a nivel de máquina, reorientaciones físicas, paquetes apilados, tiempo productivo, generación de patrones ni óptimos de material. Esto se desarrollará en etapas posteriores. El perfil `min_strip_width` será utilizado en el planificador de máquina, no se infiere automáticamente de la anchura de todos los residuos.

## Ejecución

```bash
cargo run -p optimizer-cli -- run --input examples/layout-valid.json
cargo run -p optimizer-cli -- run --input examples/layout-invalid-duplicate-stock.json
cargo run -p optimizer-cli -- run --input examples/layout-invalid-piece-size.json
cargo test --workspace
```

El JSON de un layout inválido da `report.valid=false` y detalle de errores, no un error de parseo. `cargo test` permite comprobar el contrato dorado de la respuesta válida y pruebas de regresión.
