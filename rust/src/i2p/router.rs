//! Ciclo de vida del router emissary embebido.
use std::sync::Arc;
use std::time::Instant;

use emissary_core::router::Router;
use emissary_core::{Config, Ntcp2Config, SamConfig, Ssu2Config, TransitConfig};
use emissary_util::reseeder::Reseeder;
use emissary_util::runtime::tokio::Runtime;
use emissary_util::storage::{Storage, StorageBundle};
use emissary_util::su3::Su3;

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
//
// Implementación pelada (sin #[frb]): el puente FRB vive solo en api/i2p.rs.
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
    let storage = Storage::new::<Runtime>(Some(base.clone()))
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

    // Reseed: privado → embebido → Download (suave) → red. Local primero
    // para no depender de red; la red solo si todo lo local falla.
    if routers.is_empty() {
        let t0 = Instant::now();
        // asegurar que TempDir use dir privado escribible en Android (evita /tmp sin permiso)
        std::env::set_var("TMPDIR", base.display().to_string());
        let _ = std::fs::create_dir_all(&base);
        // 1) privado: base/i2pseeds.su3 (sin tocar Download para evitar Permission denied)
        let priv_cand = base.join("i2pseeds.su3").display().to_string();
        if let Ok(bytes) = tokio::fs::read(&priv_cand).await {
            log_push(&format!("=== RESEED local privado: {priv_cand} ({}B) ===", bytes.len()));
            let parsed = Su3::parse_reseed(&bytes, true).or_else(|| Su3::parse_reseed(&bytes, false));
            if let Some(v) = parsed {
                log_push(&format!("=== RESEED local OK: {} routers ===", v.len()));
                for info in v {
                    let _ = storage.store_router_info(info.name.to_string(), info.router_info.clone()).await;
                    routers.push(info.router_info);
                }
            } else {
                log_push(&format!("=== RESEED local parse fail: {} ===", priv_cand));
            }
        } else {
            log_push(&format!("=== RESEED local no encontrado {} (usando embebido) ===", priv_cand));
        }
        // fallback embebido en binario (varios su3 48-65K) — sin parse manual extra, Su3 en memoria
        if routers.is_empty() {
            const EMBEDDEDS: &[&[u8]] = &[
                include_bytes!("../../assets/i2pseeds.su3"),
                include_bytes!("../../assets/i2pseeds2.su3"),
                include_bytes!("../../assets/i2pseeds3.su3"),
            ];
            for (idx, emb) in EMBEDDEDS.iter().enumerate() {
                if emb.is_empty() || !emb.starts_with(b"I2Psu3") {
                    continue;
                }
                log_push(&format!("=== RESEED embebido {}: {}B ===", idx + 1, emb.len()));
                let parsed = Su3::parse_reseed(emb, true).or_else(|| Su3::parse_reseed(emb, false));
                if let Some(v) = parsed {
                    log_push(&format!("=== RESEED embebido {} OK: {} routers ===", idx + 1, v.len()));
                    for info in v {
                        let _ = storage.store_router_info(info.name.to_string(), info.router_info.clone()).await;
                        routers.push(info.router_info);
                    }
                } else {
                    log_push(&format!("=== RESEED embebido {} parse fail ===", idx + 1));
                }
                if routers.len() >= 80 {
                    break;
                }
            }
        }
        // 2b) Download solo si nada local funcionó (último recurso, sin copiar)
        if routers.is_empty() {
            let dl_cand = "/storage/emulated/0/Download/i2pseeds.su3".to_string();
            match tokio::fs::read(&dl_cand).await {
                Ok(bytes) => {
                    log_push(&format!("=== RESEED Download: {}B ===", bytes.len()));
                    let parsed = Su3::parse_reseed(&bytes, true).or_else(|| Su3::parse_reseed(&bytes, false));
                    if let Some(v) = parsed {
                        log_push(&format!("=== RESEED Download OK: {} routers ===", v.len()));
                        for info in v {
                            let _ = storage.store_router_info(info.name.to_string(), info.router_info.clone()).await;
                            routers.push(info.router_info);
                        }
                    } else {
                        log_push("=== RESEED Download parse fail ===");
                    }
                }
                Err(_) => {
                    log_push("=== RESEED Download sin acceso (permiso), sigo con red ===");
                }
            }
        }
        // 3) red siempre: aunque lo local dio routers, los seeders pueden
        // estar viejos → se intenta Reseeder igual y se mezclan los nuevos.
        // Si la red falla pero ya hay routers locales, se sigue con esos.
        {
            let n = reseed_hosts.len();
            log_push(&format!("=== RESEED red: {n} hosts via Reseeder ==="));
            match Reseeder::reseed::<Runtime>(Some(reseed_hosts.clone()), false).await {
                Ok(v) => {
                    let total_ms = t0.elapsed().as_millis();
                    let mut nuevos = 0usize;
                    for info in v {
                        storage
                            .store_router_info(info.name.to_string(), info.router_info.clone())
                            .await
                            .map_err(|e| format!("guardar router info: {e}"))?;
                        routers.push(info.router_info);
                        nuevos += 1;
                    }
                    log_push(&format!(
                        "=== RESEED red OK: +{nuevos} routers (total {}) en {total_ms}ms ===",
                        routers.len()
                    ));
                    let marker = base.join("reseed.ok");
                    let _ = tokio::fs::write(&marker, format!("{} routers {}", routers.len(), total_ms)).await;
                    log_push(&format!("privado guardado en {}", marker.display()));
                }
                Err(e) => {
                    let total_ms = t0.elapsed().as_millis();
                    if routers.is_empty() {
                        log_push(&format!("=== RESEED FALLÓ: {e} ({total_ms}ms, {n} hosts) ==="));
                        let logs = super::log::log_get().join("\n");
                        return Err(format!("reseed falló: {e} en {total_ms}ms\n{logs}"));
                    }
                    log_push(&format!(
                        "=== RESEED red falló ({e}), sigo con {} routers locales ===",
                        routers.len()
                    ));
                }
            }
            let total_ms = t0.elapsed().as_millis();
            log_push(&format!("=== RESEED total {} routers en {total_ms}ms ===", routers.len()));
        }
    } else {
        log_push(&format!(
            "{} routers en disco, saltando reseed (privado: {})",
            routers.len(),
            base.display()
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
    // monitor en background: cada 60s informa netDb para que la UI no quede muda.
    // Los túneles se construyen solos en 2-10 min; el usuario ve el progreso.
    {
        let base_mon = base.clone();
        let t_boot = Instant::now();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                if !state::router_vivo() {
                    break;
                }
                let n = contar_netdb(&base_mon).await;
                let mins = t_boot.elapsed().as_secs() / 60;
                log_push(&format!(
                    "túneles: router vivo hace {mins}min · netDb {n} archivos en disco · si el GET da timeout, esperá y reintentá"
                ));
            }
        });
    }
    let pub_txt = if publicar { "publicados" } else { "no publicados" };
    // devolver también el log de reseed para que Dart lo muestre sin FRB poll
    let logs = super::log::log_get().join("\n");
    if logs.is_empty() {
        Ok(format!(
            "router vivo · SAMv3 127.0.0.1:{sam_port} · transports en {transport_port} ({pub_txt})"
        ))
    } else {
        Ok(format!(
            "router vivo · SAMv3 127.0.0.1:{sam_port} · transports en {transport_port} ({pub_txt})\n{logs}"
        ))
    }
}

/// Cuenta archivos bajo base/.emissary (netDb + profiles + keys) como
/// aproximación del crecimiento de netDb. Barato: unos cientos de archivos.
async fn contar_netdb(base: &std::path::Path) -> usize {
    let mut n = 0usize;
    let mut dirs = vec![base.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let mut rd = match tokio::fs::read_dir(&d).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        loop {
            match rd.next_entry().await {
                Ok(Some(e)) => {
                    let p = e.path();
                    if p.is_dir() {
                        dirs.push(p);
                    } else {
                        n += 1;
                    }
                }
                _ => break,
            }
        }
    }
    n
}

/// Corta el router. Idempotente.
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
pub fn i2p_is_running() -> bool {
    state::router_vivo()
}
