//! Ciclo de vida del router emissary embebido.
use std::sync::Arc;
use std::time::{Duration, Instant};

use emissary_core::router::Router;
use emissary_core::{Config, Ntcp2Config, SamConfig, Ssu2Config, TransitConfig};
use emissary_util::runtime::tokio::Runtime;
use emissary_util::storage::{Storage, StorageBundle};
use emissary_util::certificates::CREATIVECOWPAT_SSL;
use emissary_util::su3::Su3;
use reqwest::Certificate;

use super::log::log_push;
use super::state;

/// Arranca el router I2P. Bloquea mientras bootstrapea/reseededea (llamar
/// desde future/isolate aparte). Los puertos los elige Dart libres al azar:
/// [sam_port] para SAMv3 TCP y [transport_port] para NTCP2/SSU2 (UDP mismo
/// número, namespace distinto), así nada pisa al Tor SOCKS5 ni a nada más.
///
/// [publicar]: false por defecto (CGNAT móvil: nadie puede alcanzar el
/// puerto igual). Solo activar con IP real + puerto forwardeado. IPv6 va
/// siempre activo; si el equipo no tiene v6 el bind de salida simplemente
/// no alcanza peers v6 y sigue por v4.
#[flutter_rust_bridge::frb]
pub fn i2p_start(
    data_dir: String,
    sam_port: u16,
    transport_port: u16,
    publicar: bool,
    reseed_hosts: Vec<String>,
) -> Result<String, String> {
    if state::router_vivo() {
        return Err("I2P ya está corriendo".into());
    }
    state::estado_set(1);
    state::sam_port_set(sam_port);
    if let Ok(mut g) = state::DATA_DIR.lock() {
        *g = data_dir.clone();
    }

    let base = std::path::Path::new(&data_dir).join(".emissary");

    state::runtime()
        .as_ref()
        .map_err(|e| e.clone())?
        .block_on(async move { arrancar(base, sam_port, transport_port, publicar, reseed_hosts).await })
}

