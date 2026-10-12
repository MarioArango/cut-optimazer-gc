# Etapa 13 — Apilado/desapilado dinámico de franjas

## Alcance

`plan_dynamic_stacks` recibe un **prefijo de producción físicamente válido** (`ProductionPlan`) y, para cada franja/región activa relevante, un **árbol de cortes pendientes** (`WorkpieceRoute`). Planifica operaciones sobre las unidades físicas reales, incluyendo:

- conservar paquetes cuando sus capas comparten el siguiente corte;
- crear paquetes de franjas compatibles, priorizando futuras rutas semejantes;
- separar automáticamente un paquete cuando las rutas divergen;
- reagrupar franjas liberadas, incluso procedentes de tableros diferentes;
- deshacer temporalmente un paquete para incorporar franjas sueltas cuando el ahorro de pasadas compensa apilar/desapilar;
- entregar las operaciones resultantes junto a trazabilidad completa y resultados de simulación.

### Semántica de las rutas

Cada corte produce **dos regiones**, que pueden requerir otras operaciones distintas. Por eso los cortes se expresan como árbol binario, no como `next_cut` secuencial:

```json
{
  "workpiece_id": "b0_left",
  "route": {
    "kind": "cut", "direction": "horizontal", "at": 300,
    "first": {
      "kind": "cut", "direction": "vertical", "at": 150,
      "first": {"kind": "done"}, "second": {"kind": "done"}
    },
    "second": {"kind": "done"}
  }
}
```

`done` significa que **no hay más cortes planificados**. No certifica que la región sea una pieza comercial, ni realiza `finish` o `scrap`. La clasificación de las salidas se hará posteriormente.

## Política de decisión y costos

1. Si un paquete tiene rutas divergentes, se desapila.
2. Si admite un corte común, normalmente se conserva y corta junto.
3. Opcionalmente se evalúa desarmar el paquete para incorporar otras franjas compatibles: solo si el ahorro por pasadas evitadas supera el costo adicional de manejo.
4. Para franjas libres con cortes comunes, se forman grupos respetando capacidad de altura, cantidad, material, orientación, veta, apoyo y estación. Se prefiere agrupar rutas futuras semejantes.
5. Si no compensa apilar, se corta individualmente.
6. Se repite sobre las regiones generadas hasta terminar todas las rutas.
7. Opcionalmente se liberan los paquetes finales para operaciones posteriores.

Se utiliza una **heurística determinista de ahorro neto inmediato y compatibilidad futura**, no una búsqueda global exacta ni beam search. La selección de óptimos globales corresponde a la etapa 15.

Costo estimado de las operaciones **añadidas**, en unidades monetarias menores:

`(cortes_sin_apilar - pasadas_ejecutadas) * costo_pasada - apilados * costo_apilar - desapilados * costo_desapilar`

Los costos son configurables y **no están calibrados** con una máquina real. No incluyen tiempos de traslado, setup, alturas de elevadores ni productividad de pinzas.

## Seguridad y límites

- No altera el plan original. Genera `operations_added` para anexarlas al prefijo.
- **Cada operación** se valida reproduciendo el plan completo en el simulador industrial de la etapa 11.
- Conserva la genealogía física por capa y exige conservación exacta de área incluyendo kerf.
- Rechaza un ID desconocido, repetido, ya consumido o no correspondiente a una franja o región activa.
- Límite de rutas: 5000. Límite de nodos de ruta: 20000. Límite de operaciones del simulador: 20000 (configuración menor en `max_operations`).
- No genera un programa máquina, no prueba físicamente mordazas/sujeción, ni garantiza rendimiento óptimo.
- La interfaz no activa automáticamente este módulo al generar patrones en etapa 9; se invoca explícitamente después del prefijo.

## Ejecución

```bash
cargo test --workspace
cargo run -p optimizer-cli -- run --input examples/plan-dynamic-stacks.json
cargo run -p optimizer-cli -- run --input examples/plan-dynamic-divergent.json
cargo run -p optimizer-cli -- run --input examples/plan-dynamic-repackage.json
cargo run -p optimizer-cli -- run --input examples/plan-dynamic-disabled.json
```

Las pruebas incluyen un paquete de tres franjas con dos cortes comunes, divergencia posterior a un corte, reagrupamiento desde tres tableros, máquina sin soporte, casos de ahorro negativo, IDs inválidos y determinismo.

## Restricciones pendientes de la etapa 14

La admisibilidad de cortos guillotina secuenciados, acceso de pinzas, posiciones de referencia de mordazas, límites de manipulación automática y emisión de programas para seccionadoras específicas debe validarse con el perfil real de la máquina. **Una simulación físicamente conservativa no equivale a certificación de producción.**
