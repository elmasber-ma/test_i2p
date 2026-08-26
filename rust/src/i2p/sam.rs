//! Cliente SAMv3 mínimo propio (solo STYLE=STREAM): HELLO → SESSION
//! TRANSIENT → STREAM CONNECT sobre TCP plano. Sin dependencias extra.
//!
//! Tras un CONNECT exitoso, el socket queda como túnel crudo bidireccional
//! hacia el servicio destino (eepsite, etc.).
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Lee una línea de respuesta SAM (terminada en \n) del socket.
async fn linea(s: &mut TcpStream) -> Result<String, String> {
    let mut buf = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        let n = s.read(&mut byte).await.map_err(|e| format!("SAM lectura: {e}"))?;
        if n == 0 {
            return Err("SAM cerró la conexión sin respuesta".into());
        }
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 1024 {
            return Err("SAM respuesta demasiado larga".into());
        }
    }
    Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
}

async fn paso(s: &mut TcpStream, comando: &str, esperado: &str) -> Result<(), String> {
    s.write_all(comando.as_bytes())
        .await
        .map_err(|e| format!("SAM escritura: {e}"))?;
    let r = linea(s).await?;
    if !r.contains("RESULT=OK") {
        return Err(format!("{esperado} falló: {r}"));
    }
    Ok(())
}

/// Abre una sesión STREAM transitoria y conecta al destino [dest], que puede
/// ser: destino completo en base64, `xxx.b32.i2p` o nombre ya resuelto por
/// hosts.txt (ver super::hosts). Devuelve el socket en modo túnel crudo.
pub(super) async fn stream_connect(sam_port: u16, dest: &str, session: &str) -> Result<TcpStream, String> {
    let mut s = TcpStream::connect(("127.0.0.1", sam_port))
        .await
        .map_err(|e| format!("conectar a SAM 127.0.0.1:{sam_port}: {e}"))?;

    paso(
        &mut s,
        "HELLO VERSION MIN=3.0 MAX=3.1\n",
        "HELLO",
    )
    .await?;
    paso(
        &mut s,
        &format!("SESSION CREATE STYLE=STREAM ID={session} DESTINATION=TRANSIENT\n"),
        "SESSION",
    )
    .await?;
    paso(
        &mut s,
        &format!("STREAM CONNECT ID={session} DESTINATION={dest} SILENT=false\n"),
        "STREAM CONNECT",
    )
    .await?;

    Ok(s)
}
