//! HTTP directo sobre SAMv3 (SIN puente local en 127.0.0.1): Rust abre el
//! stream SAM contra el destino y habla HTTP/1.1 por ese socket. GET de
//! texto y descarga streaming a archivo, espejo de tor_http_get/download.
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{hosts, sam, state};

const CAP_TEXTO: usize = 2 * 1024 * 1024;

/// Divide una URL http://host.i2p/ruta?query en (host, recurso).
fn partir_url(url: &str) -> Result<(String, String), String> {
    let u = reqwest::Url::parse(url).map_err(|e| format!("URL inválida: {e}"))?;
    let host = u.host_str().ok_or("URL sin host")?.to_string();
    if !host.ends_with(".i2p") {
        return Err(format!("{host} no es un destino .i2p"));
    }
    let recurso = match (u.path(), u.query()) {
        ("", None) => "/".to_string(),
        ("", Some(q)) => format!("/?{q}"),
        (p, None) => p.to_string(),
        (p, Some(q)) => format!("{p}?{q}"),
    };
    Ok((host, recurso))
}

async fn abrir_tunel(url: &str) -> Result<(tokio::net::TcpStream, String), String> {
    let port = state::sam_port_get();
    if !state::router_vivo() || port == 0 {
        return Err("I2P no está corriendo".into());
    }
    let (host, recurso) = partir_url(url)?;
    let dest = hosts::resolver(&host, &state::data_dir_get()).await?;
    let session = format!(
        "m{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 + d.as_secs() % 1_000_000_000)
            .unwrap_or(7)
            % 1_000_000_000
    );
    let s = sam::stream_connect(port, &dest, &session).await?;
    Ok((s, recurso))
}

async fn pedir_texto(url: &str) -> Result<(u16, Vec<u8>), String> {
    let (mut s, recurso) = abrir_tunel(url)?;
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();
    let req = format!(
        "GET {recurso} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: mimapp\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())
        .await
        .map_err(|e| format!("enviar request: {e}"))?;

    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    let mut tmp = [0u8; 16384];
    loop {
        let n = s.read(&mut tmp).await.map_err(|e| format!("lectura: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > CAP_TEXTO {
            break; // truncado honesto para el panel
        }
    }

    // separar status/cuerpo
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("respuesta sin headers")?;
    let status_line = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("status ilegible: {status_line}"))?;
    Ok((status, buf[sep + 4..].to_vec()))
}

async fn pedir_archivo(url: &str, dest_path: &str) -> Result<u64, String> {
    use std::io::Write;
    let (mut s, recurso) = abrir_tunel(url)?;
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();
    let req = format!(
        "GET {recurso} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: mimapp\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())
        .await
        .map_err(|e| format!("enviar request: {e}"))?;

    let mut f = std::fs::File::create(dest_path)
        .map_err(|e| format!("crear {dest_path}: {e}"))?;

    // leer hasta fin de headers, luego volcar el resto streaming
    let mut hdr: Vec<u8> = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    loop {
        let n = s.read(&mut byte).await.map_err(|e| format!("headers: {e}"))?;
        if n == 0 {
            return Err("conexión cortada durante headers".into());
        }
        hdr.push(byte[0]);
        if hdr.ends_with(b"\r\n\r\n") {
            break;
        }
        if hdr.len() > 64 * 1024 {
            return Err("headers gigantes".into());
        }
    }
    let head = String::from_utf8_lossy(&hdr);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("status ilegible: {head}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status} en {url}"));
    }

    let mut total: u64 = 0;
    let mut tmp = vec![0u8; 32 * 1024];
    loop {
        let n = s.read(&mut tmp).await.map_err(|e| format!("cuerpo: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&tmp[..n])
            .map_err(|e| format!("escribir archivo: {e}"))?;
        total += n as u64;
    }
    f.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(total)
}

/// GET directo por I2P (eepsites): devuelve "HTTP <status>\n\n<cuerpo>".
#[flutter_rust_bridge::frb]
pub fn i2p_http_get(url: String) -> Result<String, String> {
    let fut = async {
        tokio::time::timeout(Duration::from_secs(90), pedir_texto(&url))
            .await
            .map_err(|_| "timeout: los túneles tardan en construirse al inicio".to_string())?
    };
    let (status, body) = state::runtime()
        .as_ref()
        .map_err(|e| e.clone())?
        .block_on(fut)?;
    let texto = String::from_utf8_lossy(&body);
    Ok(format!("HTTP {status}\n\n{texto}"))
}

/// Descarga streaming a archivo por I2P; retorna bytes escritos.
#[flutter_rust_bridge::frb]
pub fn i2p_download(url: String, dest_path: String) -> Result<u64, String> {
    let fut = async {
        tokio::time::timeout(Duration::from_secs(600), pedir_archivo(&url, &dest_path))
            .await
            .map_err(|_| "timeout de descarga".to_string())?
    };
    state::runtime()
        .as_ref()
        .map_err(|e| e.clone())?
        .block_on(fut)
}
