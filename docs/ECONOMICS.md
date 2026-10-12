# Etapa 16 — Evaluación económica y recuperación de sobrantes

## Alcance

Operaciones JSON nuevas:

- **`evaluate_economics`**: recibe un `model`, un `layout` y `economic_options` opcionales; exige plano geométricamente validado, stock físico único y pedido completo, salvo `allow_partial_order: true`.
- **`compare_economic_layouts`**: compara de 1 a 32 alternativas sobre **el mismo** modelo e inventario. Primero prioriza completar todas las piezas y luego aplica `objective`.

Todas las cantidades monetarias son **unidades menores de la moneda** configurada por tu aplicación (p. ej., céntimos PEN). No se envía el símbolo de moneda ni se aplica un tipo de cambio. La salida codifica todos los totales como **cadenas decimales** para evitar pérdidas de precisión al consumirlos desde JavaScript. Las dimensiones están en décimas de milímetro y las áreas en sus unidades cuadradas.

## Fórmula

```
material = suma(precio unitario de cada lámina física utilizada)
operaciones = pasadas_sierra * precio_pasada
            + paquetes * (precio_apilar + precio_desapilar)
            + láminas * precio_manipular_tablero
            + giros_extra * precio_giro
            + movimientos_extra * precio_movimiento
            + coste_preparación_pedido
crédito_sobrantes = sum( valor recuperable de restos elegibles, acotado por lámina )
costo_neto = material + operaciones - crédito_sobrantes
```

El desperdicio **NO** se vuelve a cobrar como una línea adicional: ya pagamos por la lámina. El sobrante no se trata como una venta confirmada ni como dinero recibido; es un **valor recuperable estimado** y debe ser calibrado con el inventario y el mercado.

### Restos elegibles

Solo se valoran hojas `residual` del árbol con `reusable: true`, dimensiones mínimas y área mínima configuradas. La valoración se calcula sobre el área bruta de su lámina original:

```
credito_nodo = floor(precio_lámina * area_resto * recovery_rate_bps
                     / (area_bruta_lámina * 10_000))
credito_total_lámina <= floor(precio_lámina * max_board_credit_bps / 10_000)
```

Se redondea **hacia abajo**, se limita el crédito **por lámina**, y nunca se genera un descuento mayor que el valor del material. La recuperación no incorpora costos de almacenamiento, manipulación o deterioro; el factor de valoración y el límite deben reflejar estas pérdidas.

### Alternativas de fabricación

- **`separate_boards`**: estima una pasada de sierra por corte geométrico; aún no es una secuencia certificada.
- **`full_board_stacks`**: reutiliza etapa 10 para paquetes de **tableros completos con patrones idénticos**. Se recomiendan paquetes solo si el ahorro en pasadas supera los costos configurados de apilar y desapilar.

**No se incluyen** apilados de franjas ni reagrupación dinámica en el cálculo económico automático. El motor de la etapa 14 puede validar algunas secuencias físicas explícitas, pero esta operación no lo invoca ni garantiza equivalencia de la secuencia con el layout.

### Objetivos de comparación

| `objective` | Primer criterio | Desempate |
|---|---|---|
| `minimum_total_cost` | Costo neto estimado | Material neto y producción |
| `minimum_material_cost` | Material menos valor recuperable | Costo neto y producción |
| `minimum_production_cost` | Costo de operaciones estimado | Costo neto y material |

Una solución **parcial** nunca supera a otra que complete más piezas, independientemente de su costo. Las alternativas geométricamente inválidas se rechazan.

## Ejecución

```bash
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/evaluate-economics.json
cargo run -p optimizer-cli -- run --input examples/evaluate-economics-no-stacking.json
cargo run -p optimizer-cli -- run --input examples/evaluate-economics-partial.json
cargo run -p optimizer-cli -- run --input examples/compare-economic-layouts.json
```

`examples/evaluate-economics-invalid.json` debe salir con código de error.

## Límites y calibración

Los valores por defecto de costo son **ilustrativos**, no tarifas reales. La máquina física y los tiempos de ciclo no se han calibrado. Antes de utilizar los resultados para cotizaciones o producción: definir costos por seccionadora, moneda, longitud/velocidad de cortes, sujeción, carga y descarga, personal, almacenamiento, política de recuperación y costo de retazos. `physically_certified` permanece en `false`. La etapa 17 estará dedicada a integración y visualización.
