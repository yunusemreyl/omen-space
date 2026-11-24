<div align="center">
  <img src="images/omenspace.png" alt="Logo de OMEN Space" width="110" />

  # OMEN Space

  **Centro de control nativo en Linux para portátiles HP OMEN, Victus y Transcend.**  
  Escrito íntegramente en **Rust** con **GTK4 y Libadwaita** para un rendimiento instantáneo y mínimo consumo.

  <p>
    <a href="https://github.com/yunusemreyl/omen-space/releases/latest">
      <img src="https://img.shields.io/badge/Versi%C3%B3n-v2.4.3-e11d48?style=flat-square" alt="Versión v2.4.3" />
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
  <img src="images/perf.png" width="92%" alt="OMEN Space — Rendimiento y Telemetría" />
</div>

---

## Descripción general

**OMEN Space** ofrece control total del hardware en Linux mediante un daemon nativo en Rust (~3 MB) y una interfaz moderna en GTK4 + Libadwaita.

| Módulo | Funciones principales |
| :--- | :--- |
| **Rendimiento y Refrigeración** | Perfiles térmicos `Eco`, `Balanced` y `Performance`; modos de ventilador `Auto`, `Max`, curvas independientes para CPU y GPU con preajustes guardados, delegación directa al **EC**, limpieza de polvo y protección térmica con rebote de 3 s (`>= 90 °C` / `95 °C`). |
| **Energía Avanzada** | Límites de potencia Intel RAPL `PL1` / `PL2` (hasta 250 W / 300 W), límite térmico `TCC Offset`, undervolt por MSR (con detección de `UV_LOCK`), Dynamic Boost de GPU y ajustes para AMD Ryzen SMU. |
| **Selector de Gráficos (MUX)** | Cambio entre **Hybrid (MSHybrid)**, **Discrete (dGPU)** y **Advanced Optimus** con estado de enrutamiento `eDP`, verificación de lectura y versión del controlador NVIDIA. |
| **RGB Studio** | Iluminación RGB de 4 zonas, zona única (Victus), barra de luz frontal, Desktop RGB y teclado por tecla HID LampArray Darfon (`0d62:54bf` / `0d62:30bf`) con calibración física, selector de distribución EN/DE y animaciones de bajo consumo. |
| **Superposición Rápida (`Shift+F2`, configurable)** | Panel flotante personalizable dentro del juego y applet de bandeja del sistema sincronizados en tiempo real. |
| **Centro de Depuración y Actualizador** | Volcado completo del sistema con un clic para GitHub, reglas por aplicación (Wayland y X11) y **Centro de Actualizaciones** con soporte `systemd` / **OpenRC** y firma automática Secure Boot MOK. |

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
  <img src="images/keyboard.png" width="49%" alt="RGB Studio" />
  <img src="images/undervolt.png" width="49%" alt="Energía Avanzada" />
</p>
<p align="center">
  <img src="images/mux.png" width="49%" alt="Selector de Gráficos (MUX)" />
  <img src="images/appprofiles.png" width="49%" alt="Gestor de Perfiles" />
</p>
<p align="center">
  <img src="images/updater.png" width="49%" alt="Centro de Actualizaciones" />
  <img src="images/diagnostic.png" width="49%" alt="Monitor del Sistema" />
</p>

---

## Novedades en v2.4.3

- **Centro de Ajustes unificado y Selector de Gráficos simplificado:** Agrupa **Actualizaciones** y **Diagnóstico** como subpestañas dentro de **Ajustes** (**General | Actualizaciones | Diagnóstico**) dejando 6 secciones principales en la barra lateral, y simplifica **Selector de gráficos** con tarjetas compactas y un único grupo **Hardware & Pantalla**.
- **Limpieza, Seguridad y Gobernanza:** Eliminadas dependencias y recursos sin uso junto con todas las advertencias de Clippy (`-D warnings` en CI); añadidos [SECURITY.md](SECURITY.md), [CHANGELOG.md](CHANGELOG.md), `.github/dependabot.yml`, plantillas de Issues/PR y documentación técnica actualizada en `docs/`.
- **Resumen v2.4.2 / v2.4.1:** MUX GPU validado por BIOS (`gpu_mux_supported_modes`) + guía PRIME, limitador de carga `0x2B` en **Energía y Batería**, corrección de `Shift+F2` / Quick Overlay en Wayland/X11, soporte Primax `0xFF13` (#385) y localización ES/EN/TR completa.

---

## Documentación y Seguridad

- **[Registro de cambios](CHANGELOG.md)** · **[Política de seguridad](SECURITY.md)** · **[Problemas conocidos](knownissues.md)** · **[Guía de instalación](docs/INSTALACION.es.md)**

---

## Licencia

Distribuido bajo la licencia **GPL-3.0**.  
*OMEN Space es un proyecto independiente de código abierto y no está afiliado ni respaldado por HP Inc.*
