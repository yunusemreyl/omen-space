#!/usr/bin/env bash
# omen-kbd-probe.sh — krok 3: znajdz kanal sterowania PODSWIETLENIEM KLAWIATURY (nie paskiem LED)
# Uruchom:  sudo bash omen-kbd-probe.sh 2>&1 | tee omen-kbd-probe.txt
# TYLKO ODCZYT. Nic nie zapisuje do sprzetu.

sec() { printf '\n==================== %s ====================\n' "$1"; }

sec "1. Sterownik hp_rgb_lighting — skad i co wystawia"
modinfo hp_rgb_lighting 2>&1 | grep -Ev '^(parmtype|sig|vermagic)' | head -25
f=$(modinfo -n hp_rgb_lighting 2>/dev/null); echo "plik: $f"
rpm -qf "$f" 2>/dev/null || echo "(rpm nie zna -> DKMS/out-of-tree)"
echo "--- drzewo ---"
find /sys/devices/platform/hp-rgb-lighting/ 2>/dev/null | grep -v '/power/' | sort
echo "--- wartosci atrybutow ---"
find /sys/devices/platform/hp-rgb-lighting/ -type f 2>/dev/null | grep -v '/power/' | sort | while read -r a; do
  printf '%-60s = %s\n' "${a#/sys/devices/platform/hp-rgb-lighting/}" "$(cat "$a" 2>&1 | head -c 160 | tr '\n' ' ')"
done
echo "--- prawa zapisu ---"
find /sys/devices/platform/hp-rgb-lighting/ -type f 2>/dev/null | grep -v '/power/' | xargs -r ls -l 2>/dev/null

sec "2. Ktora klawiatura jest WBUDOWANA (klucz: removable=fixed)"
for u in /sys/bus/usb/devices/*; do
  [ -f "$u/idVendor" ] || continue
  vid=$(cat "$u/idVendor"); pid=$(cat "$u/idProduct")
  case "$vid:$pid" in
    0d62:54bf|258a:0049|03f0:379d|03f0:479d|03f0:01c5)
      printf '%-12s %s:%s  removable=%-10s manuf=%-18s prod=%-28s port=%s\n' \
        "$(basename "$u")" "$vid" "$pid" \
        "$(cat "$u/removable" 2>/dev/null)" \
        "$(cat "$u/manufacturer" 2>/dev/null)" \
        "$(cat "$u/product" 2>/dev/null)" \
        "$(cat "$u/devpath" 2>/dev/null)"
      ;;
  esac
done
echo "(uwaga: removable=fixed => wlutowane w laptopa; removable=removable => cos w doku/USB)"

sec "3. Mapowanie HID -> hidraw -> interfejs USB"
for d in /sys/bus/hid/devices/*; do
  [ -e "$d" ] || break
  name=$(grep -m1 '^HID_NAME=' "$d/uevent" 2>/dev/null | cut -d= -f2-)
  case "$name" in *Gaming*|*HP*|*Realtek*|*HyperX*) ;; *) continue ;; esac
  hidraw=$(ls "$d/hidraw" 2>/dev/null | tr '\n' ' ')
  drv=$(basename "$(readlink -f "$d/driver" 2>/dev/null)")
  intf=$(readlink -f "$d" | grep -oE 'usb[0-9]+/[0-9.:/-]+' | tail -c 60)
  printf '%-24s %-30s hidraw=%-10s drv=%-8s %s\n' "$(basename "$d")" "$name" "${hidraw:--}" "$drv" "$intf"
done

sec "4. Deskryptory raportow — szukamy vendor-defined (Usage Page 0xFF..) = kanal RGB"
for d in /sys/bus/hid/devices/*; do
  [ -f "$d/report_descriptor" ] || continue
  name=$(grep -m1 '^HID_NAME=' "$d/uevent" 2>/dev/null | cut -d= -f2-)
  case "$name" in *Gaming*|*Realtek*) ;; *) continue ;; esac
  echo "### $(basename "$d")  [$name]  bytes=$(stat -c%s "$d/report_descriptor")"
  xxd -g1 "$d/report_descriptor" | head -n 8
  # 06 xx ff = Usage Page (Vendor Defined)
  if xxd -p "$d/report_descriptor" | tr -d '\n' | grep -qE '06[0-9a-f]{2}ff'; then
    echo "  >>> ZAWIERA Usage Page vendor-defined — kandydat na kanal sterowania RGB"
  fi
done

sec "5. Czy hidraw da sie czytac + jakie Feature reporty (bezpieczny odczyt)"
ls -l /dev/hidraw* 2>/dev/null
command -v hidrd-convert >/dev/null && echo "hidrd-convert dostepny" || echo "(opcjonalnie: dnf install hidrd)"

sec "6. Narzedzia, ktore juz moga to znac"
command -v openrgb >/dev/null && { echo "OpenRGB zainstalowany:"; openrgb --list-devices 2>&1 | head -n 40; } || echo "OpenRGB nieobecny (dnf install openrgb / flatpak)"

sec "7. Tabele ACPI — gdzie naprawde siedzi gaming WMI (SSDT, nie DSDT)"
ls -1 /sys/firmware/acpi/tables/ | head -30
if command -v iasl >/dev/null; then
  t=$(mktemp -d)
  for tab in /sys/firmware/acpi/tables/SSDT*; do
    cp "$tab" "$t/$(basename "$tab").dat" 2>/dev/null
  done
  (cd "$t" && for x in *.dat; do iasl -d "$x" >/dev/null 2>&1; done)
  echo "--- wystapienia identyfikatorow komend w SSDT ---"
  grep -l '0x00020009' "$t"/*.dsl 2>/dev/null | sed 's/^/FOURZONE 0x20009 w: /'
  grep -l '0x0002000B' "$t"/*.dsl 2>/dev/null | sed 's/^/0x2000B w: /'
  grep -l '0x0002000C' "$t"/*.dsl 2>/dev/null | sed 's/^/0x2000C w: /'
  grep -ohE '0x000200[0-9A-F]{2}' "$t"/*.dsl 2>/dev/null | sort | uniq -c | sort -rn | head -20
  echo "(zdekompilowane SSDT: $t)"
else
  echo "brak iasl: sudo dnf install acpica-tools"
fi

echo
echo "GOTOWE — wyslij omen-kbd-probe.txt"
