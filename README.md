# Server Dashboard

Aplicacion de escritorio local para registrar proyectos, configurar servicios y ejecutar de forma explicita un servicio individual en Windows.

## Stack

- Tauri 2
- React 19
- TypeScript
- Vite
- Rust para persistencia y runtime nativo
- npm
- Windows como unica plataforma de ejecucion en esta fase

## Requisitos

- Windows 10 o posterior. El runtime de procesos usa `PROC_THREAD_ATTRIBUTE_JOB_LIST` y no aplica un fallback para versiones anteriores.
- Node.js compatible con Vite 7 y npm.
- Rust estable con el target MSVC de Windows.
- Microsoft Visual Studio con **Desktop development with C++**.
- Microsoft Edge WebView2 Runtime.

Consulta los [requisitos oficiales de Tauri](https://tauri.app/start/prerequisites/) para preparar Windows.

## Desarrollo

```powershell
npm install
npm run tauri dev
```

Comprobaciones y builds disponibles:

```powershell
npm run typecheck
npm run build
npm test
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri\Cargo.toml
cargo check --manifest-path src-tauri\Cargo.toml
npm run tauri build
```

## Ejecucion controlada en Windows

Start recibe exclusivamente `projectId` y `serviceId`. Rust vuelve a cargar la configuracion, comprueba la pertenencia, canonicaliza de nuevo la raiz y el working directory, confirma que la carpeta existe y permanece dentro de la raiz real, y vuelve a validar el comando. React nunca envia un comando para ejecutar.

El comando guardado se trata explicitamente como texto de shell elegido por el usuario. Rust obtiene `%SystemRoot%\System32\cmd.exe` mediante `GetSystemDirectoryW`, lo fija como `lpApplicationName` de `CreateProcessW` y construye esta linea de invocacion:

```text
cmd.exe /D /S /C "<comando configurado>"
```

`/D` desactiva AutoRun, `/S` aplica las reglas documentadas de comillas de `cmd.exe` y `/C` ejecuta el texto y termina el interprete. El texto configurado se coloca sin reescribirlo entre las comillas exteriores requeridas por `/S /C`; no se divide en programa y argumentos ni se concatena otro comando. No se usa PowerShell, salvo que el propio comando configurado invoque PowerShell expresamente. No se eleva el proceso, no se cambia `PATH` ni `ExecutionPolicy`, y el backend no registra el comando completo en diagnosticos.

Antes de ejecutar se muestra siempre una confirmacion con proyecto, servicio, working directory canonico y comando. Guardar o editar una configuracion nunca la ejecuta.

## Arbol de procesos y Stop

Cada Start sigue este orden:

1. Crea un Windows Job Object sin permitir breakaway y activa `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
2. Crea dos pipes separados para stdout y stderr, mas stdin cerrado, y prepara los tres handles heredables.
3. Dimensiona una sola `STARTUPINFOEXW` para dos atributos: `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, limitado a stdin/stdout/stderr, y `PROC_THREAD_ATTRIBUTE_JOB_LIST`, con el Job creado en el paso 1.
4. Crea `cmd.exe` con `CREATE_SUSPENDED`, `CREATE_NO_WINDOW` y `EXTENDED_STARTUPINFO_PRESENT`; Windows asocia el proceso al Job dentro de `CreateProcessW`, sin una asignacion posterior vulnerable.
5. Instala lectores y monitor, y solo entonces reanuda el thread principal.

Los arrays de handles heredables y Jobs, sus handles propietarios y la attribute list permanecen vivos hasta que `CreateProcessW` retorna. Si no se puede configurar `PROC_THREAD_ATTRIBUTE_JOB_LIST`, Start aborta antes de crear el proceso y devuelve un error que indica el requisito de Windows 10; no vuelve al flujo antiguo.

Los descendientes heredan el Job. La aplicacion conserva el handle del Job, no intenta localizar procesos por nombre y no mata ningun PID recibido desde React. Por ello, la reutilizacion de PID no puede dirigir Stop contra un proceso ajeno.

Stop es intencionadamente forzado en esta fase: cambia a `stopping`, llama `TerminateJobObject`, espera el proceso principal con timeout, cierra el handle del Job y publica el estado final. No simula Ctrl+C ni una parada graceful. Cuando el proceso principal termina espontaneamente, cerrar su Job elimina tambien cualquier descendiente que siga activo.

Al solicitar la salida de Tauri, `RuntimeManager::shutdown` marca primero el runtime como cerrado a nuevos Starts, espera de forma acotada mediante condvar a que los guards RAII de Starts en curso finalicen sin conservar el mutex global, recoge todos los Jobs ya instalados y los termina fuera de los locks. La limpieza de procesos tiene prioridad sobre la publicacion del snapshot: si `runtimeRevision` no puede incrementarse, el estado observable puede permanecer sin transicion a `stopping`, pero el Job instalado se sigue terminando y el error de estado se agrega junto con cualquier error de limpieza. Un Start que termina su creacion despues de comenzar shutdown no pasa a `running`: limpia de inmediato el proceso ya asociado atomicamente al Job. `Drop` aplica la misma politica como segunda barrera; si el proceso de Server Dashboard termina inesperadamente, Windows cierra los handles y `KILL_ON_JOB_CLOSE` elimina los arboles restantes. No se permiten servicios en segundo plano despues de cerrar la aplicacion y no se persiste runtime.

## Estados runtime

El estado en memoria usa:

- `stopped`: no hay proceso activo o Stop forzado termino.
- `starting`: la reserva o creacion esta en curso.
- `running`: el proceso sigue vivo.
- `stopping`: se esta terminando el Job.
- `exited`: el proceso termino espontaneamente con codigo 0.
- `failed`: la creacion fallo o el proceso termino con codigo distinto de 0.

El PID se expone solo en estados activos. La hora de inicio, ultimo codigo de salida y error son opcionales. **Process running** significa exclusivamente que el proceso esta vivo. No significa **Healthy**: esta fase no implementa health checks ni comprobacion de puertos.

Cada snapshot y evento `service-runtime-updated` incluye `runtimeRevision`, un contador `u64` monotono por servicio generado en Rust bajo el mismo mutex que protege el cambio de runtime. Aumenta cuando cambia un dato observable del snapshot runtime: instalacion o retirada de ejecucion, `runId`, estado, PID, hora de inicio, codigo de salida o error. No se reinicia entre ejecuciones y usa overflow checked; una operacion idempotente que no cambia el snapshot, como Stop sobre un servicio ya detenido, conserva la revision.

`runId` identifica la ejecucion; `runtimeRevision` ordena snapshots y eventos. Las respuestas IPC y eventos Tauri pueden llegar fuera de orden, por lo que React acepta solo actualizaciones con revision mayor que la ultima aceptada para ese servicio y mantiene `runId` como defensa adicional para no mezclar una ejecucion antigua con la actual.

Mientras un servicio esta `starting`, `running` o `stopping`, Rust rechaza editarlo o eliminarlo y tambien rechaza renombrar o eliminar su proyecto. La interfaz deshabilita esas acciones, pero el control definitivo permanece en el backend.

## Logs

stdout y stderr se leen en threads separados y cada entrada contiene secuencia global de la ejecucion, fecha RFC 3339, origen y texto. Los bytes no UTF-8 se decodifican de forma tolerante. React renderiza texto normal y no usa HTML procedente del proceso.

Por servicio se mantienen como maximo:

- 2.000 entradas.
- Aproximadamente 2 MiB de texto.
- 16 KiB de entrada por fragmento; una linea mayor se divide.

Al superar un limite se descartan primero las entradas mas antiguas. Clear logs vacia el buffer en Rust, aumenta `logsRevision` y devuelve un snapshot confirmado; la UI no elimina definitivamente entradas si el backend rechaza el Clear. El evento `service-logs-cleared` lleva `projectId`, `serviceId`, `runId` actual opcional, `logsRevision` y entradas vacias. El acuse IPC y el evento se reconcilian por la misma revision, asi que el segundo se trata como duplicado.

Cada `service-log-appended` incluye `projectId`, `serviceId`, `runId`, `logsRevision` y la entrada. `get_service_logs` devuelve un snapshot tipado con `projectId`, `serviceId`, `runId` actual opcional, `logsRevision` y `entries`; nunca devuelve un array desnudo. `logsRevision` es un contador `u64` monotono por servicio, independiente de `runtimeRevision`, que aumenta una sola vez por cada append aceptado aunque ese append descarte una o varias entradas antiguas por el ring buffer, y tambien al hacer Clear y al comenzar una nueva ejecucion que reinicia el buffer. No se reinicia entre ejecuciones y no sustituye a `sequence`: `sequence` ordena entradas dentro del buffer/ejecucion, mientras `logsRevision` ordena mutaciones y snapshots.

Los logs permanecen tras Stop o salida espontanea hasta Clear, hasta alcanzar un limite o hasta comenzar una nueva ejecucion. Como las respuestas IPC y los eventos pueden llegar fuera de orden, React descarta appends, clears o snapshots con `logsRevision` menor o igual que la ultima aceptada para ese servicio; tambien rechaza appends cuyo `runId` no corresponda a la ejecucion de logs visible.

## Persistencia

Rust sigue siendo la unica fuente de verdad persistente. El JSON conserva exactamente el formato versionado `2` en:

```text
%APPDATA%\com.vladut.serverdashboard\projects.json
```

No se modifico la migracion de version 1 a 2. Nunca se escriben PID, estados, logs, handles, codigos de salida, horas de inicio, `runId`, `runtimeRevision` ni `logsRevision`. Las revisiones son efimeras, existen solo en memoria y vuelven a empezar al abrir de nuevo la aplicacion.

Las escrituras de configuracion usan un archivo temporal en el mismo directorio y sustitucion atomica. Eliminar proyectos o servicios elimina solo sus registros; nunca borra carpetas, scripts ni otros archivos reales.

## API Tauri

Comandos runtime especificos:

- `get_service_start_preview`
- `start_service`
- `stop_service`
- `get_service_runtime`
- `get_service_logs`
- `clear_service_logs`

Todos reciben `projectId` y `serviceId`. No existen comandos genericos para shell, procesos, PID, archivos de log o rutas arbitrarias. No se usa el plugin shell ni se agregaron permisos de filesystem. Las capacidades siguen limitadas a `core:default` y `dialog:allow-open`.

## Arquitectura

```text
src\features\projects\
|-- api.ts
|-- ProjectDetailView.tsx
|-- ServiceCard.tsx
|-- ServiceLogsPanel.tsx
|-- StartServiceDialog.tsx
|-- runtime.css
|-- runtimeReconciliation.ts
|-- types.ts
|-- useServiceRuntime.ts
`-- useServices.ts

src-tauri\src\runtime\
|-- commands.rs
|-- emitter.rs
|-- io_tasks.rs
|-- manager.rs
|-- model.rs
|-- state.rs
|-- windows_process.rs
|-- runtime_process_tests.rs
|-- runtime_revision_tests.rs
|-- runtime_test_support.rs
|-- runtime_validation_tests.rs
`-- runtime_windows_job_tests.rs
```

`ProjectStore` conserva configuracion y `RuntimeManager` conserva exclusivamente estado efimero indexado por `(projectId, serviceId)`. El mapa global solo se bloquea para localizar entradas; cada servicio usa su propio mutex y condvar. Ningun mutex global permanece bloqueado durante lecturas, esperas de procesos, `CreateProcessW` o emision de eventos. La espera de Starts durante shutdown usa una condvar que libera su mutex.

No se agrego una feature adicional: `windows-sys` ya expone `PROC_THREAD_ATTRIBUTE_JOB_LIST` mediante la feature existente `Win32_System_Threading`.

## Limitaciones de esta fase

No incluye Restart, Start All, Stop All, ejecucion automatica, inicio al abrir, health checks, comprobacion de puertos, apertura automatica de URL, dependencias entre servicios, variables de entorno personalizadas, stdin interactivo, terminal, deteccion de scripts, Docker, persistencia runtime, elevacion, soporte fuera de Windows ni parada graceful.
