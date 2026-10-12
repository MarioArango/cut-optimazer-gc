# Etapa 8: generación de planos guillotina de tres etapas

## Alcance

`generate_three_stage` genera un **solo tablero** siguiendo un patrón `V-H-V` o `H-V-H` elegido entre los permitidos por el perfil de máquina. Explora múltiples alternativas mediante **Beam Search** y valida geométricamente la solución con el evaluador guillotina de la etapa 5 antes de devolverla.

Esta etapa **no** optimiza el pedido global en varios tableros, no reserva stock, no calcula apilados y no modela pinzas, secuencias físicas o tiempos de máquina. Una solución geométrica válida no implica compatibilidad industrial completa.

## Algoritmo

1. Valida los datos del pedido y los límites de búsqueda.
2. Para cada stock/orientación/patrón admisible, calcula las orientaciones de pieza compatibles con veta y medidas.
3. Genera un conjunto acotado de **anchos de franja**, incluyendo combinaciones de piezas con separación de sierra.
4. Para cada candidato, construye franjas (primera dirección), filas en cada franja (segunda dirección), y piezas de ancho variable dentro de cada fila (tercera dirección).
5. Usa cuatro heurísticas (`longest_first`, `largest_area_first`, `best_fit`, `priority_first`) y conserva como máximo `beam_width` estados parciales no equivalentes por profundidad.
6. Solo admite una solución cuando su `CuttingPattern` pasa `evaluate_pattern`; los resultados representan cortes reales con kerf y conservación de área.
7. Devuelve el mejor candidato geométrico encontrado, piezas pendientes e indicadores de búsqueda. Un límite de tiempo/expansiones puede impedir encontrar mejores soluciones.

`candidates_evaluated` cuenta candidatos constructivos, no validaciones completas. `expanded_states` cuenta intentos de extensión. `search_exhausted` es verdadero cuando se alcanza el límite de tiempo o expansiones; `false` no significa optimalidad.

## API JSON

Ejemplo completo: `examples/generate-three-stage-vhv.json`.

```json
{
  "schema_version": "1.0",
  "job_id": "generate-three-stage-demo",
  "options": {"seed": 2026, "max_runtime_ms": 30000},
  "operation": {
    "type": "generate_three_stage",
    "model": "[OBJETO CuttingJobModel, NO UNA CADENA]",
    "stock_id": "board1000",
    "beam_width": 12,
    "max_expansions": 1500
  }
}
```

En un archivo real, `model` es un objeto JSON, no la cadena ilustrativa.

Campos opcionales:

- `stock_id`: se omite para considerar todos los stocks compatibles.
- `beam_width`: 1 a 64; valor por defecto 12.
- `max_expansions`: 1 a 20.000; valor por defecto 1500.
- `options.seed`: semilla de desempates, predeterminada 42.
- `options.max_runtime_ms`: presupuesto temporal, de 100 a 3.600.000 ms.

La respuesta contiene `output.type="three_stage_generation"` y `output.result` con `found`, `complete_order`, `candidates_evaluated`, `expanded_states`, `search_exhausted`, `beam_width`, `remaining_part_counts` y `board`.

Dentro de `board` se incluyen `pattern` (árbol editable en el contrato de corte) y `evaluated` (coordenadas, cortes y áreas verificadas), además de los parámetros del tablero y el aprovechamiento expresado en puntos básicos.

## Casos de prueba relevantes

```bash
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/generate-three-stage-vhv.json
cargo run -p optimizer-cli -- run --input examples/generate-three-stage-hvh.json
cargo run -p optimizer-cli -- run --input examples/generate-three-stage-no-fit.json
```

Los ejemplos V-H-V y H-V-H están diseñados para utilizar realmente la tercera orientación en el árbol. También existen pruebas de cantidades pendientes, rotaciones, stock desconocido, límites, conservación de área y repetibilidad.

## Límites conscientes

- El máximo inicial es 256 tipos de piezas y 64 colocaciones por tablero. La profundidad del árbol está acotada.
- El generador de franjas y filas es heurístico: no encontrará todas las posibles geometrías válidas.
- El número de etapas puede ser menor que tres en ramas concretas, según el árbol.
- El porcentaje de utilización solo describe área de piezas sobre área útil, no costo ni velocidad productiva.
- La validación de cantidades para **todo el pedido** requiere la etapa 9; los paquetes y apilados corresponden a etapas 10–14.

## Verificación

Las pruebas Rust están incluidas pero deben ejecutarse con Cargo; la revisión estática de fixtures (`python3 scripts/check_fixtures.py`) no las sustituye.
