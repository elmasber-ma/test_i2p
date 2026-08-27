use std::sync::Mutex;

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(crate) fn log_push(m: &str) {
    eprintln!("[i2p] {m}");
    if let Ok(mut logs) = LOGS.lock() {
        logs.insert(0, m.to_string());
        if logs.len() > 200 {
            logs.truncate(200);
        }
    }
}

#[flutter_rust_bridge::frb]
pub fn i2p_get_logs() -> String {
    LOGS.lock().map(|g| g.join("\n")).unwrap_or_default()
}

#[flutter_rust_bridge::frb]
pub fn i2p_clear_logs() -> Result<(), String> {
    if let Ok(mut logs) = LOGS.lock() {
        logs.clear();
    }
    Ok(())
}
