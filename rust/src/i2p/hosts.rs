//! AddressBook propio: hosts.txt (nombre.i2p → destino base64) cacheado en
//! disco. Sin esto los nombres `.i2p` no resuelven; las direcciones
//! `xxx.b32.i2p` funcionan siempre (el router las busca en NetDb).
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{sam, state};

/// Mirrors clearnet públicos de hosts.txt; se prueba en orden (solo clearnet válidos).
const FUENTES: &[&str] = &[
    "https://i2p.net/hosts.txt",
    "https://raw.githubusercontent.com/i2p/i2p.i2p/master/installer/resources/hosts.txt",
];

/// Default oficial de emissary-cli (config.rs): hosts.txt desde adentro de
/// I2P vía b32 — el router lo resuelve por NetDb sin necesitar hosts.txt.
const HOSTS_B32: &str = "udhdrtrcetjm5sxzskjyr5ztpeszydbh4dpl3pl4utgqqw2v4jna.b32.i2p";

/// Fallback embebido (m3u txt) — sin parsear como json, texto plano nombre=base64
const EMBEDDED_TXT: &str = include_str!("../../assets/hosts.txt");

/// Se re-baja si el cacheado tiene más de 7 días.
const CADUCIDAD: Duration = Duration::from_secs(7 * 24 * 3600);

type Mapa = HashMap<String, String>;
static MAPA: OnceLock<Mutex<Option<(SystemTime, Mapa)>>> = OnceLock::new();

fn celda() -> &'static Mutex<Option<(SystemTime, Mapa)>> {
    MAPA.get_or_init(|| Mutex::new(None))
}

async fn bajar(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Wget/1.11.4")
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("client {url}: {e}"))?;
    let r = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("HTTP {} en {url}", r.status()));
    }
    r.text().await.map_err(|e| format!("cuerpo {url}: {e}"))
}

/// Baja hosts.txt por el túnel SAM (router vivo). No pasa por resolver()
/// para evitar recursión: el b32 va directo a SAM vía NetDb.
async fn bajar_via_i2p() -> Result<String, String> {
    let port = state::sam_port_get();
    if !state::router_vivo() || port == 0 {
        return Err("router no vivo para bajar hosts.txt por I2P".into());
    }
    let fut = async {
        let session = format!(
            "h{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64 + d.as_secs() % 1_000_000_000)
                .unwrap_or(7)
                % 1_000_000_000
        );
        let mut s = sam::stream_connect(port, HOSTS_B32, &session).await?;
        let req = format!(
            "GET /hosts.txt HTTP/1.1\r\nHost: {HOSTS_B32}\r\nUser-Agent: mimapp\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes())
            .await
            .map_err(|e| format!("enviar request: {e}"))?;
        let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
        let mut tmp = [0u8; 16384];
        loop {
            let n = s.read(&mut tmp).await.map_err(|e| format!("lectura: {e}"))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.len() > 4 * 1024 * 1024 {
                break;
            }
        }
        let sep = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("respuesta sin headers")?;
        let status: u16 = String::from_utf8_lossy(&buf[..sep])
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .ok_or("status ilegible")?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status} vía I2P"));
        }
        String::from_utf8(buf[sep + 4..].to_vec()).map_err(|e| format!("utf8: {e}"))
    };
    tokio::time::timeout(Duration::from_secs(60), fut)
        .await
        .map_err(|_| "timeout hosts.txt vía I2P (túneles aún construyéndose)".to_string())?
}

fn parsear(texto: &str) -> Mapa {
    let mut m = Mapa::new();
    for l in texto.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((nombre, dest)) = l.split_once('=') {
            let nombre = nombre.trim().to_ascii_lowercase();
            let dest = dest.trim();
            if !nombre.is_empty() && !dest.is_empty() {
                m.insert(nombre, dest.to_string());
            }
        }
    }
    m
}

async fn asegurar_mapa(data_dir: &str) -> Result<Mapa, String> {
    if let Ok(g) = celda().lock() {
        if let Some((t, mapa)) = g.as_ref() {
            if t.elapsed().unwrap_or(CADUCIDAD) < CADUCIDAD {
                return Ok(mapa.clone());
            }
        }
    }

    // 1) intentar cache en disco aunque sea viejo (mejor que nada)
    let cache = Path::new(data_dir).join("i2p_hosts.txt");
    let mut mapa_disco: Option<Mapa> = None;
    if cache.exists() {
        if let Ok(txt) = std::fs::read_to_string(&cache) {
            let m = parsear(&txt);
            if !m.is_empty() {
                mapa_disco = Some(m);
            }
        }
    }

    // 2) intentar bajar fresco de algún mirror
    let mut fresco: Option<(Mapa, String)> = None;
    for f in FUENTES {
        match bajar(f).await {
            Ok(txt) => {
                let m = parsear(&txt);
                if !m.is_empty() {
                    let _ = std::fs::write(&cache, &txt);
                    fresco = Some((m.clone(), txt));
                    break;
                }
            }
            Err(_) => continue,
        }
    }

    // 2b) vía I2P (b32 oficial) si el router está vivo — refresca sin clearnet
    if fresco.is_none() && state::router_vivo() {
        match bajar_via_i2p().await {
            Ok(txt) => {
                let m = parsear(&txt);
                if !m.is_empty() {
                    let _ = std::fs::write(&cache, &txt);
                    fresco = Some((m.clone(), txt));
                }
            }
            Err(e) => {
                super::log::log_push(&format!("hosts.txt vía I2P falló: {e}"));
            }
        }
    }

    match fresco {
        Some((m, _)) => {
            if let Ok(mut g) = celda().lock() {
                *g = Some((SystemTime::now(), m.clone()));
            }
            Ok(m)
        }
        None => {
            // 3) fallback embebido — no bloquea router, permite empezar sin host
            let emb = parsear(EMBEDDED_TXT);
            if !emb.is_empty() {
                let _ = std::fs::write(&cache, EMBEDDED_TXT);
                if let Ok(mut g) = celda().lock() {
                    *g = Some((SystemTime::now(), emb.clone()));
                }
                return Ok(emb);
            }
            mapa_disco.ok_or_else(|| {
                "hosts.txt no disponible (sin mirrors ni caché): sin host o buscando host podes empezar — usá xxx.b32.i2p o base64"
                    .into()
            })
        }
    }
}

/// Resuelve [host] a un DESTINATION utilizable por SAMv3:
/// - base64 crudo (contiene '=' y es largo) → tal cual
/// - `xxx.b32.i2p` → tal cual (el router lo busca en NetDb)
/// - `nombre.i2p`  → lookup hosts.txt → destino base64
pub(super) async fn resolver(host: &str, data_dir: &str) -> Result<String, String> {
    let h = host.trim().to_ascii_lowercase();
    if !h.ends_with(".i2p") {
        if h.len() > 40 && h.contains('=') {
            return Ok(h); // destino completo en base64 pegado por el usuario
        }
        return Err(format!("{h} no es un destino I2P (*.i2p, *.b32.i2p o base64)"));
    }
    if h.ends_with(".b32.i2p") {
        return Ok(h);
    }
    let mapa = asegurar_mapa(data_dir).await?;
    mapa
        .get(&h)
        .cloned()
        .ok_or_else(|| format!("{h} no está en hosts.txt (probá su .b32.i2p)"))
}
