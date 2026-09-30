# 🦀 FerrisTelemetry

> **APM de Alto Rendimiento, Telemetría en Terminal (TUI) y Framework Arena para Rust.**  
> *Inspirado en el diseño y fluidez de Bottom, Bandwhich, Zenith y Oxker.*

[![Rust](https://img.shields.io/badge/Rust-1.78%2B-orange.svg)](https://www.rust-lang.org/)
[![Ratatui](https://img.shields.io/badge/Ratatui-0.29-blue.svg)](https://ratatui.rs/)
[![Axum](https://img.shields.io/badge/Axum-0.8-purple.svg)](https://github.com/tokio-rs/axum)
[![Tokio](https://img.shields.io/badge/Tokio-1.43-black.svg)](https://tokio.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

---

## 🌟 ¿Qué es FerrisTelemetry?

**FerrisTelemetry** es una herramienta de observabilidad, monitorización de rendimiento en tiempo real (APM) y benchmarking competitivo construida completamente en Rust.

Combina:
1. **Un servidor web de alto rendimiento en Axum 0.8** corriendo en el puerto `:3000`.
2. **SQLite embebido** para el historial interno y **PostgreSQL 18** para benchmarks de aplicación real.
3. **Motor de métricas in-memory de cero asignaciones** con contadores atómicos de hardware (`AtomicU64`) y *RingBuffers* de ventana deslizante a 60 FPS.
4. **Una interfaz gráfica de terminal interactiva con Ratatui 0.29 y Crossterm 0.28** que dibuja curvas continuas con caracteres Braille.
5. **Generador de estrés asíncrono** con `tokio` y `reqwest` para simular tráfico masivo en loopback (`[s]`, `[+]`, `[-]`).
6. **⚔️ Framework Arena**: comparación reproducible de **Rust (Axum y Actix Web)**, **Go (Fiber)** y **Python (FastAPI)** con validación de respuestas, rondas múltiples y escenarios HTTP/JSON, lecturas y escrituras PostgreSQL.

---

## 🚀 Inicio Rápido

### Preparar y ejecutar

```bash
cd /home/alek/VNC/repos/ferris_telemetry

# Una sola vez: dependencias Python y binarios release comparables
./scripts/setup-python.sh
./scripts/build-benchmarks.sh

# PostgreSQL para el escenario de stack completo
docker compose up -d --wait postgres
./scripts/seed-postgres.sh

# TUI
./target/release/ferris_telemetry
```

Al iniciar, FerrisTelemetry:
* Arrancará Axum, Actix Web, Fiber y FastAPI como procesos independientes.
* Usará SQLite `ferris_telemetry.db` únicamente para el historial de la herramienta.
* Abrirá el dashboard TUI a 60 FPS en pantalla completa.

---

## 🎮 Atajos de Teclado (Keybindings)

| Tecla | Acción |
| :---: | :--- |
| **`[Tab]` / `[Shift+Tab]`** | Cambiar a la siguiente / anterior pestaña |
| **`[1]` - `[4]`** | Ir directamente a una pestaña (*1: Overview, 2: Endpoints, 3: Arena, 4: Sistema*) |
| **`[s]`** | **Iniciar / Pausar el Generador de Carga** interno |
| **`[+]` / `[=]`** | **Aumentar concurrencia** de tráfico (+5 workers) |
| **`[-]` / `[_]`** | **Disminuir concurrencia** de tráfico (-5 workers) |
| **`[b]`** | **Lanzar Shootout Benchmark** en la Framework Arena contra los servidores activos |
| **`[m]`** | Alternar escenario: HTTP/JSON, lista PostgreSQL, una fila, multi-lectura o escritura |
| **`[v]`** | Ejecutar matriz de concurrencia PostgreSQL/HTTP: 8, 16, 32, 64, 128 y 256 |
| **`[r]`** | **Resetear métricas** y limpiar los gráficos de la sesión |
| **`[↑]` / `[↓]`** | Seleccionar rutas en la pestaña Endpoints |
| **`[q]` / `[Esc]` / `[Ctrl+C]`** | Salir limpiamente restaurando la terminal |

---

## 📑 Pestañas del Dashboard

### 1. 📊 Overview (Vista General)
- **Tarjetas KPI**: Total de peticiones, throughput actual (Req/s), tasa de fallos (%) y latencia p95.
- **Gráfica Braille Continua**: Curva histórica de Req/s de los últimos 60 segundos dibujada con resolución sub-carácter.
- **Distribución de Códigos HTTP**: Medidores en tiempo real de respuestas `2xx OK`, `4xx Client Error` y `5xx Server Error`.
- **Percentiles de Latencia**: Desglose instantáneo de `Min`, `p50`, `p90`, `p95`, `p99` y `Max`.

### 2. 🌐 Endpoints (Inspector de Rutas)
- Tabla completa de rutas HTTP (`/api/v1/users`, `/api/v1/plain`, `/api/v1/compute`, etc.).
- Métricas independientes por ruta: método HTTP, contador de llamadas, errores, latencias promedio y p99.
- Selector interactivo con ejemplos de comandos `curl` listos para copiar.

### 3. ⚔️ Framework Arena (Batalla de Rendimiento)
- Detección automática de estado (`🟢 Online` / `⚪ Offline`) para:
  - 🦀 **Rust (Axum)** en el puerto `:3000`
  - ⚡ **Rust (Actix Web)** en el puerto `:4000`
  - 🦫 **Go (Fiber)** en el puerto `:8080`
  - 🐍 **Python (FastAPI)** en el puerto `:8000`
- Comparativa visual en barras horizontales:
  - 🚀 **Throughput (Req/s)**
  - ⏱️ **Latencia p99 en microsegundos**
  - 💾 **Memoria RAM Residente (RSS en MB)**
  - 🏆 **Índice de Eficiencia (Req/s por MB de RAM)**
- **HTTP/JSON** (`/api/v1/plain`): aísla servidor HTTP, runtime y serialización.
- **PostgreSQL lista** (`/api/v1/users`): diez filas con la misma consulta, respuesta JSON y pool precalentado de 20 conexiones.
- **PostgreSQL una fila** (`/api/v1/user?id=...`): IDs variables entre 1 y 100.000 en cada petición.
- **PostgreSQL multi-lectura** (`/api/v1/queries?ids=...`): diez IDs variables en una consulta `ANY`.
- **PostgreSQL escritura** (`/api/v1/write?id=...`): upsert controlado en `benchmark_writes`.
- Axum y Actix usan `tokio-postgres` nativo con mapeo manual; Fiber usa `pgxpool` y FastAPI `asyncpg`. Todos mantienen un pool fijo y precalentado de 20 conexiones.
- Cada ejecución usa 4 rondas de 3 segundos, concurrencia 20 y orden rotativo completo.
- `[v]` repite el escenario activo con concurrencias 8, 16, 32, 64, 128 y 256; cada nivel queda registrado como una ejecución independiente.
- El throughput cuenta exclusivamente respuestas exitosas cuyo contenido haya sido validado.
- Se muestran mediana de Req/s, variación entre rondas, p50/p95/p99, errores y RAM bajo carga. Al terminar se registran CPU del contenedor PostgreSQL (si Docker la expone), conexiones activas/totales y cache hit de `pg_stat_database`.

La Arena busca comparar la aplicación completa de forma reproducible, no sustituir a
TechEmpower: cliente, servidores y PostgreSQL se ejecutan en el mismo equipo, con
concurrencia configurable desde el runner (20 por defecto), calentamiento previo y
orden rotativo. Las cifras solo son comparables entre ejecuciones realizadas con la
misma versión release, esquema, datos y configuración.

### 4. 💻 System Health (Salud del Sistema)
- Monitoreo en tiempo real de **cada núcleo de CPU** de tu máquina.
- Medidores de memoria RAM y Swap global.
- Tabla detallada de consumo de memoria y CPU de los procesos monitoreados.

---

## 🧪 Servicios de benchmark

La tecla `[a]` administra los cuatro targets desde la TUI. Los binarios deben ser builds `release` actuales; la aplicación rechaza binarios ausentes o desactualizados para no mezclar perfiles de compilación.

```bash
./scripts/setup-python.sh
./scripts/build-benchmarks.sh
docker compose up -d --wait postgres
./scripts/seed-postgres.sh
```

Puertos: Axum `3000`, Actix Web `4000`, FastAPI `8000`, Fiber `8080` y PostgreSQL `5432`. Para detener PostgreSQL: `docker compose down`.

Para una medición manual aislada puedes cambiar los puertos de los binarios con
`AXUM_PORT`, `ACTIX_PORT` y `FIBER_PORT`; la TUI conserva los puertos anteriores por defecto.

La URL predeterminada es `postgres://ferris:ferris@127.0.0.1:5432/ferris_bench`; puede reemplazarse con `DATABASE_URL`.

`seed-postgres.sh` crea un dataset determinista de 100.000 usuarios y la tabla
`benchmark_writes`. El volumen de Docker solo ejecuta los scripts de
`docker-entrypoint-initdb.d` al crearse por primera vez, por eso el script de
seed es seguro y necesario también cuando ya existía el volumen.

---

## 🛠️ Endpoints Disponibles en Axum (:3000)

Puedes probar peticiones externas desde otra terminal o navegador:

```bash
# Healthcheck
curl -s http://127.0.0.1:3000/health

# Lectura real desde PostgreSQL
curl -s http://127.0.0.1:3000/api/v1/users | jq

# Una fila con ID variable
curl -s 'http://127.0.0.1:3000/api/v1/user?id=50000' | jq

# Diez filas en una sola consulta
curl -s 'http://127.0.0.1:3000/api/v1/queries?ids=1,2,3,4,5,6,7,8,9,10' | jq

# Escritura/upsert de benchmark
curl -s -X POST 'http://127.0.0.1:3000/api/v1/write?id=50000' | jq

# Payload comparable sin base de datos
curl -s http://127.0.0.1:3000/api/v1/plain | jq

# Cálculo intensivo en CPU
curl -s http://127.0.0.1:3000/api/v1/compute | jq
```

---

## 🏗️ Arquitectura de Código

```text
src/
├── main.rs                  # Entrada principal y teardown seguro de terminal
├── app.rs                   # Loop de eventos y navegación
├── db/                      # SQLite embebido con WAL mode y pool asíncrono
├── metrics/                 # Contadores atómicos y RingBuffers de 60 segundos
├── server/                  # Servidor Axum 0.8 y middleware de telemetría
├── arena/                   # Gestor de Framework Arena y runner de benchmarks
├── simulator/               # Generador de estrés asíncrono (reqwest + Tokio)
├── system/                  # Lector de métricas del Kernel Linux con sysinfo 0.33
└── tui/                     # Widgets y layout con Ratatui 0.29
```

---

Desarrollado con 🦀 por el equipo de ingeniería Rust.



//ME

  docker compose up -d --wait postgres
  ./scripts/seed-postgres.sh
  ./scripts/setup-python.sh
  ./scripts/build-benchmarks.sh
  ./target/release/ferris_telemetry
