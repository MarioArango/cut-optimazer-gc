# Etapa 9 — Optimización geométrica multitablero

## Alcance

`generate_multi_board` asigna piezas pendientes a múltiples tableros físicos
identificados por `(stock_id, stock_unit_index)`, empleando los generadores de
2 etapas (V-H, H-V) y 3 etapas (V-H-V, H-V-H), y el generador de 4 a 6
niveles (`deep_stage.rs`) cuando el perfil de máquina los permite. Con patrones
de 4+ niveles las pasadas son **con corrección de valores** (ver
`docs/MULTI_LEVEL.md`); con 2–3 niveles el comportamiento no cambia.

**No implementa todavía:** paquetes repetidos, apilado de tableros,
apilado de franjas, LNS global ni secuenciación de seccionadora. Corresponden
a las etapas 10–15.

## Entrada JSON

```json
{
  "schema_version": "1.0",
  "job_id": "mi-pedido-001",
  "options": { "seed": 2026, "max_runtime_ms": 30000 },
  "operation": {
    "type": "generate_multi_board",
    "model": { "materials": [], "parts": [], "stocks": [], "machine": {} },
    "max_boards": 128,
    "restarts": 3,
    "beam_width": 12,
    "max_expansions": 1500
  }
}
```

`model` del ejemplo es solamente una plantilla esquemática; ver
`examples/generate-multi-board.json` para un modelo completo y válido.

- `max_boards`: 1 a 512. Límite de tableros utilizados por intento.
- `restarts`: 1 a 8. Intenta distintas políticas deterministas de selección.
- `beam_width`: 1 a 64. Ancho de búsqueda del generador de 3 etapas.
- `max_expansions`: 1 a 20000. Máximo de expansiones de la etapa 8.
- `max_runtime_ms`: límite global; si se agota, la búsqueda puede retornar
  una solución parcial.

## Salida

`multi_board_generation.result` contiene:

- `complete_order`: verdadero SOLO si todo el pedido está asignado y el
  validador independiente confirma el conjunto.
- `boards`: instancias con árboles, cortes y coordenadas evaluadas; cada una
  indica `stock_id`, `stock_unit_index`, `mode`, costo y origen 2/3 etapas.
- `layout`: referencias `BoardAssignment` compatibles con el validador etapa 6.
- `validation`: informe geométrico independiente de toda la solución.
- `remaining_part_counts`: piezas pendientes, incluso en resultados parciales.
- `stock_usage`: consumo físico por SKU.
- `stock_cost_minor` y `placed_piece_area_sq_units`: strings para preservar
  la exactitud de enteros grandes en JavaScript.
- `search_exhausted`: se alcanzó un límite de tiempo o tableros. **No** es
  un certificado de inviabilidad del problema.

## Algoritmo

1. Valida los datos completos sin mutarlos.
2. Construye temporalmente un modelo con cantidades aún pendientes.
3. Por cada SKU disponible, obtiene patrones candidatos de 2 y/o 3 etapas.
4. Compara el área colocada, las unidades y el costo del stock.
5. Reserva una unidad física específica, registra el patrón y descuenta
   exactamente las cantidades colocadas.
6. Repite hasta completar el pedido, alcanzar límites o quedarse sin opciones.
7. Ejecuta varios intentos con prioridades diferentes (área por costo, área,
   piezas, densidad). Conserva la mejor solución según piezas faltantes,
   costo y número de tableros.
8. Revalida todo el layout utilizando el validador independiente etapa 6.

**Limitaciones conocidas:** heurísticas secuenciales pueden quedarse en
óptimos locales; la mejora por intercambios multitablero y reconstrucción
parcial se implementará en la etapa 15. El presupuesto de tiempo es
cooperativo: una llamada interna puede consumir el tiempo asignado. Las
restricciones operativas de máquina y apilados todavía no están simuladas.

## Comandos

```bash
cargo build --workspace
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/generate-multi-board.json
cargo run -p optimizer-cli -- run --input examples/generate-multi-board-partial.json
cargo run -p optimizer-cli -- run --input examples/generate-multi-board-mixed.json
cargo run -p optimizer-cli -- run --input examples/generate-multi-board-three-stage.json
```
