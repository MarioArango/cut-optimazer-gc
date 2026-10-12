# Arquitectura acumulativa — etapa 6

```text
           CLI / Node.js / JSON
                    |
                contract.rs    (schema_version = 1.0)
                    |
                 engine.rs
                    |
       +------------+------------+
       |                         |
   Entrada / modelo          Plano aportado
       |                         |
   model/validation.rs       layout_validation.rs  (ETAPA 6)
       |                         |  - stock físico + patrones
   preprocess.rs                 |  - replay independiente del árbol
       |                         |  - conservación / no solapes
   geometry.rs                   |  - pedido completo y sobrantes
       |                         |
   guillotine.rs ----------------+  (sin llamadas desde el auditor)
      - evaluar patrón              El modelo PatternNode es compartido
      - construir árbol

   Futuras etapas: packing -> global optimizer -> stack optimizer -> scheduler
```

`layout_validation.rs` comparte únicamente los **tipos de modelo**, no la implementación geométrica del generador ni `Rect::split`. Por diseño, cada tablero es verificado desde su patrón de origen. Si un tablero presenta cualquier error, no se cuentan sus piezas como verificadas, y la conciliación queda incompleta.

Todos los módulos continúan siendo independientes de React. CLI invoca el motor; Rust no necesita Node.js ni un servidor para ejecutar las validaciones.


## Etapa 8: generador de tres etapas

`three_stage.rs` genera un solo árbol V-H-V/H-V-H, usando Beam Search acotado sobre franjas iniciales y filas de subcortes. Depende de las mismas estructuras de `model/` y el evaluador independiente de `guillotine.rs`. Sus resultados se exponen como `generate_three_stage` / `three_stage_generation`, sin modificar las operaciones 1–7. No reserva stock, no programa paquetes ni sustituye `layout_validation.rs` para validar pedidos multitablero.

`multi_board.rs` (etapa 9) coordina los dos generadores por cada
SKU elegible, reserva tableros físicos y pasa la solución completa al
validador independiente. Aún no modela paquetes apilados ni secuencias
de la máquina. Ver `MULTI_BOARD.md`.


## Entrega etapa 13 (estado actualizado)

El motor añade `dynamic_stacking.rs` con rutas binarias por franja y reproducción completa de operaciones mediante `production_simulator.rs`. Puede mantener paquetes, cortar franjas simultáneamente, separar rutas divergentes y reagrupar material procedente de distintos tableros. El resultado es una **propuesta de producción simulada, no un programa certificado de máquina**. La compilación y ejecución de tests Rust están pendientes por falta de toolchain en este entorno. La etapa 14 no ha comenzado.

## Etapa 14 — planificación de operaciones

`industrial_scheduler` toma un conjunto de acciones de producción ya definidas y las ordena mediante restricciones de precedencia, etapa guillotina por genealogía y replay con `production_simulator`. El planificador produce una secuencia y no un programa de máquina. Las etapas 12–13 pueden proporcionar acciones de apilado/desapilado para este componente; la optimización conjunta está prevista para la etapa 15.
