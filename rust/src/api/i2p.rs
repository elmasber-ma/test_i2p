//! I2P embebido (emissary, MIT) — router Rust puro, sin puente local.
//! Patrón tor.rs: todo en un solo archivo para que FRB genere `i2p.dart`.
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use emissary_core::router::Router;
use emissary_core::{Config, Ntcp2Config, SamConfig, Ssu2Config, TransitConfig};
use emissary_util::reseeder::Reseeder;
use emissary_util::runtime::tokio::Runtime;
use emissary_util::storage::{Storage, StorageBundle};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime::{Builder, Runtime as TokioRuntime};
use tokio::task::JoinHandle;

// ──────────────────────────── state ────────────────────────────

static ROUTER_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static DATA_DIR: Mutex<String> = Mutex::new(String::new());
static ESTADO: AtomicU8 = AtomicU8::new(0);
static SAM_PORT: AtomicU16 = AtomicU16::new(0);

fn estado_set(v: u8) { ESTADO.store(v, Ordering::Relaxed); }
fn estado_get() -> u8 { ESTADO.load(Ordering::Relaxed) }
fn sam_port_set(v: u16) { SAM_PORT.store(v, Ordering::Relaxed); }
fn sam_port_get() -> u16 { SAM_PORT.load(Ordering::Relaxed) }
fn data_dir_get() -> String { DATA_DIR.lock().map(|g| g.clone()).unwrap_or_default() }

fn router_vivo() -> bool {
    matches!(ROUTER_TASK.lock(), Ok(g) if g.as_ref().map(|h| !h.is_finished()).unwrap_or(false))
}

fn runtime() -> &'static Result<TokioRuntime, String> {
    static RT: OnceLock<Result<TokioRuntime, String>> = OnceLock::new();
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime tokio: {e}"))
    })
}

// ──────────────────────────── hosts ────────────────────────────

const FUENTES: &[&str] = &[
    "https://raw.githubusercontent.com/JustABoy/i2p-get/master/hosts.txt",
    "https://download.i2p2.no/hosts.txt",
    "https://stats.i2p/downloads/hosts.txt",
];
const CADUCIDAD: Duration = Duration::from_secs(7 * 24 * 3600);
type Mapa = HashMap<String, String>;
static MAPA: OnceLock<Mutex<Option<(SystemTime, Mapa)>>> = OnceLock::new();

fn celda() -> &'static Mutex<Option<(SystemTime, Mapa)>> {
    MAPA.get_or_init(|| Mutex::new(None))
}

