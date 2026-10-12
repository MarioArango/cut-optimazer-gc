# Planos de corte (`generate_cut_plans`)

Resultado listo para usar: cortes embebidos por nivel, cada corte indica qué
pieza o retal saca, y cada pieza dice dónde está. Todas las medidas en **mm**.

## Cómo pedirlo

```bash
cargo run --release -p optimizer-api
# Planos directamente (sin envoltura JobResponse):
curl -s -X POST http://127.0.0.1:8080/api/v1/cut-plans \
  -H 'Content-Type: application/json' \
  --data @examples/generate-cut-plans-rovere.json
```

También funciona por `POST /api/v1/jobs` (`output.type = "cut_plans"`,
documento en `output.result`) y por CLI:
`optimizer-cli run --input examples/generate-cut-plans-rovere.json`.

`global_options` es opcional: por defecto se usa una búsqueda amplia
(`default_cut_plan_search`). En el pedido de ejemplo (345 piezas, 12 ítems)
obtiene 34 tableros en ~1,5 s (release).

## Crear un proyecto (pestaña «Proyecto»)

Abre `http://127.0.0.1:8080/` (redirige a `/viewer`). Sin escribir JSON:

- **Materiales**: nombre, largo × ancho del tablero, espesor, tableros disponibles,
  precio por tablero y si tiene veta (a lo largo del largo del tablero).
  El precio importa: el optimizador compara el costo de los tableros con el de los
  cortes, así que con precio 0 puede preferir menos cortes a menos tableros.
- **Piezas**: material (si hay más de uno), cantidad, ancho, alto y veta:
  `L` a lo largo del ancho de la pieza (paralela al largo del tablero), `A` a lo
  largo del alto, vacío = sin veta (puede rotar). Se pueden pegar filas desde
  Excel (Cantidad, Ancho, Alto, Veta); Enter baja a la fila siguiente. Las piezas
  que no entran en el tablero se marcan antes de optimizar.
- **⚙ Configuración**: sierra, saneado, pieza/franja mínima, largo máximo de corte,
  patrones (2 y/o 3 niveles), apilado y tableros por libro, girar tablero,
  búsqueda (rápida / normal / exhaustiva), tiempo máximo y semilla.
- **Optimizar** envía el proyecto a `/api/v1/cut-plans` y abre la pestaña «Planos».
- **Descargar entrada** guarda el JSON de entrada (como `entrada.json`);
  **Importar entrada…** carga uno en el formulario.
- El borrador se guarda automáticamente en el navegador.

## Visor

`http://127.0.0.1:8080/viewer` — o abre `viewer/index.html` directamente en el
navegador (no necesita servidor ni internet). Acepta este formato, la
respuesta completa de `/api/v1/jobs`, el formato anterior (camelCase) y
archivos con texto antes del JSON.

- Resumen del material (tableros, libros, aprovechamiento, retales, cortes, metros de corte).
- Dibujo a escala por tablero: piezas por ítem, retales rayados, saneado, cortes coloreados por nivel.
- Al pasar el cursor por una pieza se resaltan **los cortes que la sacan** y su fila en la secuencia.
- Secuencia de cortes en árbol (nivel, posición, medida, qué saca).
- **Imprimir planos**: una hoja A4 horizontal por tablero con dibujo y secuencia.
- **Optimizar pedido…**: pega o carga el JSON de entrada (solicitud `generate_cut_plans`
  completa, como `entrada.json`, o solo el `model`), se optimiza en el motor y se dibuja.
  Requiere abrir el visor desde el servidor (`/viewer`): el API no envía cabeceras CORS, así que
  una página abierta como archivo no puede llamarlo.
- **Descargar JSON**: guarda el resultado mostrado (`planos-<job_id>.json`, formato actual).

## Estructura

Todas las claves están en **snake_case** y todas las medidas, ids y contadores
son **números JSON**. Solo son texto los campos que de verdad lo son
(`material`, `orientation`, `type`, `veta`…).

