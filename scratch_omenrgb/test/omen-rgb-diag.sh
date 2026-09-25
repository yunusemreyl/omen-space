#!/usr/bin/env bash
# omen-rgb-diag.sh — rozpoznanie sterowania podswietleniem klawiatury OMEN (plyta 8D41)
# Uruchom:  sudo bash omen-rgb-diag.sh 2>&1 | tee omen-rgb-diag.txt
# Skrypt TYLKO CZYTA. Nic nie zapisuje do sprzetu.

sec() { printf '\n==================== %s ====================\n' "$1"; }

sec "1. Platforma / DMI"
for f in sys_vendor product_name product_family board_name board_version bios_version bios_date; do
  printf '%-16s: %s\n' "$f" "$(cat /sys/class/dmi/id/$f 2>/dev/null)"
done
printf '%-16s: %s\n' "kernel" "$(uname -r)"
printf '%-16s: %s\n' "distro" "$(. /etc/os-release 2>/dev/null; echo "$PRETTY_NAME")"

sec "2. Moduly / komunikaty jadra"
lsmod | grep -Ei 'hp_wmi|hp_accel|wmi|omen' || echo "(brak dopasowan)"
echo "--- dmesg ---"
dmesg 2>/dev/null | grep -Ei 'hp.?wmi|omen|wmi:|platform.profile' | tail -n 40 || echo "(brak; sprobuj: journalctl -k)"

sec "3. Urzadzenia WMI (szukamy GUID BIOS 5FB7F034-2C63-45e9-BE91-3D44E2C707E4)"
for d in /sys/bus/wmi/devices/*; do
  [ -e "$d" ] || { echo "(brak /sys/bus/wmi/devices)"; break; }
  guid=$(basename "$d")
  drv=$(basename "$(readlink -f "$d/driver" 2>/dev/null)" 2>/dev/null)
  printf '%s  obiekt=%s  driver=%s\n' "$guid" "$(cat "$d/object_id" 2>/dev/null)" "${drv:--}"
done

sec "4. Interfejsy LED w sysfs"
ls -1 /sys/class/leds/ 2>/dev/null || echo "(pusto)"
echo "--- szczegoly LED-ow klawiatury ---"
for l in /sys/class/leds/*kbd* /sys/class/leds/*rgb* /sys/class/leds/*hp*; do
  [ -e "$l" ] || continue
  echo "### $l"; ls -1 "$l"
  echo "max_brightness=$(cat "$l/max_brightness" 2>/dev/null) brightness=$(cat "$l/brightness" 2>/dev/null)"
done

sec "5. Platform devices HP / OMEN"
ls -1 /sys/devices/platform/ | grep -Ei 'hp|omen' || echo "(brak)"
for p in /sys/devices/platform/hp-wmi /sys/devices/platform/omen-rgb-keyboard; do
  [ -d "$p" ] && { echo "### $p"; find "$p" -maxdepth 2 -type f -printf '%p\n' 2>/dev/null | head -n 40; }
done
echo "--- platform_profile ---"
cat /sys/firmware/acpi/platform_profile_choices 2>/dev/null
cat /sys/firmware/acpi/platform_profile 2>/dev/null

sec "6. Urzadzenia HID/USB (per-key RGB idzie zwykle przez HID, vendor 03F0 = HP)"
command -v lsusb >/dev/null && lsusb | grep -i '03f0' ; lsusb 2>/dev/null | head -n 20
echo "--- HID ---"
for h in /sys/bus/hid/devices/*; do
  [ -e "$h" ] || break
  echo "$(basename "$h")  $(cat "$h/../product" 2>/dev/null || cat "$h/uevent" 2>/dev/null | grep -m1 HID_NAME)"
done
ls -1 /dev/hidraw* 2>/dev/null || echo "(brak /dev/hidraw*)"

sec "7. ACPI: czy w DSDT sa metody WMI HP (WMBA/WMBB/HPWMI)"
if [ -r /sys/firmware/acpi/tables/DSDT ]; then
  if command -v iasl >/dev/null; then
    tmp=$(mktemp -d); cp /sys/firmware/acpi/tables/DSDT "$tmp/dsdt.dat"
    (cd "$tmp" && iasl -d dsdt.dat >/dev/null 2>&1)
    grep -oE 'Method \(WM[A-Z0-9]{2}' "$tmp/dsdt.dsl" 2>/dev/null | sort -u
    grep -c '0x00020009' "$tmp/dsdt.dsl" 2>/dev/null | sed 's/^/wystapien 0x20009 (FOURZONE): /'
    echo "(zdekompilowany DSDT: $tmp/dsdt.dsl)"
  else
    echo "brak iasl — zainstaluj: sudo dnf install acpica-tools"
    strings /sys/firmware/acpi/tables/DSDT | grep -Ei '^WM[A-Z0-9]{2}$' | sort -u | head
  fi
else
  echo "(brak dostepu do DSDT — uruchom przez sudo)"
fi

sec "8. Secure Boot (blokuje ladowanie niepodpisanych modulow DKMS)"
command -v mokutil >/dev/null && mokutil --sb-state || echo "(brak mokutil; dnf install mokutil)"

echo
echo "GOTOWE. Wyslij plik omen-rgb-diag.txt."
