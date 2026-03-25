//! Integração com systemd: sd_notify e watchdog.
//!
//! Suporta tanto sockets de ficheiro (path normal) como sockets abstractos
//! (prefix `@`), que o systemd usa frequentemente para NOTIFY_SOCKET.

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
    let path_str = path.to_string_lossy();

    if let Some(abstract_name) = path_str.strip_prefix('@') {
        sd_notify_abstract(abstract_name, msg);
    } else {
        // Socket de ficheiro — usar UnixDatagram da stdlib
        if let Ok(sock) = UnixDatagram::unbound() {
            if let Err(e) = sock.send_to(msg.as_bytes(), path_str.as_ref()) {
                log::debug!("sd_notify: falha ao enviar para '{path_str}': {e}");
            }
        }
    }
}

/// Envia para um abstract Unix socket via libc (não suportado pela stdlib).
///
/// # Safety
/// Usa syscalls POSIX diretos (socket, sendto, close). Seguro no contexto de
/// envio de um datagrama sem estado partilhado.
fn sd_notify_abstract(name: &str, msg: &str) {
    use std::mem;

    // Truncar ao máximo do campo sun_path (107 bytes úteis + 1 byte nulo inicial)
    let name_bytes = name.as_bytes();
    if name_bytes.len() > 106 {
        log::debug!("sd_notify: abstract socket name demasiado longo, a ignorar");
        return;
    }

    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            log::debug!(
                "sd_notify: socket() falhou: {}",
                std::io::Error::last_os_error()
            );
            return;
        }

        // Construir sockaddr_un com sun_path[0] = '\0' (abstract socket)
        let mut addr: libc::sockaddr_un = mem::zeroed();
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        // sun_path[0] já é 0 (zeroed); copiar o nome a partir do índice 1
        let dst = &mut addr.sun_path[1..1 + name_bytes.len()];
        for (d, s) in dst.iter_mut().zip(name_bytes.iter()) {
            *d = *s as libc::c_char;
        }

        // addrlen = offset de sun_path + 1 (byte nulo) + comprimento do nome
        let addrlen = mem::offset_of!(libc::sockaddr_un, sun_path) + 1 + name_bytes.len();

        let ret = libc::sendto(
            fd,
            msg.as_ptr() as *const libc::c_void,
            msg.len(),
            libc::MSG_NOSIGNAL,
            &addr as *const libc::sockaddr_un as *const libc::sockaddr,
            addrlen as libc::socklen_t,
        );

        if ret < 0 {
            log::debug!(
                "sd_notify: sendto abstract socket falhou: {}",
                std::io::Error::last_os_error()
            );
        }

        libc::close(fd);
    }
}
