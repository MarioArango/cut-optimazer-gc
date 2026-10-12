# API REST independiente — etapa 17

La comunicación con el optimizador es **exclusivamente HTTP/JSON**. Todo el servicio está implementado en **Rust** usando `axum` + `tokio` y la librería `optimizer-core`. No necesita Node.js, JavaScript, ninguna base de datos ni conexión con ningún ERP. La CLI Rust anterior queda disponible para pruebas y mantenimiento.

## Ejecutar en macOS / Linux / Windows

Instalar Rust estable desde https://rustup.rs y, dentro de la carpeta del proyecto:

```bash
cargo test --workspace
cargo run --release -p optimizer-api
```

Por defecto escucha en `http://127.0.0.1:8080`. Deja este proceso activo mientras haces solicitudes desde Postman.

## Endpoints

| Método | Ruta | Función |
|---|---|---|
| `GET` | `/healthz` | Estado del servidor; siempre público |
| `GET` | `/api/v1/capabilities` | Operaciones soportadas, versión y esquema |
| `GET` | `/api/v1/openapi.json` | OpenAPI del transporte REST |
| `POST` | `/api/v1/jobs/validate` | Validar solicitud sin generar el resultado de optimización |
| `POST` | `/api/v1/jobs` | Ejecutar operación del motor y devolver resultado JSON |

Se mantiene el contrato ya implementado por `optimizer-core`: `schema_version`, `job_id`, `options` y `operation`, con `operation.type` que selecciona `generate_two_stage`, `generate_three_stage`, `generate_multi_board`, `optimize_global`, `plan_full_board_stacks`, `plan_strip_stacks`, `plan_dynamic_stacks`, `schedule_production`, `evaluate_economics`, etc.

**Todos los ejemplos de `examples/*.json` son cuerpos válidos para POST /api/v1/jobs si su contrato es válido.** La colección Postman `postman/Industrial-Cutting-Optimizer.postman_collection.json` contiene 18 escenarios reales de cálculo y otros endpoints.

### Ejemplo de Postman

- Importa `postman/Industrial-Cutting-Optimizer.postman_collection.json`.
- Selecciona `00 - Salud` para probar la conexión.
- Prueba `01 - Comprobar motor` y después `09 - Optimizar múltiples tableros` o `16 - Optimización global LNS`.
- Cambia `baseUrl` si usas otro puerto; para autenticación habilita el encabezado `Authorization: Bearer {{apiKey}}` en cada solicitud que necesite protección y configura la variable `apiKey`.

Solicitud mínima:

```http
POST http://127.0.0.1:8080/api/v1/jobs
Content-Type: application/json

{"schema_version":"1.0","job_id":"postman-001","operation":{"type":"system_check","echo":"hola"}}
```

Ejemplo curl:

```bash
curl -sS http://127.0.0.1:8080/healthz
curl -sS -H 'Content-Type: application/json' \
  --data-binary @examples/generate-multi-board.json \
  http://127.0.0.1:8080/api/v1/jobs
```

## Configuración por variables de entorno

| Variable | Valor por defecto | Uso |
|---|---|---|
| `OPTIMIZER_BIND` | `127.0.0.1:8080` | Dirección/IP y puerto |
| `OPTIMIZER_API_KEY` | no configurada | Bearer token requerido si se configura |
| `OPTIMIZER_MAX_BODY_BYTES` | `4194304` | Máximo de JSON por solicitud (1 KiB a 64 MiB) |
| `OPTIMIZER_MAX_PARALLEL_JOBS` | `2` | Límite de cálculos simultáneos (1–64) |
| `OPTIMIZER_MAX_JOB_RUNTIME_MS` | `120000` | Máximo del presupuesto solicitado por cliente (100 a 3600000 ms) |

Por seguridad, **no se permite iniciar en una IP no local sin `OPTIMIZER_API_KEY`**. Para clientes remotos utiliza HTTPS a través de un reverse proxy (Caddy, nginx, etc.), nunca expongas HTTP sin TLS directamente. No hay CORS abierto por defecto; Postman no requiere CORS. No uses la clave desde una SPA pública: las llamadas autenticadas de otros sistemas deberían hacerse desde sus respectivos servidores.

Ejemplo de entorno para un servicio protegido (Linux/macOS):

```bash
export OPTIMIZER_BIND=0.0.0.0:8080
export OPTIMIZER_API_KEY='cambia-esto-por-un-secreto-largo'
export OPTIMIZER_MAX_PARALLEL_JOBS=4
cargo run --release -p optimizer-api
```

## Respuestas HTTP

| Estado | Significado |
|---|---|
| `200` | Se ejecutó o validó la operación; **no garantiza** que todas las piezas hayan sido colocadas |
| `400` | El JSON no coincide con el contrato sintáctico |
| `401` | Falta la clave Bearer o es incorrecta |
| `413` | Cuerpo demasiado grande |
| `415` | Content-Type no es application/json |
| `422` | Datos semánticamente inválidos o error al ejecutar el motor |
| `503` | No queda un slot de procesamiento libre; volver a intentar |
| `500` | Fallo interno en el servidor / serialización / trabajador |

Formato de errores generado por los handlers:

```json
{"error":{"code":"validation_failed","message":"descripción del error"}}
```

Los errores de ruta/método administrados directamente por `axum` pueden responder con un formato diferente.

Los resultados con **pedido parcial** pueden responder `200`: deben comprobarse campos como `result.complete_order`, `result.validation.valid` o `report.valid`, según el tipo de operación. El motor de corte no fabrica un `200` como prueba de optimización óptima ni de factibilidad mecánica completa.

## Ejecución y límites

- La API es síncrona: devuelve el resultado en la misma petición HTTP; no hay base de datos, cola de trabajos ni tareas persistentes.
- El `Tokio` runtime recibe las conexiones; cada trabajo computacional se ejecuta mediante `spawn_blocking` y el acceso a estos trabajadores está controlado por un semáforo. Solicitudes adicionales reciben 503.
- `max_runtime_ms` es un **presupuesto solicitado al motor**, no una cancelación forzosa universal. Algunas validaciones o cálculos todavía no interrumpen inmediatamente su ejecución. Un cliente desconectado no cancela por la fuerza un cálculo en Rust.
- No se ejecuta ninguna acción sobre máquinas reales: planes, secuencias y métricas necesitan validación y homologación por modelo de seccionadora antes de convertirse en comandos CNC.
- No se generan archivos de exportación propietarios de máquinas en esta etapa. REST devuelve JSON estructurado que el cliente puede adaptar.
- La colección Postman es un cliente de prueba, **no una dependencia de ejecución**.

## Verificaciones

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p optimizer-api
```

Contiene pruebas Rust del transporte HTTP: disponibilidad, operaciones existentes, autenticación, formato, límites de solicitud y rechazo de contratos inválidos. No se pudieron ejecutar aquí por la ausencia de Cargo/Rustc. El paso siguiente (etapa 18) incluye optimización de rendimiento, pruebas de carga y certificación.
