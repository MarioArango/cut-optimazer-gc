# Geometría exacta — Etapa 4

## Unidad y coordenadas

- `Unit = u32`, unidad = **0.1 mm**: `24400` = 2440.0 mm; `32` = 3.2 mm.
- El origen `(x,y)` representa la esquina superior izquierda. `+x` va a la derecha, `+y` hacia abajo.
- `Rect { x, y, width, height }`: todas las longitudes son positivas; al construir o recibir un JSON, se valida que `x + width` y `y + height` no desborden `u32`.
- Se usan rectángulos semia-biertos `[x,x+width) × [y,y+height)`; los bordes adyacentes no se solapan. Las áreas se calculan con `u128`.

## División y espesor de sierra

`split(direction, at, kerf)` recibe `at` como **offset local al comienzo del canal de sierra**, nunca coordenada global ni centro del disco. El intervalo retirado tiene ancho `kerf`.

- Vertical: región primera **izquierda**, canal de sierra, región segunda **derecha**.
- Horizontal: región primera **superior**, canal de sierra, región segunda **inferior**.
- Se exige `kerf > 0`, `at > 0`, `at + kerf < ancho/alto del rectángulo`.
- La salida contiene `first`, `saw_path` y `second`, en coordenadas absolutas.
- Se cumple: `área_original = área_first + área_saw_path + área_second`.
- Puede repetirse manualmente un corte sobre `first` o `second` para construir un conjunto de regiones; el árbol y la validación por etapas llegan en etapas 5 y 6.
- El canal de sierra es material consumido: no se considera una pieza ni un retazo reutilizable.

## Saneamiento

`inset(TrimMargins)` devuelve el rectángulo útil aplicando márgenes `top`, `right`, `bottom`, `left` y conserva coordenadas globales. **No equivale a ejecutar cortes físicos de saneamiento**: esos cortes deben modelarse en la fase de planificación industrial.

## Más operaciones

- `intersection`, `intersects` (contacto de borde no cuenta como solapamiento), `contains`.
- `translated(dx,dy)`: desplazamiento con signo y control de desbordamiento.
- `rotate_cw_within(parent)`: rotación geométrica 90° de una región junto con su marco padre; no supone que la máquina pueda girarla físicamente.

## API y ejemplos

- `geometry_split`: recibe `rect`, `direction`, `at`, `kerf`; devuelve `output.split`.
- `geometry_trim`: recibe `rect`, `margins`; devuelve `output.usable`.
- Se conserva `schema_version: "1.0"` y las tres operaciones anteriores.
- `cargo run -p optimizer-cli -- run --input examples/geometry-vertical-split.json`
- `cargo run -p optimizer-cli -- run --input examples/geometry-horizontal-split.json`
- `cargo run -p optimizer-cli -- run --input examples/geometry-trim.json`
- `cargo run -p optimizer-cli -- validate --input examples/geometry-invalid-cut.json` debe devolver código 3.

## Límites deliberados

Esta etapa **NO** implementa asignación de piezas, árbol guillotina, comprobación de etapas de máquina, optimización del layout, simulación de apilados ni secuencias físicas. `optimization_available=false` sigue siendo la respuesta correcta.

El contrato `Unit=u32` es el de la etapa 2. Si en el futuro se requieren coordenadas mayores a ese rango, habrá que introducir un nuevo contrato versionado antes de cambiar el significado de JSON.
