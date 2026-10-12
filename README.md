# Industrial Cutting Optimizer — Etapa 17 / 18

**Optimizador industrial de corte de tableros totalmente independiente, escrito en Rust y accesible por API REST.** No depende de ningún ERP, Node.js, JavaScript, React ni de un servicio externo.

## Inicio rápido para Postman

Requiere Rust estable. Dentro de esta carpeta:

```bash
cargo build --workspace
cargo test --workspace
cargo run --release -p optimizer-api
```

El servidor REST inicia en **http://127.0.0.1:8080**. Importa en Postman:

`postman/Industrial-Cutting-Optimizer.postman_collection.json`

Después ejecuta `00 - Salud`, `01 - Comprobar motor` y `09 - Optimizar múltiples tableros`.

## Endpoints HTTP

```text
GET  /healthz                  # ping público
GET  /api/v1/capabilities     # esquema, versión y operaciones
GET  /api/v1/openapi.json     # OpenAPI HTTP
POST /api/v1/jobs/validate    # valida un contrato JSON
POST /api/v1/jobs             # ejecuta el motor Rust
POST /api/v1/cut-plans        # optimiza y devuelve planos de corte listos para usar
GET  /viewer                  # visor de planos (HTML autónomo)
```

## Planos de corte y visor

`generate_cut_plans` optimiza el pedido y devuelve los planos con cortes
embebidos por nivel, qué pieza saca cada corte, retales (sin medidas negativas)
y métricas por tablero y por material. Ver **[docs/CUT_PLANS.md](docs/CUT_PLANS.md)**.

```bash
curl -s -X POST http://127.0.0.1:8080/api/v1/cut-plans \
  -H 'Content-Type: application/json' --data @examples/generate-cut-plans-rovere.json > planos.json
```

Abre `http://127.0.0.1:8080/`: en la pestaña **Proyecto** escribes materiales y piezas, ajustas la
configuración (⚙) y pulsas **Optimizar**; la pestaña **Planos** muestra el resultado y permite
descargarlo o imprimirlo. En ⚙ eliges **Niveles máximos de corte** (2 a 6, por defecto 3;
ver [docs/MULTI_LEVEL.md](docs/MULTI_LEVEL.md)). `viewer/index.html` también se abre sin servidor para ver planos ya generados.

En `POST /api/v1/jobs` puedes enviar directamente las solicitudes de `examples/*.json` o las incluidas en la colección Postman. Por ejemplo:

```json
{
  "schema_version": "1.0",
  "job_id": "primer-test",
  "operation": {"type": "system_check", "echo": "Postman conectado"}
}
```

## Estructura

- **`crates/optimizer-core/`**: algoritmos geométricos, patrones guillotina, multitablero, apilados, simulación, planificación y costos.
- **`crates/optimizer-api/`**: **servidor HTTP Rust (Axum + Tokio)**, validación, autenticación y control de carga.
- **`crates/optimizer-cli/`**: ejecutable de consola Rust opcional para soporte y pruebas.
- **`postman/`**: colección para importar directamente.
- **`examples/`**: solicitudes reales de prueba en JSON.
- **`docs/REST_API.md`**: instrucciones, endpoints, parámetros y errores; **`docs/openapi-stage17.json`**: especificación HTTP.

La comunicación entre clientes y motor se realiza exclusivamente mediante **REST/JSON**. El núcleo matemático no conoce HTTP, permitiendo mantenerlo separado del transporte.

## Seguridad de desarrollo

El servidor escucha **solo localhost** por defecto. Si se expone en red, debe configurarse `OPTIMIZER_API_KEY` y usar HTTPS con un proxy inverso. Se limita el tamaño de los cuerpos, el número de trabajos simultáneos y el presupuesto de ejecución que pueden solicitar los clientes. No hay autenticación de usuarios, cuotas por cliente ni persistencia de trabajos en esta versión.

## Estado de validación

Compilado y probado con Rust 1.99 (`cargo build --workspace` y `cargo test --workspace` en verde). Se corrigieron los tests que no compilaban, los golden files desactualizados y un fallo del generador de 2 etapas: cuando una sola franja ocupaba todo el ancho, el patrón se declaraba con la dirección equivocada (V-H en vez de H-V) y el validador lo rechazaba.

Los resultados no sustituyen un ensayo en la seccionadora real.


## Etapa 18 — API REST independiente + benchmarks

- [Ejemplo completo de entrada para Postman](docs/POSTMAN_REQUEST_STAGE18.md) (`examples/postman-industrial-real.json`)
- [Benchmark reproducible 100 % Rust](bench/README.md) (`crates/optimizer-bench`)
- [Entrega y criterios de validación de la etapa 18](docs/RELEASE_STAGE_18.md)
- Colección: `postman/Industrial-Cutting-Optimizer.postman_collection.json`
- No requiere Node.js ni ningún ERP.
