# Arquitectura independiente (etapa 17)

```text
Postman / Cliente HTTP / otro programa
              |
         HTTP REST JSON
              |
      optimizer-api (Rust)
      axum + tokio + validaciones de transporte
      autenticación opcional / concurrencia limitada
              |
      optimizer-core (Rust)
      geometría, guillotina, stock, multitablero, apilados
      LNS, producción, valoración de sobrantes
```

La integración con clientes se hace únicamente por HTTP/JSON; sin dependencia de ERP. `optimizer-cli` se conserva como herramienta de diagnóstico local y no es requisito de ejecución para el API. No hay base de datos.

El API es síncrono; el proceso REST no maneja dispositivos CNC. Los documentos históricos de etapas anteriores registran el desarrollo original, pero este archivo y `REST_API.md` rigen el transporte actual.
