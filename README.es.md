<div align="center">
  <img src="images/omenspace.png" alt="Logo de OMEN Space" width="110" />

  # OMEN Space

  **Centro de control nativo en Linux para portátiles HP OMEN, Victus y Transcend.**  
  Escrito íntegramente en **Rust** con **GTK4 y Libadwaita** para un rendimiento instantáneo y mínimo consumo.

  <p>
    <a href="https://github.com/yunusemreyl/omen-space/releases/latest">
      <img src="https://img.shields.io/badge/Versi%C3%B3n-v2.5.3-e11d48?style=flat-square" alt="Versión v2.5.3" />
    </a>
    <a href="LICENSE">
      <img src="https://img.shields.io/badge/Licencia-GPL--3.0-10b981?style=flat-square" alt="Licencia" />
    </a>
    <img src="https://img.shields.io/badge/Lenguaje-Rust-f97316?style=flat-square" alt="Rust" />
    <img src="https://img.shields.io/badge/Interfaz-GTK4%20%2B%20Libadwaita-3b82f6?style=flat-square" alt="GTK4" />
  </p>

  <p>
    <a href="README.md">English</a> · <b>Español</b>
  </p>

  <br />
  <img src="images/perf.png" width="92%" alt="OMEN Space — Rendimiento y Barra de Telemetría" />
</div>

---

## Descripción general

**OMEN Space** ofrece control total del hardware en Linux mediante un daemon nativo en Rust (~3 MB) y una interfaz moderna en GTK4 + Libadwaita.

| Módulo | Funciones principales |
| :--- | :--- |
| **Rendimiento y Refrigeración** | Perfiles térmicos `Eco`, `Balanced` y `Performance`; modos de ventilador `Auto`, `Max`, curvas independientes para CPU y GPU con botón rápido **Nuevo preajuste**, delegación al **EC**, limpieza de polvo y barra de telemetría inferior fija sin cajas (`CPU`, `GPU`, `RAM`, `NVMe`, `Batería`). |
| **Monitor del Sistema** | Gráficos en tiempo real de 60 s con selector **Potencia / Temp / Carga** para CPU y GPU, barra resumen de potencia y modos activos, uso por disco, panel dedicado de **Batería y energía** y metadatos de firmware (`OS`, `BIOS`, `EC`, `NVIDIA`). |
| **Energía y Batería** | Limitador de carga de batería HP WMI `0x2B` / nativo con histéresis (`stop`/`start`, protección apagado `hold` y bypass único al 100 %), límites Intel RAPL `PL1`/`PL2`, `TCC Offset`, undervolt por MSR (`UV_LOCK`), GPU Dynamic Boost y ajustes AMD Ryzen SMU. |
| **Selector de Gráficos (MUX y PRIME)** | Cambio validado por hardware entre **Hybrid (MSHybrid)**, **Discrete (dGPU)**, **Advanced Optimus** e **Integrated**, enrutamiento de pantallas, estado pasivo `D3cold` y copia rápida de **PRIME Offload**. |
| **RGB Studio** | Iluminación RGB de 4 zonas, zona única (Victus), barra de luz frontal, Desktop RGB y teclado por tecla HID (Darfon `0d62:54bf` / `0d62:30bf` y Primax `0xFF13`) con vista previa sincronizada con el daemon, calibración física y distribución EN/DE. |
| **Control Rápido (`Shift+F2`, configurable)** | Superposición flotante nativa en Libadwaita con atajos de teclado (`1–3`, `Q/W/E`), soporte de teclas en múltiples dispositivos y telemetría en vivo. |
| **Centro de Ajustes Unificado** | Pestañas **General \| Actualizaciones \| Diagnóstico** con volcado completo para GitHub, comprobación OTA + `fwupd` (LVFS) y perfiles automáticos por aplicación. |

---

## Instalación rápida

```bash
curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash -s -- --stable
```

Para instalar directamente desde la rama `main` (canary):

```bash
curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash -s -- --canary
```

---

## Galería

<p align="center">
  <img src="images/monitor.png" width="49%" alt="Monitor del Sistema" />
  <img src="images/undervolt.png" width="49%" alt="Energía y Batería" />
</p>
<p align="center">
  <img src="images/mux.png" width="49%" alt="Selector de Gráficos (MUX)" />
  <img src="images/keyboard.png" width="49%" alt="RGB Studio" />
</p>
<p align="center">
  <img src="images/appprofiles.png" width="49%" alt="Gestor de Perfiles" />
  <img src="images/settings.png" width="49%" alt="Ajustes" />
</p>

<details>
<summary><b>Más vistas (Control Rápido, Actualizaciones, Diagnóstico y CLI)</b></summary>
<br />
<p align="center">
  <img src="images/updater.png" width="49%" alt="Ajustes — Actualizaciones" />
  <img src="images/diagnostic.png" width="49%" alt="Ajustes — Diagnóstico" />
</p>
<p align="center">
  <img src="images/overlay.png" width="46%" alt="OMEN Quick Control" />
  <img src="images/cli.png" width="49%" alt="CLI interactiva" />
</p>
</details>

---

## Novedades en v2.5.3

- **Distribución `TR-Q` y matriz Primax `0xFF13` (`#385`):** Añadida la distribución de teclas turca Q (`TR-Q`) junto a `EN` y `DE` en RGB Studio, mapas físicos de LED `Ralph`/`Cybug` (`0461:4F03`/`4E9A`/`4E9B`/`4F11`/`4F1E`/`4ECB`), color uniforme en `Static`, onda personalizada en `Wave`, sincronización de botones `Off / Key / Zone / All` y respeto de atajos nativos `Fn` en `Hardware Effects`.
- **Recuperación tras limpieza de ventiladores (`#385`, `#364`):** Refresca `platform_profile` (`WMI 0x1A`) al salir del modo máximo, aplica un ciclo inicial seguro y ajusta la transición `PWM_MODE_MAX -> PWM_MODE_MANUAL` en `driver/hp-wmi.c`.
- **Código más limpio y modular:** Proxies D-Bus, comparación semver y detección de idioma unificados en `omen-types`; módulos divididos (`i18n/`, `monitoring/`, `updater/`, `fan/protection.rs`), paridad TR/EN/ES verificada por pruebas, `cargo fmt` en CI y limpieza de archivos/código sin uso.
- **Incluye todas las correcciones de `v2.5.2`:** Corrección de cierre en Ajustes y recuperación del bloqueo máximo EC en `8C77` (`#395`).

---

## Documentación y Seguridad

- **[Registro de cambios](CHANGELOG.md)** · **[Política de seguridad](SECURITY.md)** · **[Guía de instalación](docs/INSTALACION.es.md)**

---

## Licencia

Distribuido bajo la licencia **GPL-3.0**.  
*OMEN Space es un proyecto independiente de código abierto y no está afiliado ni respaldado por HP Inc.*
