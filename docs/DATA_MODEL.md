# Modelo industrial — etapa 2

## Unidades

- `Unit = u32` (décimas de milímetro). Todos los campos dimensionales de piezas, stock, máquina y regiones usan esta unidad.
- No se aceptan valores negativos, cero para medidas obligatorias ni dimensiones superiores a 1.000.000 unidades (100 m) como límite de seguridad inicial ajustable.
- `unit_cost_minor = u64` representa la unidad monetaria menor (en Perú puede ser céntimos); moneda y tarifas definitivas se abordarán en la etapa 16.
- `quantity = u32` representa la cantidad requerida o disponible de un tipo; en la etapa 3 se definirán índices compactos y expansión controlada.

## Material y veta

`Material`: `id`, `name`, `thickness`, `directional_grain`.

`Part`: dimensiones locales, material de referencia, cantidad, `allow_rotate_90`, `grain_axis: x|y|null`, prioridad opcional (`0..5`). La veta local de la pieza se comparará con la veta real del tablero durante el preprocesamiento; no se asume que `allow_rotate_90` tenga prioridad sobre la veta.

`Stock`: `kind: full_board|remnant`, dimensiones brutas, espesor, cantidad, veta del tablero, márgenes de saneamiento y costo. Los márgenes deben dejar área útil positiva, pero aquí todavía no se comprueba el encaje de piezas en ella.

## Perfil de seccionadora

`MachineProfile`: kerf, longitud máxima de corte, mínimos de pieza/franja/sujeción, altura y cantidad máxima de miembros de un paquete, rotación, recortes y combinaciones `V-H`, `H-V`, `V-H-V`, `H-V-H` y sus extensiones de 4, 5 y 6 niveles (`V-H-V-H` … `H-V-H-V-H-V`).

`stacking.full_boards`, `stacking.strips`, `stacking.restacking`: capacidades declaradas de la máquina y del proceso de manipulación. `restacking` requiere `strips`. La compatibilidad definitiva de paquetes con mordazas, secuencia y manipulación se verificará posteriormente.

## Patrón, región física y paquete (objetos separados)

- `CuttingPattern`: plantilla geométrica asociada a un tipo de stock. `PatternNode` tiene variantes `split`, `piece` y `residual`. `split.at` es una posición **local respecto a la región actual**. La semántica geométrica no se comprueba hasta las etapas 4–6.
- `Workpiece`: pieza física de material, tipo `board`, `strip` o `region`; conserva `origin_stock_id`, `origin_board_key` (identifica un tablero físico), `parent_workpiece_id`, orientación, registro y referencia a un patrón/nodo pendiente.
- `CuttingStack`: paquete físico con miembros **ordenados de abajo hacia arriba**, orientación y borde de referencia común. Dos paquetes pueden usar un mismo patrón, y un paquete de franjas puede contener orígenes diferentes.

Se comprueba que los miembros de un paquete tengan el mismo material, grosor, dimensiones, eje de veta y referencias de alineamiento, y que su altura y cantidad no superen el perfil de máquina. Se rechaza la pertenencia simultánea del mismo `Workpiece` a dos paquetes del modelo.

## Validación estructural y limites

- Los IDs de cada catálogo son únicos en su propia colección, con 1..64 caracteres ASCII alfanuméricos, `.`, `_`, `-`. Las distintas colecciones pueden reutilizar el mismo texto de ID porque su tipo da el contexto.
- Referencias `material_id`, `stock_id`, `part_id`, `pattern_id`, `pending_node_id`, `parent_workpiece_id` y `member_ids` deben apuntar a elementos existentes según corresponda.
- Las cadenas de ascendencia de un workpiece no deben contener ciclos y los hijos conservan la identidad de origen.
- La cantidad de tableros físicos identificados por (`origin_stock_id`, `origin_board_key`) no puede exceder `Stock.quantity`.
- Los atributos no declarados en el JSON son rechazados (`deny_unknown_fields`).
- Se limita a 100.000 entradas por catálogo, 10.000 patrones, 10.000 nodos por árbol y 100 incidencias por validación como salvaguardas iniciales. Estos máximos no representan capacidades de optimización probadas.

## Errores

En Rust, `validate_model(&CuttingJobModel)` devuelve `Result<ModelSummary, Vec<ModelIssue>>`. Cada incidencia contiene `path`, `code` y `message`. En CLI se muestran hasta diez incidencias por stderr, con código de salida 3, sin fingir que el pedido se ejecutó.

## Límites deliberados

No se calcula el tamaño útil de cada tablero, orientaciones factibles de una pieza, árbol de corte correcto, etapas físicamente ejecutables, cortes comunes, apilados óptimos, costos productivos, tiempos ni archivos de máquina. Los módulos posteriores asumirán la entrada estructural validada, pero **volverán a verificar independientemente la factibilidad geométrica y productiva**.
