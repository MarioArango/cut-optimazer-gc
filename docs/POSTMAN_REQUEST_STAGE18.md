# Ejemplo de entrada real — Postman

Endpoint: `POST http://127.0.0.1:8080/api/v1/jobs`

Cabecera: `Content-Type: application/json`. Si `OPTIMIZER_API_KEY` está definida, también `Authorization: Bearer <clave>`.

Copia directamente el contenido de `examples/postman-industrial-real.json` en **Body → raw → JSON**.

Todas las medidas del modelo están en **décimas de milímetro**: 24400 = 2440 mm, 18300 = 1830 mm, 180 = 18 mm, 32 = 3.2 mm. `unit_cost_minor` está en centavos de la divisa elegida (18500 = 185.00 unidades monetarias).

El ejemplo incluye tres tipos de piezas (laterales, repisas y fondos), ocho tableros disponibles, una seccionadora con cuatro patrones admitidos y restricciones de apilado. 

`operation.type = "generate_multi_board"` calcula planos multitablero. Este endpoint **no planifica apilados** en la misma llamada. Si necesitas agrupar patrones repetidos usa `generate_stacked_multi_board`; para mejora global usa `optimize_global` con su correspondiente contrato.

La respuesta HTTP 200 indica que se ejecutó el trabajo, **no que se fabricaron todas las piezas**. Comprueba `output.result.complete_order`, `output.result.remaining_part_counts`, `output.result.boards` y `output.result.validation`.

El endpoint `POST /api/v1/jobs/validate` verifica estructura y parámetros del JSON, sin optimizar.

## Comandos de ejecución

```bash
cargo run --release -p optimizer-api
# Terminal adicional:
curl -sS -H 'Content-Type: application/json' --data-binary @examples/postman-industrial-real.json http://127.0.0.1:8080/api/v1/jobs
```

## Opción con apilados de tableros completos

En `examples/postman-industrial-stacked.json` está el mismo pedido, cambiando a `"type": "generate_stacked_multi_board"`. Este contrato primero genera planos y luego identifica oportunidades de apilado de patrones repetidos. **No equivale a optimización global de franjas**.
