/// Módulo I2P embebido (router emissary, MIT) — sin puente local: Rust habla
/// DIRECTO con su propio router vía SAMv3. Submódulos por funcionalidad:
///
/// - `state`   estáticos compartidos (única fuente)
/// - `router`  ciclo de vida (start/stop/is_running)
/// - `status`  estado legible + sonda SAM
/// - `sam`     cliente SAMv3 mínimo (STREAM)
/// - `tunnel`  GET y descarga directa sobre SAMv3
/// - `hosts`   resolución nombre.i2p → destino (hosts.txt cacheado)
pub mod hosts;
pub mod router;
pub mod sam;
pub mod state;
pub mod status;
pub mod tunnel;

pub use router::{i2p_is_running, i2p_start, i2p_stop};
pub use status::{i2p_estado, i2p_probe_sam, i2p_sam_port};
pub use tunnel::{i2p_download, i2p_http_get};
