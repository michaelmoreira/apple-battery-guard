//! Integração com systemd: sd_notify e watchdog.
//!
//! Sem dependência externa — comunica com o socket de notify diretamente via libc.

use std::env;
use std::os::unix::net::UnixDatagram;

/// Envia `READY=1` ao systemd (Type=notify).
pub fn notify_ready() {
    sd_notify("READY=1");
}

/// Envia keepalive ao watchdog do systemd.
pub fn notify_watchdog() {
    sd_notify("WATCHDOG=1");
}

/// Envia status legível ao systemd (visível em `systemctl status`).
#[allow(dead_code)]
pub fn notify_status(msg: &str) {
    sd_notify(&format!("STATUS={msg}"));
}

fn sd_notify(msg: &str) {
    let Some(path) = env::var_os("NOTIFY_SOCKET") else {
        return; // não a correr sob systemd
    };
    let path = path.to_string_lossy();
    // O path pode começar com '@' (socket abstracto) — não suportado via UnixDatagram std
    if path.starts_with('@') {
        log::debug!("abstract NOTIFY_SOCKET não suportado, a ignorar");
        return;
    }
    if let Ok(sock) = UnixDatagram::unbound() {
        let _ = sock.send_to(msg.as_bytes(), path.as_ref());
    }
}
