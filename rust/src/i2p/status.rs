//! Estado legible del router para la UI (sin EventSubscriber: hitos propios
//! del ciclo de vida + sonda SAM). Honestidad > detalle: "corriendo" significa
//! que el proceso vive; "listo" que un HELLO SAM ya respondió.
use super::state;

/// Estado humano del ciclo de vida del router I2P.
pub fn i2p_estado() -> String {
    match state::estado_get() {
        1 => "bootstrapeando/reseeding…".into(),
        2 => "corriendo (túneles construyendo…)".into(),
        3 => "listo".into(),
        _ => "apagado".into(),
    }
}

/// Puerto SAMv3 activo, o None si está apagado.
pub fn i2p_sam_port() -> Option<u16> {
    let p = state::sam_port_get();
    if state::router_vivo() && p != 0 {
        Some(p)
    } else {
        None
    }
}

/// Sonda real: ¿el SAMv3 acepta conexiones? Al primer OK sube el estado a
/// "listo" (3). No lanza: devuelve diagnóstico en texto.
pub fn i2p_probe_sam() -> String {
    let port = match i2p_sam_port() {
        Some(p) => p,
        None => return "apagado: no hay puerto SAM".into(),
    };
    let rt = match state::runtime().as_ref() {
        Ok(r) => r,
        Err(e) => return format!("runtime no disponible: {e}"),
    };
    let r = rt.block_on(async {
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    match r {
        Ok(()) => {
            if state::estado_get() < 3 {
                state::estado_set(3);
            }
            format!("SAM vivo en 127.0.0.1:{port}")
        }
        Err(e) => format!("SAM no responde aún ({e}); los túneles siguen construyéndose"),
    }
}
