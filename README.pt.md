# apple-battery-guard

🇵🇹 Português | [🇬🇧 English](README.md)

[![AUR version](https://img.shields.io/aur/version/apple-battery-guard)](https://aur.archlinux.org/packages/apple-battery-guard)
[![AUR votes](https://img.shields.io/aur/votes/apple-battery-guard)](https://aur.archlinux.org/packages/apple-battery-guard)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Daemon inteligente de gestão de bateria para **MacBooks Intel com Linux**.

MacBooks com Linux carregam a bateria sempre até 100%, degradando-a prematuramente. O macOS limita automaticamente a 80% via Apple SMC. Este projeto replica esse comportamento no Linux através de sysfs, sem patches de kernel nem dependências pesadas.

Testado em **MacBook Air 2017 (Intel) com Manjaro**. Funciona em qualquer distro com systemd.

---

## Quick Start

```bash
# Arch / Manjaro
yay -S apple-battery-guard
sudo systemctl enable --now apple-battery-guard
abg status
```

Se `abg status` mostrar `Threshold: 80%` — está pronto. Se mostrar `Threshold: unsupported`, consulta [Módulo de kernel (applesmc-next)](#módulo-de-kernel-applesmc-next).

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
- [Verificar a instalação](#verificar-a-instalação)
- [Setup completo de gestão de energia](#setup-completo-de-gestão-de-energia)
- [Configuração](#configuração)
- [Utilização](#utilização)
- [Serviço systemd](#serviço-systemd)
- [Resolução de problemas](#resolução-de-problemas)
- [Desinstalar](#desinstalar)
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
┌──────────────────────────────────────────────────┐
│                    abg daemon                     │
│                                                   │
│  ┌──────────┐    ┌──────────┐    ┌─────────────┐  │
│  │ scheduler│───▶│ battery  │───▶│    sysfs    │  │
│  │  (30s)   │    │  module  │    │ /sys/class/ │  │
│  └────▲─────┘    └──────────┘    │ power_supply│  │
│       │                          └─────────────┘  │
│  ┌────┴──────────────────────────────────────┐   │
│  │  udev monitor (NETLINK_KOBJECT_UEVENT)    │   │
│  │  deteta eventos power_supply e aciona     │   │
│  │  reaplicação imediata (suspend/resume,    │   │
│  │  ligação/desligação do carregador)        │   │
│  └────────────────────────────────────────────┘   │
│                                                   │
│  ┌──────────────────────────────────────────┐    │
│  │          Unix socket server              │    │
│  │  /run/apple-battery-guard/daemon.sock    │    │
│  └──────────────────────────────────────────┘    │
└──────────────────┬───────────────────────────────┘
                   │ sd_notify / watchdog
              ┌────▼─────┐
              │ systemd   │
              └──────────┘
```

1. O daemon arranca e **aplica imediatamente** o threshold configurado.
2. O **monitor udev** escuta eventos `power_supply` via netlink kernel (NETLINK_KOBJECT_UEVENT). Quando a máquina acorda do suspend, ou o carregador é ligado/desligado, o threshold é reaplicado **imediatamente** — sem esperar o próximo ciclo de polling.
3. A cada 30 segundos (configurável) verifica e reaplica o threshold como proteção adicional.
4. Comunica com o systemd via `sd_notify` (Type=notify + watchdog).
5. Expõe o estado atual via **Unix socket** — o CLI (`abg status`) lê daqui sem precisar de root. Fallback para sysfs direto se o daemon não estiver ativo.
6. Suporta **"full charge day"**: um dia por semana carrega a 100% para calibração.

---

## Requisitos

### Hardware
- MacBook com chip Intel (Air, Pro, Mini — qualquer modelo Intel)
- Testado em MacBook Air 2017

> **Apple Silicon (M1/M2/M3) não é suportado.** Estes chips têm uma arquitetura de gestão de energia completamente diferente que não usa o driver `applesmc`.

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

| Output | Significado | Ação |
|---|---|---|
| Um número (ex: `80`) | Suporte nativo presente | Avança para [Instalação](#instalação) |
| `No such file or directory` | Sem suporte nativo | Instala o `applesmc-next` primeiro — ver abaixo |
| `Permission denied` | Ficheiro existe mas não é gravável | Configura a regra udev — ver [Instalação](#instalação) passo 6 |

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

**Instala dkms e headers do kernel primeiro:**

| Distribuição | Comando |
|---|---|
| Arch / Manjaro | `sudo pacman -S dkms linux-headers` |
| Debian / Ubuntu | `sudo apt install dkms linux-headers-$(uname -r)` |
| Fedora | `sudo dnf install dkms kernel-devel` |

Após instalação, o módulo é gerido pelo DKMS e será **recompilado automaticamente em cada atualização de kernel** — sem intervenção manual necessária.

### Instalação manual

Se preferires instalar o `applesmc-next` sem usar o script:

**Opção A — AUR (Arch / Manjaro apenas):**
```bash
yay -S applesmc-next-dkms
sudo modprobe applesmc

# Verificar
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
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

Após a instalação, aparece uma mensagem post-install com os dois passos de configuração:

```
╔══════════════════════════════════════════════════════════════╗
║              apple-battery-guard instalado                   ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  1. Ativar o serviço de threshold de bateria:               ║
║     sudo systemctl enable --now apple-battery-guard         ║
║                                                              ║
║  2. (Opcional) Setup completo de gestão de energia:         ║
║     Instala auto-cpufreq + mbpfan + tlp com configurações   ║
║     otimizadas para MacBook Intel.                          ║
║                                                              ║
║     sudo apple-battery-guard-setup-power                    ║
║                                                              ║
╚══════════════════════════════════════════════════════════════╝
```

O pacote AUR instala:

| Caminho | Descrição |
|---|---|
| `/usr/bin/abg` | Binário CLI principal |
| `/usr/bin/apple-battery-guard-setup-power` | Script opcional de setup de gestão de energia |
| `/etc/apple-battery-guard/apple-battery-guard.toml` | Ficheiro de configuração principal |
| `/usr/share/apple-battery-guard/auto-cpufreq.conf` | Config de frequência CPU para auto-cpufreq |
| `/usr/share/apple-battery-guard/mbpfan.conf` | Config de controlo de ventoinha para mbpfan |
| `/usr/share/apple-battery-guard/tlp-macbook.conf` | Config de poupança de energia para tlp |
| `/usr/lib/systemd/system/apple-battery-guard.service` | Unidade de serviço systemd |

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

# 5. Instalar binário, config e serviço systemd
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 6. Permissão de escrita no sysfs (elimina a necessidade de root permanente)
#    Esta regra udev executa chmod no ficheiro threshold sempre que o dispositivo
#    de bateria é adicionado (boot, resume de suspend), tornando-o gravável por
#    qualquer utilizador — o daemon nunca precisa de correr como root.
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# 7. Ativar e iniciar o serviço
sudo systemctl enable --now apple-battery-guard
```

---

## Verificar a instalação

Após instalar, confirma que tudo está a funcionar:

```bash
# 1. Verificar que o serviço está a correr
systemctl status apple-battery-guard
# Esperado: Active: active (running)

# 2. Verificar que o threshold foi aplicado
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
# Esperado: 80

# 3. Verificar o estado via CLI
abg status
# Esperado:
# Battery:   75%
# Status:    Discharging
# Threshold: 80%

# 4. Ver os logs da sequência de arranque
journalctl -u apple-battery-guard -n 20
# Esperado:
# apple-battery-guard[...]: battery detected: BAT0
# apple-battery-guard[...]: threshold applied: 80%
# apple-battery-guard[...]: ready
```

Se `Threshold` aparecer como `unsupported` no `abg status`, o ficheiro sysfs `charge_control_end_threshold` não está disponível. Consulta [Módulo de kernel (applesmc-next)](#módulo-de-kernel-applesmc-next).

---

## Setup completo de gestão de energia

O `apple-battery-guard` gere o threshold de carga. Para uma experiência de gestão de energia completa semelhante ao macOS no teu MacBook Intel, o projeto inclui também configurações otimizadas para três ferramentas complementares.

### O que cada ferramenta faz

| Ferramenta | Responsabilidade | Risco de conflito |
|---|---|---|
| **apple-battery-guard** | Threshold de carga da bateria (80%) | — |
| **auto-cpufreq** | Governor CPU, freq scaling, turbo | Desativar `power-profiles-daemon` |
| **mbpfan** | Controlo da velocidade da ventoinha via applesmc | Nenhum |
| **tlp** | Disco, USB, WiFi, áudio, PCIe power saving | NÃO definir settings de CPU nem de bateria |

> **Importante:** As configurações incluídas estão pré-configuradas para evitar conflitos. As definições de governor CPU e de threshold de bateria do TLP estão intencionalmente comentadas — são geridas pelo `auto-cpufreq` e pelo `apple-battery-guard` respetivamente.

### Setup automático (recomendado)

**Se instalaste via AUR** (o script já está no teu sistema):
```bash
sudo apple-battery-guard-setup-power
```

**Se estás a correr a partir do repositório:**
```bash
sudo bash scripts/setup-power.sh
```

O script:
1. Deteta o teu gestor de pacotes (pacman, apt, dnf)
2. Instala auto-cpufreq, mbpfan, tlp se não estiverem presentes
3. Aplica as configurações otimizadas para MacBook incluídas
4. Desativa o `power-profiles-daemon` se estiver ativo (conflito com auto-cpufreq)
5. Ativa os três serviços
6. Verifica que cada serviço está a correr e mostra o estado atual da bateria

### Setup manual

**1. auto-cpufreq** — Gestão de frequência do CPU

```bash
# Instalar
yay -S auto-cpufreq

# Aplicar config (otimizada para MacBook Air 2017, i5-5350U)
sudo cp config/auto-cpufreq.conf /etc/auto-cpufreq.conf

# Instalar como serviço (desativa power-profiles-daemon automaticamente)
sudo auto-cpufreq --install

# Verificar
auto-cpufreq --stats
```

**2. mbpfan** — Controlo da ventoinha

```bash
# Instalar
yay -S mbpfan

# Carregar módulos necessários no boot
sudo tee /etc/modules-load.d/mbpfan.conf << 'EOF'
coretemp
applesmc
EOF

# Aplicar config
sudo cp config/mbpfan.conf /etc/mbpfan.conf

# Ativar serviço
sudo systemctl enable --now mbpfan
```

**3. tlp** — Poupança de energia do sistema

```bash
# Instalar
sudo pacman -S tlp

# Aplicar config específica para MacBook
sudo cp config/tlp-macbook.conf /etc/tlp.d/10-macbook.conf

# Ativar e aplicar
sudo systemctl enable --now tlp
sudo tlp start
```

### Verificar todos os serviços

```bash
for svc in apple-battery-guard auto-cpufreq mbpfan tlp; do
    systemctl is-active --quiet $svc && echo "✓ $svc" || echo "✗ $svc"
done
```

### O que isto consegue vs macOS

| Feature | macOS | Linux (com setup completo) |
|---|---|---|
| Threshold de carga | ✅ 80% | ✅ 80% (apple-battery-guard) |
| CPU freq scaling | ✅ automático | ✅ automático (auto-cpufreq) |
| Controlo de ventoinha | ✅ nativo | ✅ configurado (mbpfan) |
| USB autosuspend | ✅ nativo | ✅ ativado (tlp) |
| WiFi power save | ✅ nativo | ✅ em bateria (tlp) |
| Áudio power save | ✅ nativo | ✅ em bateria (tlp) |
| PCIe ASPM | ✅ nativo | ✅ powersupersave em bateria (tlp) |
| Disco power save | ✅ nativo | ✅ configurado (tlp) |
| App Nap / throttling de processos | ✅ nativo | ❌ não disponível no Linux |
| Hibernação profunda | ✅ nativo | ⚠️ depende do kernel/distro |

Na prática, este setup recupera **60–70% da diferença de autonomia** entre macOS e Linux em MacBooks Intel.

### Ficheiros de configuração incluídos

Todos os ficheiros estão pré-ajustados para o **MacBook Air 2017 (i5-5350U, MacBookAir7,2)** e instalados em `/usr/share/apple-battery-guard/` pelo pacote AUR.

**`auto-cpufreq.conf`** — Escalonamento de frequência do CPU

```ini
[charger]
governor = performance
scaling_max_freq = 2900000   # turbo (2,9 GHz)
turbo = auto

[battery]
governor = powersave
scaling_max_freq = 1800000   # frequência base (1,8 GHz)
turbo = auto
```

**`mbpfan.conf`** — Controlo da ventoinha

```ini
min_fan1_speed = 1200        # RPM — mínimo do hardware
max_fan1_speed = 6500        # RPM — máximo do hardware
low_temp = 55                # °C — ventoinha fica no mínimo abaixo disto
high_temp = 65               # °C — ventoinha começa a subir aqui
max_temp = 85                # °C — ventoinha vai ao máximo imediatamente
polling_interval = 5         # segundos
```

**`tlp-macbook.conf`** — Poupança de energia do sistema (drop-in em `/etc/tlp.d/10-macbook.conf`)

Cobre: timers de disco em idle, SATA link power, AHCI runtime PM, PCIe ASPM (`powersupersave` em bateria), WiFi power save (só em bateria), áudio power save (só em bateria), USB autosuspend, NMI watchdog desativado.

Governor CPU e thresholds de bateria estão **intencionalmente comentados** — geridos pelo auto-cpufreq e pelo apple-battery-guard respetivamente.

---

## Configuração

Ficheiro: `/etc/apple-battery-guard/apple-battery-guard.toml`

```toml
[battery]
# Threshold de fim de carga em condições normais (1–100).
# 80% é o valor recomendado para maximizar a longevidade da bateria.
charge_end_threshold = 80

[daemon]
# Com que frequência o daemon verifica e reaplica o threshold, em segundos.
# Valores mais baixos reagem mais rapidamente a eventos de resume-from-suspend.
# Padrão: 30
interval_secs = 30

# Caminho para o Unix socket usado para comunicação com o CLI.
# O CLI (abg status, abg config) lê deste socket sem precisar de root.
socket_path = "/run/apple-battery-guard/daemon.sock"

[full_charge]
# "Full charge day": no dia da semana configurado, o threshold é elevado para
# 100% para a bateria carregar completamente. Útil para calibração (mantém o
# indicador de carga preciso) e para dias em que precisas de autonomia máxima
# longe de uma tomada.
#
# O threshold volta automaticamente ao valor normal no dia seguinte.
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

### Aplicar alterações de configuração

O daemon lê o ficheiro de configuração no arranque. Após editá-lo, reinicia o serviço:

```bash
sudo systemctl restart apple-battery-guard
abg status  # confirma que o novo threshold está ativo
```

---

## Utilização

```bash
# Ver estado atual da bateria
# Lê do socket do daemon se estiver ativo; fallback para sysfs direto
abg status

# Exemplo de output (daemon ativo):
# Bateria:   75%
# Estado:    Discharging
# Threshold: 80%

# Exemplo de output (daemon inativo, leitura direta do sysfs):
# Bateria:   75%
# Estado:    Discharging
# Threshold: 80%  (ou "não suportado pelo kernel" se applesmc não estiver carregado)

# Definir threshold manualmente (ignora o daemon, escreve diretamente no sysfs)
abg set 80

# Ver a configuração efetiva carregada pelo daemon
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
# Ativar no boot e iniciar imediatamente
sudo systemctl enable --now apple-battery-guard

# Ver estado
systemctl status apple-battery-guard

# Ver logs em tempo real
journalctl -u apple-battery-guard -f

# Ver logs desde o último boot
journalctl -u apple-battery-guard -b

# Reiniciar após alterar configuração
sudo systemctl restart apple-battery-guard

# Parar o daemon (o threshold mantém o último valor definido)
sudo systemctl stop apple-battery-guard

# Desativar no boot
sudo systemctl disable apple-battery-guard
```

O serviço usa `Type=notify` com watchdog — se o daemon travar, o systemd reinicia-o automaticamente ao fim de 90 segundos.

### Como são os logs

```
Mar 25 11:27:31 macbook systemd[1]: Starting Apple Battery Guard...
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg] a iniciar daemon (threshold=80%)
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] socket a escutar em /run/apple-battery-guard/daemon.sock
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold definido para 80%
Mar 25 11:27:31 macbook systemd[1]: Started Apple Battery Guard.
# A cada 30s (threshold já correto — sem escrita no sysfs):
Mar 25 11:28:01 macbook apple-battery-guard[52127]: [DEBUG abg::daemon] bateria: 75% | Discharging | threshold atual: Some(80)
# Quando o threshold precisa de ser reaplicado:
Mar 25 11:28:01 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold definido para 80%
# Após resume de suspend (acionado pelo monitor udev, sem esperar 30s):
Mar 25 14:32:10 macbook apple-battery-guard[52127]: [DEBUG abg::daemon] uevent power_supply: a aplicar threshold imediatamente
Mar 25 14:32:10 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold definido para 80%
# No full charge day:
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold definido para 100%
```

O daemon usa aproximadamente **400 KB de RAM** em estado estável.

---

## Resolução de problemas

### `charge_control_end_threshold` não encontrado

```
abg status → Threshold: unsupported
```

O teu kernel não expõe o ficheiro de threshold. Instala o `applesmc-next`:
```bash
bash scripts/setup-kernel-module.sh
```
Consulta [Módulo de kernel (applesmc-next)](#módulo-de-kernel-applesmc-next) para detalhes.

---

### Permissão negada ao escrever threshold

```
journalctl -u apple-battery-guard → I/O error: Permission denied
```

A regra udev não está ativa. Verifica se existe e recarrega:
```bash
cat /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
# Se o ficheiro não existir, cria-o conforme o passo 6 da Instalação
```

Após recarregar, reinicia o serviço:
```bash
sudo systemctl restart apple-battery-guard
```

---

### O daemon falha ao arrancar

```
systemctl status apple-battery-guard → Failed
```

Verifica os logs completos:
```bash
journalctl -u apple-battery-guard -n 50 --no-pager
```

Causas comuns:
- Erro de sintaxe no ficheiro de configuração — valida com: `abg --config /etc/apple-battery-guard/apple-battery-guard.toml config`
- Bateria não encontrada — verifica `ls /sys/class/power_supply/`
- Diretório do socket não existe — cria-o: `sudo mkdir -p /run/apple-battery-guard`

---

### O threshold volta a 100% após suspend/resume

Pode acontecer se outro programa (ex: `tlp`, `auto-cpufreq`) sobrescrever o threshold no resume. Verifica conflitos:
```bash
systemctl list-units | grep -E "tlp|cpufreq|battery"
```

Se o `tlp` estiver instalado, adiciona a `/etc/tlp.conf`:
```
START_CHARGE_THRESH_BAT0=0
STOP_CHARGE_THRESH_BAT0=80
```
Ou desativa a gestão de bateria do tlp e deixa o `apple-battery-guard` tratar disso exclusivamente.

---

### Comando `abg` não encontrado ou autocorrigido pelo zsh

Se o zsh perguntar `correct 'abg' to 'ab'?`, responde `n` ou adiciona isto ao `~/.zshrc` para suprimir permanentemente:

```bash
echo "CORRECT_IGNORE='abg'" >> ~/.zshrc
source ~/.zshrc
```

Em alternativa, usa o caminho completo: `/usr/bin/abg status`.

---

### `modules/applesmc-next` está vazio após clonar

Clonaste sem inicializar os submodules:
```bash
git submodule update --init
```

---

### applesmc-next falha a compilar após atualização de kernel

O módulo upstream pode ainda não suportar o novo kernel. Verifica:
```bash
# Ver se foi lançado um fix
git submodule update --remote modules/applesmc-next
git diff modules/applesmc-next
```

Se ainda não existir fix, podes temporariamente carregar a 100% desativando o daemon:
```bash
sudo systemctl stop apple-battery-guard
```

---

## Desinstalar

### AUR (Arch / Manjaro)

```bash
# Remover o pacote (binário, configs, scripts, serviço systemd)
sudo pacman -R apple-battery-guard

# Opcionalmente remover o ficheiro de config do utilizador (o pacman mantém-no por padrão)
sudo rm -rf /etc/apple-battery-guard

# Opcionalmente remover as configs partilhadas (configs do setup-power.sh)
sudo rm -rf /usr/share/apple-battery-guard

# Opcionalmente remover o applesmc-next se o instalaste
sudo pacman -R applesmc-next-dkms

# Opcionalmente remover as ferramentas complementares se as instalaste
sudo pacman -R auto-cpufreq mbpfan tlp
```

### Compilado da fonte

```bash
# Parar e desativar o serviço
sudo systemctl disable --now apple-battery-guard

# Remover ficheiros instalados
sudo rm /usr/local/bin/abg
sudo rm /etc/systemd/system/apple-battery-guard.service
sudo rm -rf /etc/apple-battery-guard
sudo rm -f /etc/udev/rules.d/99-battery-threshold.rules

# Recarregar udev e systemd
sudo udevadm control --reload-rules
sudo systemctl daemon-reload

# Opcionalmente remover o módulo DKMS applesmc-next
VERSION=$(grep "^PACKAGE_VERSION=" modules/applesmc-next/dkms.conf | cut -d= -f2 | tr -d '"')
sudo dkms remove applesmc-next/${VERSION} --all
sudo rm -rf /usr/src/applesmc-next-${VERSION}
```

Após desinstalar, a bateria voltará a carregar até 100% no próximo ciclo de carga completo.

---

## Arquitetura

### Estrutura do projeto

```
apple-battery-guard/
├── src/
│   ├── main.rs        — Entrypoint: parse de argumentos CLI, despacha subcomandos
│   ├── battery.rs     — Abstração sobre sysfs: detect, status, set_charge_threshold
│   ├── config.rs      — Struct Config + deserialização TOML + validação
│   ├── daemon.rs      — Loop principal, scheduler, Unix socket, signal handling
│   ├── systemd.rs     — sd_notify, watchdog keepalive
│   └── tui.rs         — Dashboard ratatui com gauge, estado e threshold
├── modules/
│   └── applesmc-next/ — git submodule: módulo DKMS para kernels antigos
├── scripts/
│   ├── setup-kernel-module.sh  — deteta e instala applesmc-next se necessário
│   └── setup-power.sh          — instala e configura auto-cpufreq + mbpfan + tlp
├── config/
│   ├── apple-battery-guard.toml   — configuração principal do daemon
│   ├── auto-cpufreq.conf          — config de frequência CPU (MacBook Air 2017)
│   ├── mbpfan.conf                — config de controlo de ventoinha (MacBook Air 2017)
│   └── tlp-macbook.conf           — config de poupança de energia (drop-in)
├── systemd/
│   └── apple-battery-guard.service
└── packaging/
    ├── PKGBUILD
    ├── apple-battery-guard.install  — hook post-install (AUR)
    └── apple-battery-guard.spec
```

### Decisões de design

**Sem tokio.** O daemon usa `std::thread` + `std::sync`. O problema não justifica um runtime async — são dois threads: o loop de polling e o servidor de socket.

**Sysfs como interface.** Toda a interação com hardware passa por `/sys/class/power_supply/`. Sem ioctls, sem acesso direto ao SMC, sem código específico de kernel. Isto significa que o daemon funciona com qualquer driver que exponha a interface sysfs padrão — não apenas o `applesmc`.

**Fallback gracioso.** Erros de I/O no sysfs são logados mas nunca crasham o daemon. Se `charge_control_end_threshold` não existir, o daemon avisa e continua — não impede o serviço de arrancar.

**Unix socket para IPC.** O CLI (`abg status`) comunica com o daemon via socket Unix com um protocolo de linha simples (JSON). Não requer root após setup inicial. O socket está em `/run/apple-battery-guard/daemon.sock`.

**Sem root permanente.** A regra udev concede permissão de escrita universal no ficheiro threshold quando o dispositivo de bateria é adicionado pelo kernel. Esta é a abordagem padrão usada por ferramentas como `tlp` e `auto-cpufreq`. O próprio daemon corre como utilizador normal.

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
   ├── set_charge_threshold() ← só se valor derivou  │
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
Não. Apenas o setup inicial da regra udev precisa de root. A regra udev executa `chmod 666` no ficheiro threshold sempre que o dispositivo de bateria é registado pelo kernel (no boot e no resume). Depois disso o daemon corre como utilizador normal.

**Funciona em MacBooks Apple Silicon (M1/M2/M3)?**
Não. Este projeto é específico para MacBooks Intel. Os chips Apple Silicon têm uma arquitetura de gestão de energia completamente diferente que não usa o driver `applesmc`.

**O threshold é persistido entre reboots?**
Sim — o daemon aplica o threshold no arranque. Com o serviço systemd ativo, fica garantido após cada boot.

**Posso usar em portáteis não-Apple com Linux?**
Possivelmente. Se o teu kernel expuser `charge_control_end_threshold` para a tua bateria, o daemon irá usá-lo. O `abg status` dirá se é suportado.

**O que acontece se o daemon crashar?**
O systemd reinicia-o automaticamente (`Restart=on-failure`). O watchdog reinicia-o também se ficar suspenso por mais de 90 segundos sem enviar um keepalive.

**Preciso de instalar o applesmc-next?**
Só se o teu kernel não expuser `charge_control_end_threshold` nativamente. Corre `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` — se devolver um número, não precisas.

**O applesmc-next vai quebrar após uma atualização de kernel?**
Não. Como está instalado via DKMS, é recompilado automaticamente para cada nova versão de kernel. Se um novo kernel mudar uma API interna que quebre a compilação, atualiza o submodule (`git submodule update --remote modules/applesmc-next`) para obter o fix mais recente do upstream.

**Clonei o repo mas `modules/applesmc-next` está vazio. Porquê?**
Clonaste sem inicializar os submodules. Corre:
```bash
git submodule update --init
```

**Conflita com tlp ou auto-cpufreq?**
Pode, se essas ferramentas também gerirem o threshold da bateria. Consulta [O threshold volta a 100% após suspend/resume](#o-threshold-volta-a-100-após-suspendresume) na secção de resolução de problemas.

**O que é exatamente o full charge day?**
Quando `full_charge.enabled = true`, no dia da semana configurado o daemon eleva o threshold para 100% em vez do valor normal. Isto permite que a bateria carregue completamente — útil para calibrar o indicador de carga e para dias de viagem em que precisas de autonomia máxima. No dia seguinte, o threshold volta automaticamente ao valor normal.

**Posso ter dois thresholds — um para casa e outro para viagem?**
Ainda não, mas é um roadmap item. Workaround atual: `abg set 90` para viagem e `abg set 80` ao regressar.

---

## Licença

MIT — ver [LICENSE](LICENSE).

---

## Contribuições

Issues e PRs são bem-vindos. Antes de abrir um PR, corre `cargo test && cargo clippy -- -D warnings` e confirma que passam sem erros.
