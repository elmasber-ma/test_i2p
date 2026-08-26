//! Ciclo de vida del router emissary embebido. Porte del ejemplo oficial
//! rust-tutorial ajustado a móvil: sin puertos publicados (no inbound),
//! IPv4 solamente y transit tunnels en cero (batería).
use std::sync::Arc;

use emissary_core::router::Router;
use emissary_core::{Config, Ntcp2Config, SamConfig, Ssu2Config, TransitConfig};
use emissary_util::reseeder::Reseeder;
use emissary_util::runtime::tokio::Runtime;
use emissary_util::storage::{Storage, StorageBundle};

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

    // Init tracing para ver logs del reseeder
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "emissary=debug".parse().unwrap()),
        )
        .try_init();

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

    // Reseed solo la primera vez (HTTPS a floodfills claros); después los
    // routers conocidos viven en disco.
    if routers.is_empty() {
        eprintln!("[i2p] routers vacíos, intentando reseed con {} hosts...", reseed_hosts.len());
        for (i, h) in reseed_hosts.iter().enumerate() {
            eprintln!("[i2p]   host[{i}]: {h}");
        }
        match Reseeder::reseed::<Runtime>(Some(reseed_hosts), true).await {
            Ok(nuevos) => {
                eprintln!("[i2p] reseed OK: {} routers descargados", nuevos.len());
                for info in nuevos {
                    storage
                        .store_router_info(info.name.to_string(), info.router_info.clone())
                        .await
                        .map_err(|e| format!("guardar router info: {e}"))?;
                    routers.push(info.router_info);
                }
            }
            Err(e) if routers.is_empty() => {
                eprintln!("[i2p] reseed FALLO y no hay routers guardados: {e}");
                return Err(format!("reseed falló y no hay routers guardados: {e}"));
            }
            Err(e) => eprintln!("[i2p] reseed falló (hay {nombres} en disco): {e}", nombres = routers.len()),
        }
    } else {
        eprintln!("[i2p] {} routers en disco, saltando reseed", routers.len());
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
    Ok(format!(
        "router vivo · SAMv3 127.0.0.1:{sam_port} · transports en {transport_port} (no publicados)"
    ))
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
