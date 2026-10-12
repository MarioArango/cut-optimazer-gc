# Etapa 3 — Preprocesamiento industrial

## Contrato público

`operation.type = "preprocess_model"`, con el mismo objeto `model` usado por `validate_model`. La respuesta tiene `output.type = "model_preprocessing"` y `output.prepared = PreprocessedJob`.

Ejemplos: `examples/preprocess-model.json` y `examples/expected-preprocessing.json`.

La API Rust es `optimizer_core::preprocess_model(&CuttingJobModel) -> Result<PreprocessedJob, PreprocessError>`.

## Reglas exactas

1. **Unidad de medida:** entero `u32` en décimas de mm. `24400 = 2440 mm`. No redondeamos ni reinterpretamos medidas recibidas. Valores decimales en JSON no son válidos para dimensiones. Las áreas se devuelven como cadenas enteras, expresadas en `(0.1 mm)^2`, para evitar errores de precisión en JavaScript.
2. **Márgenes de saneamiento:** rectángulo útil sin márgenes; `usable_x=left`, `usable_y=top`, `usable_width=width-left-right`, `usable_height=height-top-bottom`.
3. **Giro de tablero:** si `machine.allow_board_rotation`, también representamos una rotación física horaria 90°. Para ella `width'=height`, `height'=width`; `top'=left`, `right'=top`, `bottom'=right`, `left'=bottom`. El eje de veta también cambia X↔Y.
4. **Giro de pieza:** solo disponible si `part.allow_rotate_90`. Cambia ancho/alto y el eje de veta local X↔Y.
5. **Veta:** el eje resultante de la veta de pieza debe coincidir con el del stock (ambos `None` para materiales sin veta direccional). Una pieza permitida para girar 90° no puede violar la veta por ese permiso.
6. **Dimensiones mínimas:** comprobamos `machine.min_piece_width` y `machine.min_piece_height` para cada orientación posible. Las restricciones adicionales de cortes, pinzas y bandas no se resuelven todavía.
7. **Encaje:** ancho y alto finales de pieza deben ser ≤ ancho/alto del rectángulo útil de alguna orientación de stock del mismo material. Se admite encaje exacto en este análisis dimensional: el kerf pertenece a la etapa geométrica posterior.
8. **Cantidades:** se expanden individualmente con `instance_id = "{part_id}#{ordinal}"`, ordinal 1..quantity, sin perder los tipos originales. Los identificadores `instance_id` son de trabajo y no son IDs ERP.
9. **Grupos:** se generan ordenados por `material_id`; las piezas y el stock dentro de cada grupo mantienen orden de entrada. Se incluye un grupo aunque no tenga piezas o stock.
10. **Área agregada:** suma `width × height × quantity` de piezas y `usable_width × usable_height × quantity` de existencias. Si el material solicitado excede el disponible se emite advertencia `insufficient_stock_area`. No se descuenta kerf: suficiente área NO garantiza plano fabricable.
11. **Estado de la pieza:** `eligible` o `ineligible`. La no elegibilidad no cancela toda la solicitud: se devuelve `issue.code` (`no_stock_for_material`, `below_machine_minimum`, `grain_conflict`, `does_not_fit_any_stock`). Las piezas no elegibles siguen en `instances` con `eligible=false`.
12. **Stock:** `FitOption` enumera posibilidades, no inventario reservado ni asignación de cantidades.

## Límites de recursos

- `MAX_EXPANDED_PIECES = 100_000` (instancias de pieza).
- `MAX_FIT_CHECKS = 2_000_000` (combinaciones intentadas de stock/rotaciones).
- `MAX_FIT_OPTIONS = 500_000` (combinaciones admitidas en la salida).

Estos límites evitan explosiones de memoria y tiempo al construir JSON. Si se exceden, `run` responde con error explícito y código de salida 3. En etapas posteriores podremos optar por representar cantidades agregadas sin expansión completa. `validate` comprueba estructura, no ejecuta el preprocesamiento ni sus límites de búsqueda.

## Advertencias industriales

**No confundir:** `allow_board_rotation` modela girar físicamente el tablero, no girar libremente la veta de piezas. El planificador posterior decidirá si esta maniobra se puede realizar en el estado real de la máquina.

Este módulo no interpreta `machine.max_cut_length`, `min_strip_width`, `min_clamp_width`, paquetes ni altura de apilado para descartar una pieza. Estas restricciones dependen de cortes y secuencias reales que todavía no existen en la etapa 3.

El modelo validado y preparado no es una orden de corte. Debe pasar por geometría, guillotina, validación, asignación y planificación productiva en las etapas aprobadas siguientes.
