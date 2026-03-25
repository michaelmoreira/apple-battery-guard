# apple-battery-guard

🇵🇹 Português | [🇬🇧 English](README.en.md)

Daemon inteligente de gestão de bateria para **MacBooks Intel com Linux**.

MacBooks com Linux carregam a bateria sempre até 100%, degradando-a prematuramente. O macOS limita automaticamente a 80% via Apple SMC. Este projeto replica esse comportamento no Linux através de sysfs, sem patches de kernel nem dependências pesadas.

Testado em **MacBook Air 2017 (Intel) com Manjaro**. Funciona em qualquer distro com systemd.

---

## Conteúdo

- [Problema](#problema)
- [Como funciona](#como-funciona)
- [Requisitos](#requisitos)
- [Instalação](#instalação)
  - [Arch Linux / Manjaro (AUR)](#arch-linux--manjaro-aur)
  - [Outras distros (compilar da fonte)](#outras-distros-compilar-da-fonte)
- [Configuração](#configuração)
- [Utilização](#utilização)
- [Serviço systemd](#serviço-systemd)
- [Arquitetura](#arquitetura)
- [Desenvolvimento](#desenvolvimento)
- [FAQ](#faq)

---

## Problema

| Sistema | Comportamento padrão | Longevidade da bateria |
|---|---|---|
| macOS | Limita carga a 80% via SMC | Preservada |
| Linux (sem este projeto) | Carrega sempre até 100% | Degradação acelerada |
| Linux (com apple-battery-guard) | Limita ao threshold configurado | Preservada |

Carregar constantemente a 100% sujeita as células de lítio a tensão máxima, acelerando a degradação eletroquímica. Manter a carga entre 20–80% pode **duplicar o número de ciclos úteis** da bateria.

O driver `applesmc` em kernels recentes (≥ 5.4) expõe o ficheiro sysfs `charge_control_end_threshold`, mas não existe nenhum daemon que o gira de forma inteligente — até agora.

---

## Como funciona

```
┌─────────────────────────────────────────────┐
│                  abg daemon                  │
│                                              │
│  ┌──────────┐    ┌──────────┐  ┌─────────┐  │
│  │ scheduler│───▶│ battery  │─▶│  sysfs  │  │
│  │  (30s)   │    │  module  │  │ /sys/.. │  │
│  └──────────┘    └──────────┘  └─────────┘  │
│                                              │
│  ┌──────────────────────────────────────┐   │
│  │         Unix socket server            │   │
│  │  /run/apple-battery-guard/daemon.sock │   │
│  └──────────────────────────────────────┘   │
└──────────────────┬──────────────────────────┘
                   │ sd_notify / watchdog
              ┌────▼─────┐
              │ systemd   │
              └──────────┘
```

1. O daemon arranca e **aplica imediatamente** o threshold configurado.
2. A cada 30 segundos (configurável) verifica se o threshold está correto e reaplica se necessário.
3. Comunica com o systemd via `sd_notify` (Type=notify + watchdog).
4. Expõe o estado atual via **Unix socket** — o CLI lê daqui sem precisar de root.
5. Suporta **"full charge day"**: um dia por semana carrega a 100% para calibração.

---

## Requisitos

### Hardware
- MacBook com chip Intel (Air, Pro, Mini — qualquer modelo Intel)
- Testado em MacBook Air 2017

### Software
- Linux com systemd
- Kernel ≥ 5.4 com suporte a `charge_control_end_threshold` **ou** módulo `applesmc-next` (DKMS)
- Rust ≥ 1.70 (só para compilar da fonte)

### Verificar suporte do kernel

```bash
# Se devolver um número (ex: 80), o kernel já suporta
cat /sys/class/power_supply/BAT0/charge_control_end_threshold

# Se "No such file or directory", instala applesmc-next:
# Arch/Manjaro: yay -S applesmc-next-dkms
```

---

## Instalação

### Arch Linux / Manjaro (AUR)

```bash
# Com yay
yay -S apple-battery-guard

# Com paru
paru -S apple-battery-guard

# Manual
git clone https://aur.archlinux.org/apple-battery-guard.git
cd apple-battery-guard
makepkg -si
```

O pacote AUR inclui:
- Binário `abg` em `/usr/bin/`
- Configuração de exemplo em `/etc/apple-battery-guard/apple-battery-guard.toml`
- Serviço systemd em `/usr/lib/systemd/system/apple-battery-guard.service`

### Outras distros (compilar da fonte)

```bash
# 1. Instalar Rust (se não tiveres)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clonar e compilar
git clone https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard
cargo build --release

# 3. Instalar
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 4. Permissão de escrita no sysfs (sem root permanente)
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

---

## Configuração

Ficheiro: `/etc/apple-battery-guard/apple-battery-guard.toml`

```toml
[battery]
# Threshold de fim de carga em condições normais (1–100).
# 80% é o valor recomendado para maximizar a longevidade da bateria.
charge_end_threshold = 80

[daemon]
# Intervalo de verificação em segundos.
interval_secs = 30

# Unix socket para comunicação com o CLI.
socket_path = "/run/apple-battery-guard/daemon.sock"

[full_charge]
# "Full charge day": uma vez por semana carrega até 100%.
# Útil para calibração e dias de uso intensivo fora de casa.
enabled = false

# Dia da semana: sunday, monday, tuesday, wednesday, thursday, friday, saturday
weekday = "sunday"
```

### Opções de threshold recomendadas

| Uso | Threshold | Notas |
|---|---|---|
| Uso sedentário (sempre ligado) | 60–70% | Máxima longevidade |
| Uso misto | 80% | Recomendado (padrão) |
| Uso móvel frequente | 90% | Mais autonomia, menos durabilidade |
| Calibração / viagem | 100% | Temporário via full_charge_day |

---

## Utilização

```bash
# Ver estado atual da bateria
abg status

# Exemplo de output:
# Bateria:   75%
# Estado:    Discharging
# Threshold: 80%

# Definir threshold manualmente (requer permissão no sysfs)
abg set 80

# Ver configuração efetiva
abg config

# Dashboard TUI interativo
abg tui

# Iniciar daemon em foreground (normalmente feito pelo systemd)
abg daemon

# Usar ficheiro de configuração alternativo
abg --config ~/.config/abg.toml status
```

### Dashboard TUI

O comando `abg tui` abre um dashboard interativo no terminal:

```
╔══════════════════════════════════════╗
║         apple-battery-guard          ║
╠══════════════════════════════════════╣
║  Carga                               ║
║  ████████████████████░░░░  75%       ║
╠══════════════════════════════════════╣
║  Estado: Discharging  Threshold: 80% ║
╚══════════════════════════════════════╝
  q / Esc: sair
```

Atualiza a cada 5 segundos. Sair com `q` ou `Esc`.

---

## Serviço systemd

```bash
# Ativar e iniciar
sudo systemctl enable --now apple-battery-guard

# Ver estado
systemctl status apple-battery-guard

# Ver logs
journalctl -u apple-battery-guard -f

# Reiniciar após alterar configuração
sudo systemctl restart apple-battery-guard
```

O serviço usa `Type=notify` com watchdog — se o daemon travar, o systemd reinicia-o automaticamente ao fim de 90 segundos.

---

## Arquitetura

O projeto está organizado em módulos com responsabilidades bem definidas:

```
src/
├── main.rs      — Entrypoint: parse de argumentos CLI, despacha subcomandos
├── battery.rs   — Abstração sobre sysfs: detect, status, set_charge_threshold
├── config.rs    — Struct Config + deserialização TOML + validação
├── daemon.rs    — Loop principal, scheduler, Unix socket, signal handling
├── systemd.rs   — sd_notify, watchdog keepalive
└── tui.rs       — Dashboard ratatui com gauge, estado e threshold
```

### Decisões de design

**Sem tokio.** O daemon usa `std::thread` + `std::sync`. O problema não justifica um runtime async — são dois threads: o loop de polling e o servidor de socket.

**Sysfs como interface.** Toda a interação com hardware passa por `/sys/class/power_supply/`. Sem ioctls, sem acesso direto ao SMC, sem código específico de kernel.

**Fallback gracioso.** Erros de I/O no sysfs são logados mas nunca crasham o daemon. Se `charge_control_end_threshold` não existir, o daemon avisa e continua.

**Unix socket para IPC.** O CLI (`abg status`) comunica com o daemon via socket Unix com um protocolo de linha simples (JSON). Não requer root após setup inicial.

### Fluxo do daemon

```
arranque
   │
   ▼
Battery::detect()          ← escaneia /sys/class/power_supply/BAT*
   │
   ▼
apply_threshold()          ← escreve charge_control_end_threshold
   │
   ▼
systemd::notify_ready()    ← READY=1
   │
   ▼
loop (cada 30s) ───────────────────────────────────┐
   │                                                │
   ├── battery.status()    ← lê capacity + status   │
   ├── effective_threshold() ← normal ou 100% (FCD) │
   ├── set_charge_threshold() ← só se necessário     │
   └── systemd::notify_watchdog() ← WATCHDOG=1 ─────┘
```

---

## Desenvolvimento

```bash
# Compilar
cargo build

# Correr todos os testes (sem hardware necessário)
cargo test

# Correr testes de um módulo específico
cargo test battery
cargo test config

# Linting (zero warnings tolerados)
cargo clippy -- -D warnings

# Formatação
cargo fmt --check
cargo fmt
```

### Testes

Os testes são 100% unitários e não requerem hardware. Simulam o sysfs com ficheiros temporários via `tempfile`:

```bash
$ cargo test
running 21 tests
test battery::tests::detect_finds_battery ... ok
test battery::tests::detect_ignores_non_battery_entries ... ok
test battery::tests::set_threshold_fails_without_sysfs_file ... ok
test battery::tests::set_threshold_rejects_above_100 ... ok
test battery::tests::set_threshold_rejects_zero ... ok
test battery::tests::set_threshold_writes_value ... ok
test battery::tests::status_reads_all_fields ... ok
test battery::tests::status_without_threshold_support ... ok
test battery::tests::supports_threshold_false_when_file_missing ... ok
test battery::tests::supports_threshold_true_when_file_exists ... ok
test config::tests::defaults_are_sane ... ok
test config::tests::empty_toml_uses_all_defaults ... ok
test config::tests::invalid_toml_returns_error ... ok
test config::tests::load_missing_file_returns_error ... ok
test config::tests::load_or_default_returns_default_when_missing ... ok
test config::tests::parse_full_toml ... ok
test config::tests::partial_toml_uses_defaults ... ok
test config::tests::save_and_reload_roundtrip ... ok
test config::tests::validation_rejects_threshold_above_100 ... ok
test config::tests::validation_rejects_zero_interval ... ok
test config::tests::validation_rejects_zero_threshold ... ok

test result: ok. 21 passed; 0 failed; 0 ignored
```

### Adicionar suporte a kernels antigos (applesmc-next)

Se `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` devolver erro:

```bash
# Arch / Manjaro
yay -S applesmc-next-dkms
sudo modprobe applesmc

# Verificar após instalar
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

---

## FAQ

**O daemon requer root permanente?**
Não. Apenas o setup inicial da regra udev precisa de root. Depois disso o daemon corre como utilizador normal com acesso ao sysfs via a regra udev.

**Funciona em MacBooks Apple Silicon (M1/M2/M3)?**
Não. Este projeto é específico para MacBooks Intel. Os chips Apple Silicon têm uma arquitetura de gestão de energia completamente diferente.

**O threshold é persistido entre reboots?**
Sim — o daemon aplica o threshold no arranque. Com o serviço systemd ativo, fica garantido após cada boot.

**Posso usar em portáteis não-Apple com Linux?**
Possivelmente, se o teu kernel expuser `charge_control_end_threshold` para a tua bateria. O `abg status` dirá se é suportado.

**O que acontece se o daemon crashar?**
O systemd reinicia-o automaticamente (`Restart=on-failure`). O watchdog reinicia-o se ficar suspenso por mais de 90 segundos.

**Posso ter dois thresholds — um para casa e outro para viagem?**
Ainda não, mas é um roadmap item. Workaround atual: `abg set 90` para viagem e `abg set 80` ao regressar.

---

## Licença

MIT — ver [LICENSE](LICENSE).

---

## Contribuições

Issues e PRs são bem-vindos. Antes de abrir um PR, corre `cargo test && cargo clippy -- -D warnings` e confirma que passam sem erros.
