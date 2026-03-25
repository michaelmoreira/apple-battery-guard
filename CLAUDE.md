# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`apple-battery-guard` — daemon inteligente de gestão de bateria para MacBooks Intel com Linux. Resolve a ausência de charge threshold automático: sem ele, a bateria carrega sempre até 100%, degradando-a prematuramente. O macOS limita a 80% via SMC; este projeto replica esse comportamento no Linux via sysfs.

Testado em MacBook Air 2017 com Manjaro. Compatível com qualquer distro com systemd.

## Stack

| Camada | Tecnologia |
|---|---|
| Daemon / CLI | Rust (sem tokio — threading std apenas) |
| Kernel interface | sysfs `/sys/class/power_supply/BAT0/` |
| Kernel driver | `applesmc` / `applesmc-next` (DKMS) |
| Config | TOML via `serde` |
| TUI | `ratatui` |
| Init | systemd (unit + timer) |
| Packaging | PKGBUILD (AUR) + `.deb` |

## Estrutura

```
apple-battery-guard/
├── src/
│   ├── main.rs       # entrypoint: parse args, despacha para daemon/CLI/TUI
│   ├── battery.rs    # leitura e escrita de sysfs (charge_control_end_threshold, etc.)
│   ├── config.rs     # struct Config + deserialização TOML
│   ├── daemon.rs     # loop principal, scheduler 30s, integração udev
│   ├── tui.rs        # dashboard ratatui
│   └── systemd.rs    # sd_notify, watchdog
├── config/
│   └── apple-battery-guard.toml
├── systemd/
│   └── apple-battery-guard.service
└── packaging/
    ├── PKGBUILD
    └── apple-battery-guard.spec
```

## Comandos

```bash
cargo build                  # build debug
cargo build --release        # build release
cargo test                   # todos os testes
cargo test battery           # testes só de battery.rs
cargo test config            # testes só de config.rs
cargo clippy -- -D warnings  # linting (sem warnings tolerados)
cargo fmt --check            # verificar formatação
```

## Regras de implementação

- **Safe Rust** sempre que possível; `unsafe` requer comentário justificativo
- **Zero dependências desnecessárias** — sem tokio; usar `std::thread` + `std::sync`
- **Sem root em runtime** — setup inicial via polkit; o daemon corre como utilizador
- **Sysfs com fallback gracioso** — erros de I/O nunca devem crashar o daemon
- **Testes obrigatórios** para `battery.rs` e `config.rs` — usar ficheiros temporários para simular sysfs

## Comportamento do daemon

- Polling a cada 30s (configurável em `[daemon] interval_secs`)
- Aplica threshold no arranque e após resume de suspend (evento udev `POWER_SUPPLY_STATUS`)
- "Full charge day": carregar até 100% num dia da semana configurável
- Estado exposto via Unix socket para o CLI consultar sem root
- Integração com systemd: `sd_notify(READY=1)`, watchdog keepalive
