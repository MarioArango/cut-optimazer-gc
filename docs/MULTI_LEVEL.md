# Cortes de hasta 6 niveles

El motor admite patrones guillotina de **2 a 6 niveles** (cada nivel cambia de
dirección: vertical, horizontal, vertical…). **3 niveles es el valor por
defecto** en la pantalla de proyecto.

| Niveles | Patrones |
|---:|---|
| 2 | `V-H`, `H-V` |
| 3 | `V-H-V`, `H-V-H` |
| 4 | `V-H-V-H`, `H-V-H-V` |
| 5 | `V-H-V-H-V`, `H-V-H-V-H` |
| 6 | `V-H-V-H-V-H`, `H-V-H-V-H-V` |

Elegir «N niveles máximos» envía en `machine.allowed_patterns` todos los
patrones de N niveles o menos, en ambas orientaciones. Una rama del árbol puede
terminar antes del último nivel.

## Qué gana un nivel más

Con 3 niveles, dentro de una fila todas las piezas deben tener **exactamente**
el alto de la fila. Un 4.º nivel permite recortar una pieza más baja que su
fila y aprovechar el sobrante con otras piezas; los niveles 5 y 6 repiten la
idea dentro de esos sobrantes.

## Cómo funciona

- **Validación (sin cambios de criterio):** todo patrón, de cualquier nivel,
  pasa por el evaluador independiente (`guillotine.rs`): secuencia de niveles,
  ancho de sierra, medida exacta de cada pieza, veta, mínimos de máquina y
  largo máximo de corte. Un patrón inválido se descarta; nunca se entrega.
- **Generador de 4 a 6 niveles (`deep_stage.rs`):** divide una región en
  celdas con cortes paralelos; cada celda es una pieza exacta, un retal o se
  rellena con el nivel siguiente. Las franjas del primer nivel se eligen con
  búsqueda en haz; dentro de ellas el relleno es voraz con cuatro criterios.
- **Corrección de valores (solo con 4+ niveles):** rellenar tablero por
  tablero agota pronto las piezas que combinan bien y deja tableros casi
  vacíos al final. Por eso el reparto hace varias pasadas completas: las piezas
  que acabaron en tableros mal aprovechados suben de valor y en la pasada
  siguiente se colocan antes y mejor acompañadas. Se conserva la mejor pasada.
- **Garantía frente a 3 niveles:** con 4+ niveles el pedido primero se resuelve
  exactamente como un trabajo de 3 niveles (mismos patrones de 2–3 niveles,
  semilla y tiempo) y después con todos los patrones en el tiempo restante
  (al menos la mitad del máximo). Se entrega el mejor de los dos: **más niveles
  nunca dan un plano peor que 3 niveles**. Por eso el trabajo puede tardar
  hasta 1,5 × el «Tiempo máximo» en el peor caso.
- **Con 2–3 niveles nada cambia:** ni el generador nuevo ni la corrección de
  valores se ejecutan; los 84 ejemplos y `entrada.json` dan exactamente el
  mismo resultado que antes de este cambio.

## Medición (pedido Rovere, 345 piezas, 12 ítems, `entrada.json`)

| Niveles máx. | Tableros | Aprovechamiento | Tiempo |
|---:|---:|---:|---:|
| 2 | 39 | 60,19 % | 0,8 s |
| 3 | 34 | 69,05 % | 1,4 s |
| 4 | **30** | **78,25 %** | 23 s |
| 5 | **30** | **78,25 %** | 27 s |
| 6 | **30** | **78,25 %** | 30 s |

Referencia: el optimizador anterior del cliente usó 32 tableros completos y 3
parciales (≈ 33,5 tableros de material). Mínimo teórico por área: 24.

Con 4+ niveles la búsqueda usa todo el «Tiempo máximo» (hasta 1,5 × en el peor
caso, por la garantía); en este pedido ya se llega a 30 tableros con 5 s.

## Validación realizada

- Pruebas Rust: árboles de 2 a 6 niveles aceptados solo bajo patrones de igual o
  más niveles y la orientación correcta; un 7.º nivel o una orientación errónea
  se rechazan; un caso donde **solo un 4.º nivel** coloca todas las piezas.
- Planos de 4, 5 y 6 niveles revisados con el verificador de planos (sin
  solapes, dentro del saneado, cada pieza sacada por su corte, `stock_cuts`,
  ningún corte más allá del nivel máximo) y pedidos aleatorios en 2, 3, 4 y 6
  niveles.
- Fuera de las pruebas: 60 pedidos aleatorios × niveles 2–6 (300 ejecuciones)
  verificados de forma independiente: 0 fallos, todos completos; con 4–6
  niveles 19 pedidos usan menos tableros que con 3 y **ninguno usa más**
  (media 13,47 frente a 14,07 tableros).

## Límites

- Heurístico: no garantiza el óptimo.
- Hasta 400 piezas y 48 cortes paralelos por región en un tablero.
- Con 4+ niveles pueden aparecer cortes reales que sacan un **retal entre
  piezas** (`type: "retal"` con `i_cut`); cuentan en `stock_cuts`.
