/// Estado compartido del módulo I2P: única fuente de verdad para todos los
/// submódulos (accesan por `super::state`). Mismo patrón de estáticos que
/// tor.rs y needle.rs.
use std::sync::{Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

/// Tarea del router (futuro `Router` spawneado en tokio). None = apagado;
/// Some con is_finished() = el router murió por fuera (Android en bg).
pub(super) static ROUTER_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Directorio de datos de la app (para cachear hosts.txt), fijado en arranque.
pub(super) static DATA_DIR: Mutex<String> = Mutex::new(String::new());

/// 0 apagado · 1 bootstrapeando/reseeding · 2 corriendo · 3 listo (SAM probado)
pub(super) static ESTADO: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Puerto SAMv3 elegido por Dart (libre al azar, mismo patrón que Tor).
pub(super) static SAM_PORT: std::sync::atomic::AtomicU16 =
    std::sync::atomic::AtomicU16::new(0);

/// Log global del módulo (reseed, arranque, errores). Visible desde Dart.

use std::sync::atomic::Ordering;

pub(super) fn estado_set(v: u8) {
    ESTADO.store(v, Ordering::Relaxed);
}

pub(super) fn estado_get() -> u8 {
    ESTADO.load(Ordering::Relaxed)
}

pub(super) fn sam_port_set(v: u16) {
    SAM_PORT.store(v, Ordering::Relaxed);
}

pub(super) fn sam_port_get() -> u16 {
    SAM_PORT.load(Ordering::Relaxed)
}

pub(super) fn data_dir_get() -> String {
    DATA_DIR.lock().map(|g| g.clone()).unwrap_or_default()
}

/// ¿Hay tarea de router viva?
pub(crate) fn router_vivo() -> bool {
    matches!(ROUTER_TASK.lock(), Ok(g) if g.as_ref().map(|h| !h.is_finished()).unwrap_or(false))
}



/// Runtime tokio global del módulo (patrón tor.rs): &'static Result para no
/// paniquear en construcción y poder mapear el error como String legible.
pub(super) fn runtime() -> &'static Result<Runtime, String> {
    static RT: OnceLock<Result<Runtime, String>> = OnceLock::new();
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime tokio: {e}"))
    })
}