async fn hosts_bajar(url: &str) -> Result<String, String> {
    let r = reqwest::get(url).await.map_err(|e| format!("GET {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("HTTP {} en {url}", r.status()));
    }
    r.text().await.map_err(|e| format!("cuerpo {url}: {e}"))
}

fn hosts_parsear(texto: &str) -> Mapa {
    let mut m = Mapa::new();
    for l in texto.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') { continue; }
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
    let cache = Path::new(data_dir).join("i2p_hosts.txt");
    let mut mapa_disco: Option<Mapa> = None;
    if cache.exists() {
        if let Ok(txt) = std::fs::read_to_string(&cache) {
            let m = hosts_parsear(&txt);
            if !m.is_empty() { mapa_disco = Some(m); }
        }
    }
    let mut fresco: Option<Mapa> = None;
    for f in FUENTES {
        match hosts_bajar(f).await {
            Ok(txt) => {
                let m = hosts_parsear(&txt);
                if !m.is_empty() {
                    let _ = std::fs::write(&cache, &txt);
                    fresco = Some(m);
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    match fresco {
        Some(m) => {
            if let Ok(mut g) = celda().lock() {
                *g = Some((SystemTime::now(), m.clone()));
            }
            Ok(m)
        }
        None => mapa_disco.ok_or_else(|| {
            "hosts.txt no disponible: usá xxx.b32.i2p o destino base64".into()
        }),
    }
}

async fn resolver_host(host: &str, data_dir: &str) -> Result<String, String> {
    let h = host.trim().to_ascii_lowercase();
    if !h.ends_with(".i2p") {
        if h.len() > 40 && h.contains('=') { return Ok(h); }
        return Err(format!("{h} no es un destino I2P"));
    }
    if h.ends_with(".b32.i2p") { return Ok(h); }
    let mapa = asegurar_mapa(data_dir).await?;
    mapa.get(&h).cloned().ok_or_else(|| format!("{h} no está en hosts.txt"))
}

// ──────────────────────────── sam ────────────────────────────

async fn sam_linea(s: &mut TcpStream) -> Result<String, String> {
    let mut buf = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        let n = s.read(&mut byte).await.map_err(|e| format!("SAM lectura: {e}"))?;
        if n == 0 { return Err("SAM cerró la conexión".into()); }
        if byte[0] == b'\n' { break; }
        buf.push(byte[0]);
        if buf.len() > 1024 { return Err("SAM respuesta larga".into()); }
    }
    Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
}

async fn sam_paso(s: &mut TcpStream, cmd: &str, ctx: &str) -> Result<(), String> {
    s.write_all(cmd.as_bytes()).await.map_err(|e| format!("SAM escritura: {e}"))?;
    let r = sam_linea(s).await?;
    if !r.contains("RESULT=OK") { return Err(format!("{ctx} falló: {r}")); }
    Ok(())
}

async fn sam_stream(sam_port: u16, dest: &str, session: &str) -> Result<TcpStream, String> {
    let mut s = TcpStream::connect(("127.0.0.1", sam_port)).await
        .map_err(|e| format!("SAM 127.0.0.1:{sam_port}: {e}"))?;
    sam_paso(&mut s, "HELLO VERSION MIN=3.0 MAX=3.1\n", "HELLO").await?;
    sam_paso(&mut s, &format!("SESSION CREATE STYLE=STREAM ID={session} DESTINATION=TRANSIENT\n"), "SESSION").await?;
    sam_paso(&mut s, &format!("STREAM CONNECT ID={session} DESTINATION={dest} SILENT=false\n"), "CONNECT").await?;
    Ok(s)
}

// ──────────────────────────── tunnel ────────────────────────────

const CAP_TEXTO: usize = 2 * 1024 * 1024;

fn partir_url(url: &str) -> Result<(String, String), String> {
    let u = reqwest::Url::parse(url).map_err(|e| format!("URL inválida: {e}"))?;
    let host = u.host_str().ok_or("URL sin host")?.to_string();
    if !host.ends_with(".i2p") { return Err(format!("{host} no es .i2p")); }
    let recurso = match (u.path(), u.query()) {
        ("", None) => "/".to_string(),
        ("", Some(q)) => format!("/?{q}"),
        (p, None) => p.to_string(),
        (p, Some(q)) => format!("{p}?{q}"),
    };
    Ok((host, recurso))
}

async fn abrir_tunel(url: &str) -> Result<(TcpStream, String), String> {
    let port = sam_port_get();
    if !router_vivo() || port == 0 { return Err("I2P no está corriendo".into()); }
    let (host, recurso) = partir_url(url)?;
    let dest = resolver_host(&host, &data_dir_get()).await?;
    let session = format!("m{}", SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64 + d.as_secs() % 1_000_000_000).unwrap_or(7) % 1_000_000_000);
    let s = sam_stream(port, &dest, &session).await?;
    Ok((s, recurso))
}

async fn pedir_texto(url: &str) -> Result<(u16, Vec<u8>), String> {
    let (mut s, recurso) = abrir_tunel(url).await?;
    let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default();
    let req = format!("GET {recurso} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: test_i2p\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.map_err(|e| format!("enviar: {e}"))?;
    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    let mut tmp = [0u8; 16384];
    loop {
        let n = s.read(&mut tmp).await.map_err(|e| format!("lectura: {e}"))?;
        if n == 0 { break; }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > CAP_TEXTO { break; }
    }
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or("sin headers")?;
    let status_line = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| format!("status: {status_line}"))?;
    Ok((status, buf[sep + 4..].to_vec()))
}

async fn pedir_archivo(url: &str, dest_path: &str) -> Result<u64, String> {
    use std::io::Write;
    let (mut s, recurso) = abrir_tunel(url).await?;
    let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default();
    let req = format!("GET {recurso} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: test_i2p\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.map_err(|e| format!("enviar: {e}"))?;
    let mut f = std::fs::File::create(dest_path).map_err(|e| format!("crear {dest_path}: {e}"))?;
    let mut hdr: Vec<u8> = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    loop {
        let n = s.read(&mut byte).await.map_err(|e| format!("headers: {e}"))?;
        if n == 0 { return Err("cortado en headers".into()); }
        hdr.push(byte[0]);
        if hdr.ends_with(b"\r\n\r\n") { break; }
        if hdr.len() > 64 * 1024 { return Err("headers gigantes".into()); }
    }
    let head = String::from_utf8_lossy(&hdr);
    let status: u16 = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).ok_or_else(|| format!("status: {head}"))?;
    if !(200..300).contains(&status) { return Err(format!("HTTP {status} en {url}")); }
    let mut total: u64 = 0;
    let mut tmp = vec![0u8; 32 * 1024];
    loop {
        let n = s.read(&mut tmp).await.map_err(|e| format!("cuerpo: {e}"))?;
        if n == 0 { break; }
        f.write_all(&tmp[..n]).map_err(|e| format!("escribir: {e}"))?;
        total += n as u64;
    }
    f.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(total)
}

// ──────────────────────────── router ────────────────────────────

async fn arrancar(base: std::path::PathBuf, sam_port: u16, transport_port: u16, publicar: bool) -> Result<String, String> {
    let storage = Storage::new::<Runtime>(Some(base)).await.map_err(|e| format!("storage: {e}"))?;
    let StorageBundle { ntcp2_iv, ntcp2_key, profiles, router_info, mut routers, signing_key, static_key, ssu2_intro_key, ssu2_static_key } = storage.load().await;
    if routers.is_empty() {
        match Reseeder::reseed::<Runtime>(None, false).await {
            Ok(nuevos) => {
                for info in nuevos {
                    storage.store_router_info(info.name.to_string(), info.router_info.clone()).await.map_err(|e| format!("guardar: {e}"))?;
                    routers.push(info.router_info);
                }
            }
            Err(e) if routers.is_empty() => return Err(format!("reseed falló: {e}")),
            Err(e) => eprintln!("[i2p] reseed falló ({n} routers): {e}", n = routers.len()),
        }
    }
    let config = Config {
        ntcp2: Some(Ntcp2Config { port: transport_port, key: ntcp2_key, iv: ntcp2_iv, publish_ipv4: publicar, publish_ipv6: publicar, ipv4_host: None, ipv6_host: None, ipv4: true, ipv6: true, ml_kem: Some(4), max_connections: None, disable_pq: false }),
        ssu2: Some(Ssu2Config { intro_key: ssu2_intro_key, static_key: ssu2_static_key, ipv4: true, ipv4_host: None, ipv6: true, ipv6_host: None, port: transport_port, publish_ipv4: publicar, publish_ipv6: publicar, ipv4_mtu: None, ipv6_mtu: None, disable_pq: false, ml_kem: Some("4".to_string()), max_connections: None }),
        samv3_config: Some(SamConfig { tcp_port: sam_port, udp_port: sam_port, host: "127.0.0.1".to_string() }),
        routers, profiles, router_info,
        static_key: Some(static_key), signing_key: Some(signing_key),
        transit: Some(TransitConfig { max_tunnels: Some(1000) }),
        ..Default::default()
    };
    let (router, _events, router_info) = Router::<Runtime>::new(config, None, Some(Arc::new(storage))).await.map_err(|e| format!("arranque: {e}"))?;
    storage.store_local_router_info(router_info).await.map_err(|e| format!("guardar local: {e}"))?;
    let handle = tokio::spawn(router);
    if let Ok(mut g) = ROUTER_TASK.lock() { *g = Some(handle); }
    estado_set(2);
    Ok(format!("router vivo · SAMv3 127.0.0.1:{sam_port} · transports {transport_port}"))
}

// ──────────────────────────── FRB (API pública) ────────────────────────────

#[flutter_rust_bridge::frb]
pub fn i2p_start(data_dir: String, sam_port: u16, transport_port: u16, publicar: bool) -> Result<String, String> {
    if router_vivo() { return Err("I2P ya está corriendo".into()); }
    estado_set(1);
    sam_port_set(sam_port);
    if let Ok(mut g) = DATA_DIR.lock() { *g = data_dir.clone(); }
    let base = std::path::Path::new(&data_dir).join(".emissary");
    runtime().as_ref().map_err(|e| e.clone())?.block_on(async move { arrancar(base, sam_port, transport_port, publicar).await })
}

#[flutter_rust_bridge::frb]
pub fn i2p_stop() -> Result<(), String> {
    estado_set(0);
    if let Ok(mut g) = ROUTER_TASK.lock() {
        if let Some(h) = g.take() { h.abort(); }
    }
    sam_port_set(0);
    Ok(())
}

#[flutter_rust_bridge::frb]
pub fn i2p_is_running() -> bool {
    router_vivo()
}

#[flutter_rust_bridge::frb]
pub fn i2p_estado() -> String {
    match estado_get() {
        1 => "bootstrapeando/reseeding…".into(),
        2 => "corriendo (túneles construyendo…)".into(),
        3 => "listo".into(),
        _ => "apagado".into(),
    }
}

#[flutter_rust_bridge::frb]
pub fn i2p_sam_port() -> Option<u16> {
    let p = sam_port_get();
    if router_vivo() && p != 0 { Some(p) } else { None }
}

#[flutter_rust_bridge::frb]
pub fn i2p_probe_sam() -> String {
    let port = match i2p_sam_port() {
        Some(p) => p,
        None => return "apagado: sin puerto SAM".into(),
    };
    let r = runtime().as_ref().map_err(|e| e.clone()).and_then(|rt| {
        rt.block_on(async { TcpStream::connect(("127.0.0.1", port)).await.map(|_| ()).map_err(|e| e.to_string()) })
    });
    match r {
        Ok(()) => {
            if estado_get() < 3 { estado_set(3); }
            format!("SAM vivo en 127.0.0.1:{port}")
        }
        Err(e) => format!("SAM no responde ({e})"),
    }
}

#[flutter_rust_bridge::frb]
pub fn i2p_http_get(url: String) -> Result<String, String> {
    let fut = async { tokio::time::timeout(Duration::from_secs(90), pedir_texto(&url)).await.map_err(|_| "timeout túneles".to_string())? };
    let (status, body) = runtime().as_ref().map_err(|e| e.clone())?.block_on(fut)?;
    Ok(format!("HTTP {status}\n\n{}", String::from_utf8_lossy(&body)))
}

#[flutter_rust_bridge::frb]
pub fn i2p_download(url: String, dest_path: String) -> Result<u64, String> {
    let fut = async { tokio::time::timeout(Duration::from_secs(600), pedir_archivo(&url, &dest_path)).await.map_err(|_| "timeout".to_string())? };
    runtime().as_ref().map_err(|e| e.clone())?.block_on(fut)
}