```text
{ success, message, complete_order, remaining_parts{part_id: n},
  records[ {                      // uno por material
    index, version, elapsed_time (s), material, material_id, saw_width, trim,
    max_books, thickness, unit_measure: "mm",
    summary{ boards, books, pieces, cuts, cut_length_m, board_area_m2, pieces_area_m2,
             remnant_area_m2, kerf_area_m2, trim_area_m2, utilization_pct, usable_utilization_pct },
    boards[ {
      index, orientation ("v"|"h": dirección del 1.er nivel), stock_no,
      root_stock_no   // libro (apilado): stock_no del primer tablero del grupo
      cuts_trims[ {stock_no, i_cut, x1, y1, x2, y2, a_level} ]   // saneado
      trim, trim_margins{top,right,bottom,left}, saw_width, max_books,
      stock_count: 1, stock_cuts       // n.º de cortes reales
      cuts[]          // nivel 1, con level2…level6 anidados
      parts[]         // solo piezas
      waste_part[]    // solo retales
      width, height, x1, y1, x2, y2,   // tablero y líneas de saneado
      metrics{ pieces, remnants, cuts, levels, cut_length_m, board_area_m2, usable_area_m2,
               pieces_area_m2, remnant_area_m2, kerf_area_m2, trim_area_m2, utilization_pct,
               usable_utilization_pct, largest_remnant, stock_id, board_id, rotated_90, pattern }
    } ]
  } ] }
```

### Cortes (`cuts`, `level2` … `level6`)

| Campo | Significado |
|---|---|
| `stock_no` | Tablero. |
| `x1,y1 → x2,y2` | Línea central de la sierra. Se extiende medio kerf más allá de la región, hasta el centro de los cortes que la limitan. |
| `a_level` | Nivel 1–6. |
| `i_cut` | Orden del corte real en el tablero (recorrido en profundidad). |
| `dimension` | Medida de la región que saca (ancho si el corte es vertical, alto si es horizontal). |
| `id_piece` | Pieza que saca (único en todo el pedido); `0` si la región se sigue cortando (lleva `levelN+1`); `10000+` si es un retal (ver `waste_part`). |
| `n_item` | Número de ítem (1 = primer `part` del modelo); para retales, igual a `id_piece`. |
| `type: "retal"` | La región es retal. |
| `edge: true` | La región llega exacta al borde: no hay pasada de sierra (no cuenta en `stock_cuts`). |

Cada corte saca la región **entre el corte hermano anterior (o el inicio de la
región padre) y él mismo**. Cada lista termina con su entrada de cierre en el
borde: un retal (`type: "retal"`, sin `i_cut`) o, si la última franja llega
exacta al borde, esa franja con `edge: true`.

**Retales negativos:** se calculan desde la geometría real del árbol guillotina,
por lo que nunca tienen medida cero o negativa. Cuando la última franja llega al
borde (o deja menos de un kerf), no se emite retal.

### Piezas (`parts`)

Exactamente estos campos:

| Campo | Significado |
|---|---|
| `stock_no` | Tablero. |
| `part` | Posición de la pieza dentro del tablero (0, 1, …). |
| `x, y` | Esquina superior izquierda. |
| `width`, `height` | Medidas nominales (`part.width`, `part.height` del modelo). |
| `rotated` | Si es `true`, en el plano `width` y `height` se intercambian. |
| `id_piece` | Id único en el pedido; coincide con el `id_piece` del corte que la saca. |
| `n_item` | Número de ítem. |
| `veta` | `"L"` veta a lo largo de `width`, `"A"` a lo largo de `height`, `null` sin veta. |

### Retales (`waste_part`)

`stock_no, part, x, y, width, height, id_piece, reusable`. El `id_piece`
(10000+) coincide con la entrada `type: "retal"` de `cuts`.

## Veta

En un material con veta (`directional_grain: true`), las piezas con
`grain_axis` respetan la veta del tablero y las piezas con
`grain_axis: null` **no tienen veta** y pueden rotar libremente (si
`allow_rotate_90: true`). Así está `examples/generate-cut-plans-rovere.json`.

## Formato anterior (camelCase)

El visor también abre los planos del formato anterior (`nIdPza`, `cutsTrims`,
números como texto, retales dentro de `parts`): los convierte al formato
actual al cargarlos.

## Limitaciones conocidas

- Hasta 6 niveles de corte (`allowed_patterns` hasta `V-H-V-H-V-H` / `H-V-H-V-H-V`); ver `docs/MULTI_LEVEL.md`.
- Con 4 o más niveles la optimización usa todo el tiempo máximo configurado (hasta 1,5 × en el peor caso).
- `elapsedTime` es el tiempo real de cálculo, por lo que dos ejecuciones no dan un JSON idéntico.
