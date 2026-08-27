/// Binding FRB del módulo I2P (patrón laurelia.rs): archivo plano que FRB
/// convierte en `api/i2p.dart`. La implementación vive en `crate::i2p::*`.
use crate::i2p;
use std::sync::Mutex;

pub static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn log_push(m: &str) {
    eprintln!("[i2p] {m}");
    if let Ok(mut logs) = LOGS.lock() {
        logs.insert(0, m.to_string());
        if logs.len() > 200 {
            logs.truncate(200);
        }
    }
}

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
    LOGS.lock().map(|g| g.clone()).unwrap_or_default()
}

#[flutter_rust_bridge::frb]
pub fn i2p_clear_logs() {
    if let Ok(mut logs) = LOGS.lock() {
        logs.clear();
    }
}
