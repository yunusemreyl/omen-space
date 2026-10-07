<div align="center">
  <img src="images/omenspace.png" alt="Logo de OMEN Space" width="120" />

  # OMEN Space

  **El centro de control para Linux, ligero y completo, para HP Omen, Victus y Transcend.**  
  *Escrito íntegramente en Rust para un rendimiento nativo con GTK4 y sin sobrecarga.*

  [![Version](https://img.shields.io/badge/Release-v2.0.8-red.svg?style=flat-square)](https://github.com/yunusemreyl/omen-space/releases)
  [![License](https://img.shields.io/badge/License-GPL%203.0-green.svg?style=flat-square)](LICENSE)
  [![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg?style=flat-square)]()
  [![Built with Rust](https://img.shields.io/badge/Language-Rust-orange.svg?style=flat-square)]()

  [English](README.md) · **Español**
</div>

---

## ✨ Características

OMEN Space ofrece todo lo necesario para aprovechar al máximo tu portátil en Linux, sin añadidos innecesarios.

- 🎛️ **Control de ventiladores y temperatura:** crea curvas de ventilador personalizadas para un funcionamiento casi silencioso sin limitación térmica. Incluye un **modo de limpieza de ventiladores**.
- 🎮 **Superposición HUD rápida (Shift+F2):** HUD flotante GTK4 sin latencia dentro del juego para cambiar al instante los modos de ventilador y energía sin perder la concentración.
- ⚡ **Perfiles de rendimiento:** cambia entre los modos ACPI/WMI `power-saver`, `balanced` y `performance`.
- 🚀 **Ryzen SMU y undervolt:** undervolt directo por MSR, control del desfase TCC, límites de TGP de la GPU y ajuste del SMU de AMD Ryzen.
- 🎮 **Interruptor MUX:** cambio nativo de Optimus / enrutamiento de la dGPU para el máximo rendimiento en juegos.
- 🌈 **RGB Studio:** iluminación de teclado de 4 zonas o por tecla, con efectos de onda, respiración, ciclo y estático.
- 🔄 **Comprobador inteligente de BIOS:** consulta los servidores de HP para buscar el firmware más reciente de tu placa.

---

## ⚡ Instalación rápida

La forma más sencilla de instalar OMEN Space es con el instalador web de una línea. Detecta tu distribución, gestiona las dependencias y lo compila todo automáticamente.

```bash
curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash
```

> **Consejo:** para instalar directamente la versión canary (lo más reciente), añade `--canary`:  
> `curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash -s -- --canary`

📖 **Guía completa en español** (requisitos, dependencias, solución de problemas): [docs/INSTALACION.es.md](docs/INSTALACION.es.md)

<details>
<summary><b>📦 Otros métodos de instalación (manual, AUR, NixOS)</b></summary>

**Con Git (Ubuntu/Debian, Fedora, Arch, openSUSE):**
```bash
git clone https://github.com/yunusemreyl/omen-space.git
cd omen-space
sudo ./setup.sh install
```

**Arch Linux (PKGBUILD):**
```bash
git clone https://github.com/yunusemreyl/omen-space.git
cd omen-space
makepkg -si
```

**NixOS (Flakes):**
```bash
nix profile install github:yunusemreyl/omen-space
```

**Desinstalar:**
Si instalaste con el script de instalación rápida o con `setup.sh`:
```bash
cd omen-space
sudo ./setup.sh uninstall
```
*(Si borraste la carpeta, ejecuta de nuevo `git clone https://github.com/yunusemreyl/omen-space.git` antes).*
</details>

---

## 📸 Capturas de pantalla

<p align="center">
  <img src="images/perf.png" width="48%" alt="Rendimiento y editor de curva de ventiladores" />
  <img src="images/appprofiles.png" width="48%" alt="Perfiles por aplicación" />
</p>
<p align="center">
  <img src="images/keyboard.png" width="48%" alt="Ajustes RGB" />
  <img src="images/undervolt.png" width="48%" alt="Undervolt en Ryzen" />
</p>

<details>
<summary><b>🔍 Ver más capturas (MUX, diagnóstico, ajustes, actualizador, CLI)</b></summary>
<br>
<p align="center">
  <img src="images/mux.png" width="48%" alt="Interruptor MUX" />
  <img src="images/diagnostic.png" width="48%" alt="Diagnóstico" />
</p>
<p align="center">
  <img src="images/settings.png" width="48%" alt="Ajustes" />
  <img src="images/updater.png" width="48%" alt="Actualizador" />
</p>
<p align="center">
  <img src="images/cli.png" width="48%" alt="CLI" />
</p>
</details>

---

## 🏗️ Arquitectura

OMEN Space es una reescritura completa del antiguo *OmenCtl* en Python, ahora en **Rust**, con una huella de ~3 MB, menos de 5 MB de RAM y respuesta instantánea.

- **`omen-space-daemon`**: el backend. Se ejecuta como servicio de systemd (root) y gestiona las interacciones con WMI, ACPI, Sysfs y MSR a través de D-Bus.
- **`omen-gui`**: un frontend GTK4 + Libadwaita muy rápido, que se ejecuta en espacio de usuario.
- **`omen-tray`**: un applet ligero para el panel del escritorio que permite cambiar rápido de perfil.
- **`omen-cli`**: una interfaz de terminal rápida y programable, con un comando para el HUD (`omen-cli overlay toggle`).
- **`hp-omen-extra`**: el controlador de kernel DKMS que amplía las capacidades estándar del kernel.

---

## 🌐 Idioma de la aplicación

La interfaz está disponible en **inglés, turco y español**. Cambia el idioma en *Ajustes → Apariencia e idioma*. Con la opción automática se usa el idioma del sistema.

¿Quieres añadir otro idioma? Las cadenas están en `src/omen-gui/src/i18n.rs` (una función `translate_*` por idioma). Las normas de contribución están en [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).

---

## 👨‍💻 Créditos y licencia

OMEN Space se distribuye bajo la licencia **GPL-3.0** y lo impulsa una comunidad increíble.

- **[yunusemreyl](https://github.com/yunusemreyl)** - Desarrollador principal
- **[tuxov](https://github.com/tuxov)** - Responsable del módulo de kernel

Gracias a todos los colaboradores: [@aloshy0](https://github.com/aloshy0), [@CodesRahul96](https://github.com/CodesRahul96), [@xcellsior](https://github.com/xcellsior), [@TitoTFP](https://github.com/TitoTFP), [@SafSaf0999](https://github.com/SafSaf0999), [@yijean34-source](https://github.com/yijean34-source), y a los proyectos `omencore` y `omen-rgb-keyboard`.

*Aviso: OMEN Space es un proyecto independiente y NO está afiliado ni respaldado por HP.*
