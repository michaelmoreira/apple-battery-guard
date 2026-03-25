# apple-battery-guard

🇵🇹 Português | [🇬🇧 English](README.md)

Daemon inteligente de gestão de bateria para **MacBooks Intel com Linux**.

MacBooks com Linux carregam a bateria sempre até 100%, degradando-a prematuramente. O macOS limita automaticamente a 80% via Apple SMC. Este projeto replica esse comportamento no Linux através de sysfs, sem patches de kernel nem dependências pesadas.

Testado em **MacBook Air 2017 (Intel) com Manjaro**. Funciona em qualquer distro com systemd.

---

## Conteúdo

- [Problema](#problema)
- [Como funciona](#como-funciona)
- [Requisitos](#requisitos)
  - [Verificar suporte do kernel](#verificar-suporte-do-kernel)
- [Módulo de kernel (applesmc-next)](#módulo-de-kernel-applesmc-next)
  - [O que é](#o-que-é)
  - [Setup automático (recomendado)](#setup-automático-recomendado)
  - [Instalação manual](#instalação-manual)
  - [Manter o submodule atualizado](#manter-o-submodule-atualizado)
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
- Kernel ≥ 5.4 com suporte a `charge_control_end_threshold` **ou** módulo `applesmc-next` DKMS (incluído — ver abaixo)
- Rust ≥ 1.70 (só para compilar da fonte)
- `dkms` e headers do kernel (só necessário se for preciso instalar o `applesmc-next`)

### Verificar suporte do kernel

Antes de instalar, verifica se o teu kernel já expõe o ficheiro sysfs necessário:

```bash
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

| Output | Significado |
|---|---|
| Um número (ex: `80`) | O kernel já suporta. Podes ir diretamente para [Instalação](#instalação). |
| `No such file or directory` | O kernel não suporta. Instala o `applesmc-next` primeiro — ver abaixo. |

---

## Módulo de kernel (applesmc-next)

### O que é

`applesmc-next` é um módulo DKMS que aplica patches ao driver `applesmc` para expor o `charge_control_end_threshold` em kernels que não têm suporte nativo (tipicamente kernels < 5.4 ou distribuições com configurações de kernel mais antigas).

Este repositório inclui o `applesmc-next` como **git submodule** em `modules/applesmc-next`, por isso não precisas de o procurar nem descarregar separadamente. Está sincronizado com o projeto upstream e testado contra novas versões de kernel à medida que são lançadas.

> **Upstream:** [github.com/c---/applesmc-next](https://github.com/c---/applesmc-next) — ativamente mantido, última atualização julho 2025 (v0.1.6, compatível com kernel 6.15).

### Setup automático (recomendado)

O script de setup incluído deteta se o suporte já está presente. Se não estiver, instala o módulo via DKMS automaticamente:

```bash
# 1. Clonar o repositório com submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

# 2. Correr o script de setup
bash scripts/setup-kernel-module.sh
```

O script irá:
1. Verificar se `/sys/class/power_supply/BAT0/charge_control_end_threshold` existe
2. Se existir — termina imediatamente, não há nada a fazer
3. Se não existir — pede confirmação e depois:
   - Verifica se o `dkms` está instalado (e indica como instalar se não estiver)
   - Copia o source do módulo de `modules/applesmc-next` para `/usr/src/`
   - Regista e compila o módulo via `dkms install`
   - Carrega o módulo com `modprobe applesmc`
   - Confirma que o ficheiro de threshold está agora disponível

**Dependências necessárias pelo script:**

| Distribuição | Comando |
|---|---|
| Arch / Manjaro | `sudo pacman -S dkms linux-headers` |
| Debian / Ubuntu | `sudo apt install dkms linux-headers-$(uname -r)` |
| Fedora | `sudo dnf install dkms kernel-devel` |

Após instalação, o módulo é gerido pelo DKMS e será **recompilado automaticamente em cada atualização de kernel**.

### Instalação manual

Se preferires instalar o `applesmc-next` sem usar o script:

**Opção A — AUR (Arch / Manjaro apenas):**
```bash
yay -S applesmc-next-dkms
sudo modprobe applesmc
```

**Opção B — A partir do submodule incluído:**
```bash
# Inicializar o submodule se clonaste sem --recurse-submodules
git submodule update --init

# Ler a versão do dkms.conf
VERSION=$(grep "^PACKAGE_VERSION=" modules/applesmc-next/dkms.conf | cut -d= -f2 | tr -d '"')

# Copiar source e instalar
sudo cp -r modules/applesmc-next /usr/src/applesmc-next-${VERSION}
sudo dkms install applesmc-next/${VERSION}
sudo modprobe applesmc

# Verificar
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

**Opção C — Diretamente do upstream:**
```bash
git clone https://github.com/c---/applesmc-next.git
cd applesmc-next
VERSION=$(grep "^PACKAGE_VERSION=" dkms.conf | cut -d= -f2 | tr -d '"')
sudo cp -r . /usr/src/applesmc-next-${VERSION}
sudo dkms install applesmc-next/${VERSION}
sudo modprobe applesmc
```

### Manter o submodule atualizado

O submodule está fixado a um commit específico do `applesmc-next`. Para o atualizar quando sair uma nova versão upstream:

```bash
# Atualizar o submodule para o commit mais recente do upstream
git submodule update --remote modules/applesmc-next

# Rever o que mudou
git diff modules/applesmc-next

# Commit do ponteiro atualizado
git add modules/applesmc-next
git commit -m "chore: update applesmc-next submodule to latest"
```

Se um novo kernel quebrar o módulo, verifica o repositório upstream para fixes e atualiza o submodule.

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

> **Nota:** `applesmc-next-dkms` é listado como dependência opcional. O AUR helper perguntará se queres instalá-lo. Instala se `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` devolver erro.

### Outras distros (compilar da fonte)

```bash
# 1. Instalar Rust (se não tiveres)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clonar com submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

# 3. Verificar suporte do kernel e instalar applesmc-next se necessário
bash scripts/setup-kernel-module.sh

# 4. Compilar
cargo build --release

# 5. Instalar
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 6. Permissão de escrita no sysfs (sem root permanente)
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# 7. Ativar e iniciar o serviço
sudo systemctl enable --now apple-battery-guard
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

### Estrutura do projeto

```
apple-battery-guard/
├── src/
│   ├── main.rs      — Entrypoint: parse de argumentos CLI, despacha subcomandos
│   ├── battery.rs   — Abstração sobre sysfs: detect, status, set_charge_threshold
│   ├── config.rs    — Struct Config + deserialização TOML + validação
│   ├── daemon.rs    — Loop principal, scheduler, Unix socket, signal handling
│   ├── systemd.rs   — sd_notify, watchdog keepalive
│   └── tui.rs       — Dashboard ratatui com gauge, estado e threshold
├── modules/
│   └── applesmc-next/   — git submodule: módulo DKMS para kernels antigos
├── scripts/
│   └── setup-kernel-module.sh  — deteta e instala applesmc-next se necessário
├── config/
│   └── apple-battery-guard.toml
├── systemd/
│   └── apple-battery-guard.service
└── packaging/
    ├── PKGBUILD
    └── apple-battery-guard.spec
```

### Decisões de design

**Sem tokio.** O daemon usa `std::thread` + `std::sync`. O problema não justifica um runtime async — são dois threads: o loop de polling e o servidor de socket.

**Sysfs como interface.** Toda a interação com hardware passa por `/sys/class/power_supply/`. Sem ioctls, sem acesso direto ao SMC, sem código específico de kernel.

**Fallback gracioso.** Erros de I/O no sysfs são logados mas nunca crasham o daemon. Se `charge_control_end_threshold` não existir, o daemon avisa e continua — não impede o serviço de arrancar.

**Unix socket para IPC.** O CLI (`abg status`) comunica com o daemon via socket Unix com um protocolo de linha simples (JSON). Não requer root após setup inicial.

**Módulo de kernel incluído.** O `applesmc-next` está incluído como git submodule em vez de dependência externa. Isto garante reprodutibilidade e permite ao script de setup instalar a versão exata testada sem necessitar de acesso à rede além do clone inicial.

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
# Clonar com submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

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
running 30 tests
test battery::tests::detect_finds_battery ... ok
test battery::tests::detect_ignores_non_battery_entries ... ok
test battery::tests::detect_prefers_bat0_over_bat1 ... ok
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
test config::tests::load_or_default_returns_default_on_invalid_toml ... ok
test config::tests::load_or_default_returns_default_when_missing ... ok
test config::tests::parse_full_toml ... ok
test config::tests::partial_toml_uses_defaults ... ok
test config::tests::save_and_reload_roundtrip ... ok
test config::tests::validation_rejects_empty_socket_path ... ok
test config::tests::validation_rejects_threshold_above_100 ... ok
test config::tests::validation_rejects_zero_interval ... ok
test config::tests::validation_rejects_zero_threshold ... ok
test daemon::tests::effective_threshold_full_charge_day_disabled ... ok
test daemon::tests::effective_threshold_normal ... ok
test daemon::tests::format_status_json_empty_state ... ok
test daemon::tests::format_status_json_escapes_special_chars_in_status ... ok
test daemon::tests::format_status_json_with_data ... ok
test daemon::tests::json_escape_handles_quotes_and_backslashes ... ok

test result: ok. 30 passed; 0 failed; 0 ignored
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

**Preciso de instalar o applesmc-next?**
Só se o teu kernel não expuser `charge_control_end_threshold` nativamente. Corre `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` — se devolver um número, não precisas.

**O applesmc-next vai quebrar após uma atualização de kernel?**
Não. Como está instalado via DKMS, é recompilado automaticamente para cada nova versão de kernel. Se um novo kernel mudar uma API interna que quebre a compilação, atualiza o submodule (`git submodule update --remote modules/applesmc-next`) para obter o fix mais recente do upstream.

**Clonei o repo mas `modules/applesmc-next` está vazio. Porquê?**
Clonaste sem inicializar os submodules. Corre:
```bash
git submodule update --init
```

**Posso ter dois thresholds — um para casa e outro para viagem?**
Ainda não, mas é um roadmap item. Workaround atual: `abg set 90` para viagem e `abg set 80` ao regressar.

---

## Licença

MIT — ver [LICENSE](LICENSE).

---

## Contribuições

Issues e PRs são bem-vindos. Antes de abrir um PR, corre `cargo test && cargo clippy -- -D warnings` e confirma que passam sem erros.
