use std::sync::Mutex;

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn log_push(m: &str) {
    eprintln!("[i2p] {m}");
    if let Ok(mut logs) = LOGS.lock() {
        logs.insert(0, m.to_string());
        if logs.len() > 200 {
            logs.truncate(200);
        }
    }
}

pub fn log_get() -> Vec<String> {
    LOGS.lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn log_clear() {
    if let Ok(mut logs) = LOGS.lock() {
        logs.clear();
    }
}
