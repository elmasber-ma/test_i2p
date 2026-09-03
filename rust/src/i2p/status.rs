//! Estado legible del router para la UI (sonda SAM + netinfo en vivo).
//! Honestidad > detalle: "corriendo" significa que el proceso vive;
//! "listo" que un HELLO SAM ya respondió; "red: ..." dice conexiones reales.
use emissary_core::events::Event;

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

/// Resume un Event en una línea para el log/pantalla.
pub(super) fn resumir_evento(e: &Event) -> String {
    match e {
        Event::RouterStatus {
            transport,
            tunnel,
            transit,
            firewall_statuses,
            ..
        } => {
            let fw = firewall_statuses
                .iter()
                .map(|(s, v4)| format!("{}:{}", if *v4 { "v4" } else { "v6" }, s))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "conectados: {} · túneles ok {}/fallos {} · tránsito {} · fw [{}]",
                transport.num_connected_routers,
                tunnel.num_tunnels_built,
                tunnel.num_tunnel_build_failures,
                transit.num_tunnels,
                if fw.is_empty() { "-".into() } else { fw }
            )
        }
        Event::ShuttingDown => "apagando…".into(),
        Event::ShutDown => "apagado".into(),
    }
}

/// Info viva del router para la UI. Vacío honesto si aún no hay datos.
pub fn i2p_netinfo() -> String {
    match state::NETINFO.lock().map(|g| g.clone()) {
        Ok(Some(e)) => resumir_evento(&e),
        _ => "sin datos aún — túneles construyéndose, esperá".into(),
    }
}