async fn arrancar(
    base: std::path::PathBuf,
    sam_port: u16,
    transport_port: u16,
    publicar: bool,
    reseed_hosts: Vec<String>,
) -> Result<String, String> {
    let storage = Storage::new::<Runtime>(Some(base))
        .await
        .map_err(|e| format!("storage: {e}"))?;

    let StorageBundle {
        ntcp2_iv,
        ntcp2_key,
        profiles,
        router_info,
        mut routers,
        signing_key,
        static_key,
        ssu2_intro_key,
        ssu2_static_key,
    } = storage.load().await;

    // Reseed: HTTP directo por URL, 5s timeout cada una, log en pantalla.
    if routers.is_empty() {
        let n = reseed_hosts.len();
        log_push(&format!("=== RESEED: {n} hosts, 5s timeout c/u ==="));
        let t0 = Instant::now();

        let client = {
            let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(15));
            if let Ok(cert) = Certificate::from_pem(CREATIVECOWPAT_SSL.as_bytes()) {
                builder = builder.add_root_certificate(cert);
            }
            builder
                .user_agent("Wget/1.11.4")
                .build()
                .map_err(|e| format!("http client: {e}"))?
        };

        for (i, host) in reseed_hosts.iter().enumerate() {
            let t1 = Instant::now();
            let host_trim = host.trim_end_matches('/');
            let url = format!("{host_trim}/i2pseeds.su3");
            log_push(&format!("[{}/{}] {url} ...", i + 1, n));

            let bytes = match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let ms = t1.elapsed().as_millis();
                        log_push(&format!(
                            "[{}/{}] {host} → HTTP {status} ({ms}ms)",
                            i + 1, n
                        ));
                        continue;
                    }
                    match resp.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            let ms = t1.elapsed().as_millis();
                            log_push(&format!(
                                "[{}/{}] {host} → body err ({ms}ms): {e}",
                                i + 1, n
                            ));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    let ms = t1.elapsed().as_millis();
                    log_push(&format!(
                        "[{}/{}] {host} → FAIL ({ms}ms): {e}",
                        i + 1, n
                    ));
                    continue;
                }
            };

            let ms = t1.elapsed().as_millis();
            // primero intenta con verificación (true); si falla por cert expirado, reintenta sin verificar
            let parsed = Su3::parse_reseed(&bytes, true).or_else(|| {
                log_push(&format!(
                    "[{}/{}] {host} → verify fail, reintentando sin verify ({ms}ms)",
                    i + 1, n
                ));
                Su3::parse_reseed(&bytes, false)
            });
            match parsed {
                Some(nuevos) => {
                    log_push(&format!(
                        "[{}/{}] {host} → OK {} routers ({ms}ms)",
                        i + 1, n, nuevos.len()
                    ));
                    for info in nuevos {
                        storage
                            .store_router_info(info.name.to_string(), info.router_info.clone())
                            .await
                            .map_err(|e| format!("guardar router info: {e}"))?;
                        routers.push(info.router_info);
                    }
                }
                None => {
                    log_push(&format!(
                        "[{}/{}] {host} → SU3 parse fail ({ms}ms, {}B)",
                        i + 1, n, bytes.len()
                    ));
                }
            }
        }

        let total_ms = t0.elapsed().as_millis();
        if routers.is_empty() {
            log_push(&format!(
                "=== RESEED FALLÓ: 0 routers, {total_ms}ms, {n} hosts ==="
            ));
            let logs = super::log::log_get().join("\n");
            return Err(format!(
                "reseed falló: 0 routers de {n} hosts en {total_ms}ms\n{logs}"
            ));
        }
        log_push(&format!(
            "=== RESEED OK: {} routers en {total_ms}ms ===",
            routers.len()
        ));
    } else {
        log_push(&format!(
            "{} routers en disco, saltando reseed",
            routers.len()
        ));
    }

    // Multiplataforma sin recortes: IPv4+IPv6, PQ activo, transit como el
    // ejemplo oficial (1000). Lo ÚNICO opcional es publicar direcciones
    // (parámetro [publicar], default off porque tras CGNAT nadie alcanza
    // el puerto y solo genera intentos fallidos de otros routers).
    let config = Config {
        ntcp2: Some(Ntcp2Config {
            port: transport_port,
            key: ntcp2_key,
            iv: ntcp2_iv,
            publish_ipv4: publicar,
            publish_ipv6: publicar,
            ipv4_host: None,
            ipv6_host: None,
            ipv4: true,
            ipv6: true,
            ml_kem: Some(4),
            max_connections: None,
            disable_pq: false,
        }),
        ssu2: Some(Ssu2Config {
            intro_key: ssu2_intro_key,
            static_key: ssu2_static_key,
            ipv4: true,
            ipv4_host: None,
            ipv6: true,
            ipv6_host: None,
            port: transport_port,
            publish_ipv4: publicar,
            publish_ipv6: publicar,
            ipv4_mtu: None,
            ipv6_mtu: None,
            disable_pq: false,
            ml_kem: Some("4".to_string()),
            max_connections: None,
        }),
        samv3_config: Some(SamConfig {
            tcp_port: sam_port,
            udp_port: sam_port, // TCP y UDP son namespaces distintos
            host: "127.0.0.1".to_string(),
        }),
        routers,
        profiles,
        router_info,
        static_key: Some(static_key),
        signing_key: Some(signing_key),
        transit: Some(TransitConfig { max_tunnels: Some(1000) }),
        ..Default::default()
    };

    let storage_arc = Arc::new(storage);

    let (router, _events, router_info) = Router::<Runtime>::new(config, None, Some(storage_arc.clone()))
        .await
        .map_err(|e| format!("arranque del router: {e}"))?;

    storage_arc
        .store_local_router_info(router_info)
        .await
        .map_err(|e| format!("guardar router info local: {e}"))?;

    let handle = tokio::spawn(router);
    if let Ok(mut g) = state::ROUTER_TASK.lock() {
        *g = Some(handle);
    }
    state::estado_set(2);
    // devolver también el log de reseed para que Dart lo muestre sin FRB poll
    let logs = super::log::log_get().join("\n");
    if logs.is_empty() {
        Ok(format!(
            "router vivo · SAMv3 127.0.0.1:{sam_port} · transports en {transport_port} (no publicados)"
        ))
    } else {
        Ok(format!(
            "router vivo · SAMv3 127.0.0.1:{sam_port} · transports en {transport_port} (no publicados)\n{logs}"
        ))
    }
}

/// Corta el router. Idempotente.
#[flutter_rust_bridge::frb]
pub fn i2p_stop() -> Result<(), String> {
    state::estado_set(0);
    if let Ok(mut g) = state::ROUTER_TASK.lock() {
        if let Some(h) = g.take() {
            h.abort();
        }
    }
    state::sam_port_set(0);
    Ok(())
}

/// ¿El router sigue vivo? Detecta muertes silenciosas (Android en segundo
/// plano) igual que tor_is_running con is_finished().
#[flutter_rust_bridge::frb]
pub fn i2p_is_running() -> bool {
    state::router_vivo()
}
