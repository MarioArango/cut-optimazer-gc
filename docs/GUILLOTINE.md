# Etapa 5 — Evaluador de árboles de cortes guillotina

## Alcance

`guillotine_evaluate` recibe un modelo estructuralmente válido y el `pattern_id` de un árbol **existente**. Recorre el árbol, asigna coordenadas a cada nodo, coloca las piezas y sobrantes y devuelve los canales consumidos por la sierra. No genera ni optimiza patrones.

## Modelo geométrico

- 1 unidad = 0.1 mm, sin coma flotante.
- Origen de coordenadas: esquina superior izquierda del tablero (X hacia la derecha, Y hacia abajo).
- Los márgenes (`trim`) determinan la región útil. La representación física del saneamiento es trabajo posterior.
- `PatternNode.Split.at` es la distancia **local** al inicio del canal de sierra (no es la coordenada global ni el centro del corte).
- Vertical: `first` es izquierda, `second` derecha. Horizontal: `first` arriba y `second` abajo.
- Cada división conserva exactamente `área_padre = área_first + área_saw_path + área_second`.
- Cada hoja debe representar una pieza **del mismo tamaño exacto** que su rectángulo resultante o un residual identificado. No se permiten sobrantes implícitos dentro de una hoja `piece`.

## Etapas de corte

| Modo | Secuencia máxima permitida por rama |
|---|---|
| `V-H` | vertical → horizontal |
| `H-V` | horizontal → vertical |
| `V-H-V` | vertical → horizontal → vertical |
| `H-V-H` | horizontal → vertical → horizontal |
| `V-H-V-H`, `H-V-H-V` | 4 niveles, alternando |
| `V-H-V-H-V`, `H-V-H-V-H` | 5 niveles, alternando |
| `V-H-V-H-V-H`, `H-V-H-V-H-V` | 6 niveles, alternando (máximo) |

El número de etapa aumenta solo cuando cambia la dirección. Son admisibles cortes repetidos de la misma dirección dentro de una etapa. Las ramas pueden terminar antes de completar todas las etapas del patrón. Se rechaza saltar etapas, cambiar la primera orientación o añadir una etapa adicional.

La regla de etapas es **por ruta desde la raíz hasta una hoja**. La etapa 5 todavía NO demuestra que el orden global de los cortes, volteos, pinzas o apilados sea viable en una seccionadora. Los cortes devueltos están ordenados en DFS geométrico; no son instrucciones listas para máquina.

## Construcción incremental del árbol desde Rust

`GuillotineTreeBuilder::new("root")` inicia con una región residual. `split_residual("root", Vertical, 400, "left", "right")` subdivide la región; `place_piece("left", "A", false)` asigna una hoja a la pieza A; `set_residual_reusable("right", true)` marca un sobrante. `finish()` produce un `PatternNode`, que se incorpora al modelo y se evalúa con `evaluate_pattern`.

El constructor comprueba IDs, evita duplicados y solo permite subdividir hojas residuales. **La geometría y la compatibilidad de etapas se comprueban después**, con el evaluador.

## Validaciones incluidas en esta etapa

- Existencia del patrón y stock y validez estructural del modelo.
- Reglas de `allowed_patterns` mediante el validador del modelo existente.
- Kerf positivo, coordenadas, dos subregiones no vacías y suma exacta de áreas.
- Restricción de longitud máxima de corte (`machine.max_cut_length`).
- Dimensiones exactas de cada hoja `piece`, rotación permitida y eje de veta congruente con el tablero.
- Dimensiones mínimas por pieza y un máximo de ocurrencias de cada pieza por instancia de patrón.
- Rotación de tablero, márgenes y eje de veta coherentes con el preprocesamiento.
- Límite defensivo de profundidad de 128 niveles para evitar consumos excesivos.

## Todavía NO validado

- Completitud de todas las cantidades del pedido, asignación de inventario o repetición de patrones.
- Capacidad real de mordazas/pinzas, longitud de alimentación, tamaños manipulables de franjas y operaciones de saneamiento.
- Secuencia de máquina, corte en paquetes, apilado/desapilado o reagrupaciones.
- Aprovechamiento óptimo o diseño automático de patrones.

Esas comprobaciones quedan en las etapas 6–15; por ello `summary.production_validated` permanece en `false` y las capacidades anuncian `optimization_available: false`.

## Ejemplo reproducible

```bash
cargo run -p optimizer-cli -- run --input examples/guillotine-vhv.json
```

Este ejemplo usa un tablero de 1000×800 unidades con márgenes de 10 unidades por lado. Región útil: `(x=10, y=10, width=980, height=780)`.

Cortes geométricos del patrón `V-H-V` (espesor de sierra 10):

1. Vertical a 400 dentro de la región raíz.
2. Horizontal a 300 en la región izquierda.
3. Horizontal a 300 en la región derecha.
4. Vertical a 250 en la subregión derecha-superior.

La salida debe sumar:

| Área (unidades cuadradas) | Valor |
|---|---:|
| Región útil | 764400 |
| Piezas A+B+C | 288000 |
| Sobrantes | 455900 |
| Canales de sierra | 20500 |

Los campos de área se serializan como **strings decimales exactos** para evitar pérdida de precisión en JavaScript.

Otros ejemplos: `guillotine-vh.json`, `guillotine-hv.json`, `guillotine-hvh.json`, `guillotine-rotated-board.json` y cinco archivos `guillotine-invalid-*.json`.

## Comprobar

```bash
python3 scripts/check_fixtures.py
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`check_fixtures.py` es una verificación Python *estática*, no ejecuta Rust. Las demás comprobaciones requieren `rustc` y Cargo.
