/// Módulo I2P embebido (router emissary, MIT) — sin puente local: Rust habla
/// DIRECTO con su propio router vía SAMv3. Submódulos por funcionalidad:
///
/// - `state`   estáticos compartidos (única fuente)
/// - `router`  ciclo de vida (start/stop/is_running)
/// - `status`  estado legible + sonda SAM
/// - `sam`     cliente SAMv3 mínimo (STREAM)
/// - `tunnel`  GET y descarga directa sobre SAMv3
/// - `hosts`   resolución nombre.i2p → destino (hosts.txt cacheado)
mod hosts;
mod router;
mod sam;
mod state;
mod status;
mod tunnel;

#[flutter_rust_bridge::frb]
pub fn i2p_start(
    data_dir: String,
    sam_port: u16,
    transport_port: u16,
    publicar: bool,
) -> Result<String, String> {
    router::i2p_start(data_dir, sam_port, transport_port, publicar)
}

#[flutter_rust_bridge::frb]
pub fn i2p_stop() -> Result<(), String> {
    router::i2p_stop()
}

#[flutter_rust_bridge::frb]
pub fn i2p_is_running() -> bool {
    router::i2p_is_running()
}

#[flutter_rust_bridge::frb]
pub fn i2p_estado() -> String {
    status::i2p_estado()
}

#[flutter_rust_bridge::frb]
pub fn i2p_sam_port() -> Option<u16> {
    status::i2p_sam_port()
}

#[flutter_rust_bridge::frb]
pub fn i2p_probe_sam() -> String {
    status::i2p_probe_sam()
}

#[flutter_rust_bridge::frb]
pub fn i2p_http_get(url: String) -> Result<String, String> {
    tunnel::i2p_http_get(url)
}

#[flutter_rust_bridge::frb]
pub fn i2p_download(url: String, dest_path: String) -> Result<u64, String> {
    tunnel::i2p_download(url, dest_path)
}
