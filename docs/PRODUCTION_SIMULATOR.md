# Etapa 11 — Simulador físico de producción

## Objetivo

Ejecutar una **secuencia explícita de operaciones** sobre unidades físicas de stock. El simulador no decide cuáles son las mejores operaciones; recibe un plan e informa su resultado o rechaza la primera operación imposible según las reglas actualmente implementadas.

### Entrada JSON

`operation.type = "simulate_production"` con los campos:

- `model`: contrato `CuttingJobModel` de las etapas 2–10.
- `plan.initial_boards`: tableros o retazos físicos seleccionados, identificados por `board_id`, `stock_id` y `stock_unit_index`; `rotated_90` es opcional.
- `plan.operations`: lista ordenada de acciones. Los IDs de unidades físicas y paquetes nuevos no deben repetirse jamás.

Operaciones (`type` dentro de `plan.operations`):

| Tipo | Descripción |
|---|---|
| `stack` | Agrupar **al menos dos** unidades libres con dimensiones, material, veta, orientación, registro y estación idénticos |
| `unstack` | Liberar los miembros existentes de un paquete |
| `cut_workpiece` | Dividir una unidad libre en dos regiones con un solo paso de sierra |
| `cut_stack` | Dividir simultáneamente todas las capas; devuelve dos paquetes nuevos |
| `rotate_workpiece` | Rotar una unidad libre 90° en sentido horario |
| `rotate_stack` | Rotar todas las capas del paquete 90° simultáneamente |
| `move_workpiece` | Mover una unidad libre a una estación identificada |
| `move_stack` | Mover conjuntamente los miembros del paquete |
| `finish` | Declarar una región como pieza fabricada después de verificar material, medidas, veta, giro y cantidad pedida |
| `scrap` | Dar por terminada una región como sobrante, reutilizable o no |

## Reglas y semántica

- **Unidad:** décimas de mm; las áreas se serializan como cadenas decimales en `(0.1 mm)^2`.
- **Cortes:** la posición `at` es relativa al borde inicial de la región, hasta el **borde cercano** del canal consumido por la sierra. Cada capa pierde el espesor `machine.kerf` en el tramo del corte.
- **Stack:** un corte sobre N tableros consume un único paso de sierra simulado, pero produce **2N regiones físicas** y N canales de kerf. Los paquetes resultantes mantienen el orden inferior-superior de sus capas.
- **Conservación:** después de **cada** operación se verifica: `área inicial = área activa + área terminada + área sobrante + área perdida por sierra`. Los elementos consumidos no suman dos veces.
- **Trazabilidad:** `parent_id`, `origin_board_id`, `origin_stock_unit_index`, `source_rect`, `stack_id` y el histórico `events[].operation` permiten reconstruir la secuencia.
- **Stock:** un par `(stock_id, stock_unit_index)` solo puede usarse una vez.
- **Ningún estado parcial** se devuelve cuando la simulación falla.
- **Saneamiento:** los tableros comienzan con su dimensión física **completa**. Los márgenes del stock no se descuentan sin cortes físicos explícitos. La semántica de saneamiento y limitaciones de acceso se integrará en el planificador industrial.
- **Apilados de franjas:** el simulador puede validar operaciones explícitas sobre franjas si `machine.stacking.strips` y `machine.stacking.restacking` están habilitados. **No las descubre ni optimiza** (etapas 12–13).

## Delimitación industrial

**NO** se certifica un programa de seccionadora. Aún no hay planificador de guillotina por fases, limitaciones dinámicas de mordazas/pinzas y registro en mesa, gestión de lotes superpuestos ni programación automática de `STACK`/`UNSTACK`. Se prevén para las etapas 12–14. El resultado contiene explícitamente `production_certified: false`.

## Pruebas

```bash
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/simulate-production-stack-cut.json
cargo run -p optimizer-cli -- run --input examples/simulate-production-single.json
cargo run -p optimizer-cli -- run --input examples/simulate-production-invalid-cut.json
```

En el último ejemplo la CLI debe terminar con error `cut_geometry` (código de salida 3).

## Ejemplo significativo

1. `b0` y `b1`, tableros 1000 × 800 × 180 unidades.
2. Se forma `whole` (dos láminas y 360 unidades de altura).
3. `cut_stack`, posición 400, kerf 10: aparecen dos paquetes: franja izquierda de 400 × 800, región derecha 590 × 800, en **cada capa**.
4. Se desapila el grupo izquierdo y se realiza un corte horizontal individual sobre la franja de `b0`.
5. La región 400 × 300 se identifica como `A`; su sobrante se registra como reutilizable.
6. El grupo derecho se desplaza a `outfeed` y rota 90°.
7. El balance es `1,600,000` de área inicial y `20,000` de área de sierra (cadenas decimales), con dos pasadas de sierra.
