# Benchmarks reproducibles — Rust

Ejecutar sobre el **binario de release**; los resultados dependen del equipo, las piezas, la configuración y el presupuesto de búsqueda. Estos scripts **no generan números de rendimiento ficticios**.

## Medir núcleo sin red

```bash
cargo run --release -p optimizer-bench -- --fixture examples/generate-multi-board.json --iterations 20 --warmup 3 --output benchmark-multiboard.json
cargo run --release -p optimizer-bench -- --fixture examples/generate-three-stage-vhv.json --iterations 20 --warmup 3
cargo run --release -p optimizer-bench -- --fixture examples/optimize-global.json --iterations 10 --warmup 2
```

## Medir HTTP concurrente

En otra terminal iniciar `cargo run --release -p optimizer-api`. Luego:

```bash
cargo run --release -p optimizer-bench -- --http 127.0.0.1:8080 --fixture examples/generate-multi-board.json --iterations 20 --concurrency 2 --output benchmark-http.json
```

Si el servicio requiere `OPTIMIZER_API_KEY`, define la misma variable en la terminal del benchmark. La prueba solo admite HTTP plano local o de red interna; NO soporta URLs HTTPS (usar TLS a través de proxy para clientes públicos). No ejecutes una prueba de carga contra producción sin permiso.

Métricas: tiempos `min`, `p50`, `p95`, `máximo`, `promedio`, `total`, peticiones exitosas y fallidas y productividad aproximada. Los percentiles se calculan sobre todas las solicitudes del lote. El benchmark REST incluye red, transporte y serialización; el de núcleo mide ejecución del motor.

Los escenarios de prueba deben fijar `seed` y demás opciones para poder comparar versiones. Un benchmark satisfactorio NO sustituye un ensayo de exactitud geométrica o certificación de la seccionadora.
