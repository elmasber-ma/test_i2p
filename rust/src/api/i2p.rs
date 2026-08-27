/// Binding FRB del módulo I2P (patrón laurelia.rs): archivo plano que FRB
/// convierte en `api/i2p.dart`. La implementación vive en `crate::i2p::*`.
use crate::i2p;

#[flutter_rust_bridge::frb]
pub fn i2p_start(
    data_dir: String,
    sam_port: u16,
    transport_port: u16,
    publicar: bool,
    reseed_hosts: Vec<String>,
) -> Result<String, String> {
    i2p::i2p_start(data_dir, sam_port, transport_port, publicar, reseed_hosts)
}

#[flutter_rust_bridge::frb]
pub fn i2p_stop() -> Result<(), String> {
    i2p::i2p_stop()
}

#[flutter_rust_bridge::frb]
pub fn i2p_is_running() -> bool {
    i2p::i2p_is_running()
}

#[flutter_rust_bridge::frb]
pub fn i2p_estado() -> String {
    i2p::i2p_estado()
}

#[flutter_rust_bridge::frb]
pub fn i2p_sam_port() -> Option<u16> {
    i2p::i2p_sam_port()
}

#[flutter_rust_bridge::frb]
pub fn i2p_probe_sam() -> String {
    i2p::i2p_probe_sam()
}

#[flutter_rust_bridge::frb]
pub fn i2p_http_get(url: String) -> Result<String, String> {
    i2p::i2p_http_get(url)
}

#[flutter_rust_bridge::frb]
pub fn i2p_download(url: String, dest_path: String) -> Result<u64, String> {
    i2p::i2p_download(url, dest_path)
}

#[flutter_rust_bridge::frb]
pub fn i2p_get_logs() -> Vec<String> {
    i2p::log_get()
}

#[flutter_rust_bridge::frb]
pub fn i2p_clear_logs() {
    i2p::log_clear()
}
