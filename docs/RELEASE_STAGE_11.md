# Entrega etapa 11 / 18 — simulador de producción

Se añade `production_simulator.rs`, operación CLI/JSON `simulate_production`, modelos de estado físico, genealogía de materiales, historial de operaciones y cierre de área en cada transformación.

**Pruebas entregadas:** pruebas unitarias de integración Rust para apilar, cortar, desapilar, rotar, mover, finalizar pieza y registrar sobrantes; casos negativos para stock duplicado, IDs duplicados, paquete sobredimensionado, material consumido, cortes geométricos imposibles y cambios incompatibles; dos tests de CLI. Se mantienen los tests de etapas anteriores actualizados a `stage: 11`.

**Límites:** no se calculan apilados de franjas óptimos, no se decide cuándo agrupar/desagrupar, no hay planificación física de cabezales y mordazas; sigue `production_certified: false`. La geometría original de los planos sigue existiendo en las etapas 1–10 y no se reescribe.

**Compilación:** no verificada en este contenedor porque no hay Rust/Cargo y el acceso a los servidores de descarga está bloqueado. Se realizaron validaciones de datos y scripts estáticas, **no** tests ejecutados en Rust.
