# Hoja de ruta aprobada — 18 etapas

| N.º | Etapa | Alcance |
|---:|---|---|
| 1 | Arquitectura del proyecto | Workspace Rust, CLI, contrato JSON, pruebas, ejemplos y documentación |
| 2 | Modelo de datos | Piezas, materiales, stock, máquina, patrones, unidades físicas, paquetes |
| 3 | Preprocesamiento | Veta, rotaciones, márgenes, factibilidad y normalización |
| 4 | Motor geométrico | Rectángulos, divisiones, kerf, coordenadas |
| 5 | Árbol guillotina | 2-3 etapas y árboles de corte |
| 6 | Validador geométrico | Geometría, cantidades y restricciones independientes |
| 7 | Generador de 2 etapas | Strip packing y heurísticas |
| 8 | Generador de 3 etapas | Guillotine + beam search |
| 9 | Optimización multitablero | Asignación de stock y distribución global inicial |
| 10 | Apilado de tableros | Patrones repetidos, paquetes y altura |
| 11 | Simulador de producción | Transformaciones físicas, cortes, apilados, trazabilidad |
| 12 | Apilado de franjas | Paquetes intermedios y cortes compartidos |
| 13 | Apilado/desapilado dinámico | Decisiones de formar/separar/reagrupar |
| 14 | Planificador industrial | Secuencias ejecutables y restricciones de manipulación |
| 15 | Global Optimizer | LNS geométrico y productivo |
| 16 | Costos y sobrantes | Función objetivo y valorización |
| 17 | Integración industrial | Node.js, eCarpintero, edición y exportadores |
| 18 | Rendimiento y pruebas finales | Benchmarks, paralelismo, regresiones |

**Nota histórica:** etapa 9 entregada en su momento. Consultar el estado más reciente al final de este documento.

**Regla:** una etapa se implementa, se prueba y se presenta para aprobación antes de iniciar la siguiente. Ninguna etapa futura se declara completada por tener únicamente su diseño descrito.

## Estado de avance de esta entrega

Registro histórico de etapa 9; para el estado vigente consultar el final.


## Estado actual: etapa 10 entregada para revisión

Se ha añadido el planificador conservador de apilado de **tableros completos**
(`full_board_stacking.rs`). Las franjas, simulación de paquetes, apilado y
desapilado dinámicos siguen pendientes de las etapas 11–14.
La búsqueda conjunta geometría/apilados pertenece a la etapa 15.

## Estado de la entrega — Etapa 11

Se añadió el simulador de transformaciones físicas (`simulate_production`) y sus pruebas. Los estados narrativos anteriores se conservan como histórico y no sustituyen el avance actual: **11 de 18 etapas implementadas en código fuente**, pendiente verificación con Cargo y ensayos en máquina. Etapa 12 no comenzada.

## Estado de etapa 12 (octubre 2026)

Etapa 12 implementada a nivel de código: planificar apilados de franjas por el próximo corte común y reproducirlos en el simulador. Compilación/test Rust pendiente. Etapa 13 aún no iniciada: decisiones dinámicas de apilado/desapilado.

## Estado más reciente — Etapa 13

Implementada en fuente la planificación dinámica de paquetes de franjas con rutas de cortes binarias, separación por divergencia, reagrupación entre tableros y reproducción de todas las operaciones en el simulador. Las etapas 1–13 permanecen en el proyecto acumulativo. **Rust/Cargo no disponible aquí; compilación y ejecución de tests pendientes.** La etapa 14 (planificación industrial con restricciones específicas de máquina) requiere autorización del usuario.

### Etapa 14 implementada (pendiente de compilación)

Planificación de secuencias de operaciones físicas con dependencias explícitas/implícitas, control de etapas guillotina y reproducción de simulación. Próxima: etapa 15 — optimizador global LNS de patrones y apilados, **únicamente tras autorización**.

## Estado actual — Etapa 15

Se añadió un LNS acotado para reconstruir grupos de tableros y favorecer patrones repetidos,
comparando el material con un proxy de apilado de tablero completo. Se validan por separado
los árboles y las cantidades, manteniendo la mejor solución. **Compilación y pruebas Rust pendientes**
por ausencia de Cargo y de conectividad a `static.rust-lang.org`. Las decisiones dinámicas de franjas
no se han integrado aún en el costo global. Etapa 16 no iniciada.

## Estado actual — Etapa 16 entregada (pendiente de compilación)

`economics` aporta evaluación independiente de costo de material, cortes, manipulaciones, paquetes completos y crédito acotado de sobrantes, y permite comparar diferentes layouts. Esto **no** reemplaza la calibración real por máquina ni optimiza todavía paquetes dinámicos de franjas en la búsqueda global. Siguiente etapa: 17, integración API/editor/exportadores, únicamente con autorización del usuario.

## Cambio de alcance aprobado — Etapa 17

El producto es independiente de cualquier ERP. Su interfaz para clientes es exclusivamente REST/JSON en Rust (`optimizer-api`), con Postman como cliente de prueba. Se descartan SDK Node.js/TypeScript, integración con eCarpintero y dependencias runtime de JavaScript. Ver `REST_API.md`.
