# Etapa 14: planificador industrial de operaciones

## Propósito

`industrial_scheduler.rs` transforma **operaciones físicas explícitas, pero desordenadas** en una secuencia reproducible. No genera cortes que no hayan sido indicados: utiliza las operaciones y regiones proporcionadas por etapas anteriores y encuentra un orden que el simulador pueda ejecutar.

La nueva operación JSON se denomina `schedule_production`.

### Entrada

- `model`: `CuttingJobModel` con perfil de seccionadora, tableros, piezas y reglas de máquina.
- `plan.initial_boards`: unidades concretas de inventario, sin duplicar índices.
- `plan.operations`: colección de operaciones aún no ordenadas: `stack`, `unstack`, `cut_workpiece`, `cut_stack`, `rotate_*`, `move_*`, `finish` y `scrap`.
- `dependencies`: pares `before`/`after`, referidos a índices **base cero** de las operaciones originales. Las dependencias físicas por identificadores se comprueban implícitamente en cada reproducción.
- `schedule_options`: límite de nodos, verificación de huella mínima de sujeción, exigencia de estado terminal y costos unitarios orientativos.

### Estrategia

1. Valida las dependencias explícitas y la cantidad de operaciones (máximo 256 en esta versión).
2. Inicializa el estado material y rastrea la dirección de corte de cada rama en el sistema de coordenadas de su tablero de origen.
3. Enumera operaciones disponibles cuyas dependencias se han satisfecho. Prefiere cortes de paquetes y acciones de corte, con orden determinista.
4. Comprueba para cada candidato el patrón guillotina (`V-H`, `H-V`, `V-H-V`, `H-V-H`) y el umbral conservador de ancho de sujeción.
5. **Reproduce desde el comienzo** la secuencia candidata usando `simulate_production`. Rechaza duplicación de material, cortes inválidos, alturas de paquete prohibidas, problemas de estación, referencias inexistentes, etc.
6. Realiza búsqueda en profundidad con retroceso y límites de tiempo y nodos para hallar una secuencia que incluya todas las operaciones originales exactamente una vez.
7. Reproduce el resultado definitivo y devuelve los índices originales, plan ordenado, trazabilidad por operación, indicadores y costo orientativo.

### Ejemplo

```bash
cargo run -p optimizer-cli -- run --input examples/schedule-production.json
```

El ejemplo incluye ocho operaciones desordenadas para un paquete de dos tableros. La secuencia generada debe apilar, cortar el paquete, separar una franja y ejecutar el corte siguiente sin consumir dos veces el material.

Otros casos:

```bash
cargo run -p optimizer-cli -- run --input examples/schedule-production-terminal.json
# Los siguientes deben fallar de manera controlada:
cargo run -p optimizer-cli -- run --input examples/schedule-production-invalid-stages.json
cargo run -p optimizer-cli -- run --input examples/schedule-production-cycle.json
```

### Salida

`industrial_scheduling` contiene `ordered_source_indices`, `scheduled_plan`, `steps`, `simulation`, `search_nodes`, `cut_passes`, `stack_operations`, `unstack_operations` y `estimated_operation_cost_minor`.

### Limitaciones importantes

- **No crea automáticamente planos, nuevos paquetes ni operaciones de corte**. Ordena las operaciones de entrada; construirlas es responsabilidad de generadores y del planificador de apilado anterior.
- La búsqueda acotada es heurística: no certifica que el orden elegido sea el de menor costo ni que un pedido sin solución encontrada sea matemáticamente imposible si la búsqueda agota su presupuesto.
- `require_min_clamp_footprint` comprueba únicamente un umbral geométrico conservador. **No modela pinzas, ángulos accesibles, alimentación, escuadrado, topes, limitaciones de la bancada ni restricciones propias de marcas/modelos**.
- No constituye un archivo de instrucciones listo para una seccionadora, ni autoriza ejecución física sin adaptar y validar para la máquina concreta.
- El tiempo real de corte/manipulación debe calibrarse; los importes son pesos configurables en unidades menores de moneda, no cotizaciones de fábrica.
- La planificación global de geometría y apilados para minimizar el costo final corresponde a la etapa 15.

## Verificación

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
python3 scripts/check_fixtures.py
```

La revisión estática de los ejemplos no sustituye la compilación ni las pruebas Rust.
