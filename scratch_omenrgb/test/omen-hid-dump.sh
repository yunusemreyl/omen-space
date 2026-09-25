#!/usr/bin/env bash
# omen-hid-dump.sh — krok 4b: deskryptory HID wbudowanej klawiatury (0d62:54bf, port 3-9)
# Uruchom: sudo bash omen-hid-dump.sh 2>&1 | tee omen-hid-dump.txt
# TYLKO ODCZYT. Uzywa od zamiast xxd (xxd nie ma w tej instalacji).

dump() { od -An -tx1 -v "$1" | tr -s ' ' | sed 's/^ //'; }

for d in /sys/bus/hid/devices/*0D62:54BF*; do
  [ -f "$d/report_descriptor" ] || continue
  n=$(basename "$d")
  size=$(od -An -tx1 -v "$d/report_descriptor" | wc -w)
  echo "======== $n  (rzeczywisty rozmiar deskryptora: $size bajtow) ========"
  hex=$(dump "$d/report_descriptor" | tr -d ' \n')
  echo "$hex" | fold -w 64
  echo "-- interfejs USB: $(readlink -f "$d" | grep -oE '3-9:[0-9.]+')"
  echo "-- hidraw: $(ls "$d/hidraw" 2>/dev/null | tr '\n' ' ')"
  # Usage Page (Vendor Defined) = 06 xx FF
  if echo "$hex" | grep -qE '06[0-9a-f]{2}ff'; then
    echo "-- >>> VENDOR-DEFINED USAGE PAGE OBECNA (kandydat na kanal RGB)"
  else
    echo "-- (brak vendor-defined usage page)"
  fi
  echo
done

echo "======== rozmiary raportow wg hidraw (do czego mozna pisac) ========"
for h in /dev/hidraw3 /dev/hidraw4 /dev/hidraw5 /dev/hidraw6 /dev/hidraw7; do
  [ -e "$h" ] || continue
  echo "$h -> $(udevadm info -q property -n "$h" 2>/dev/null | grep -E 'HID_NAME|HID_ID' | tr '\n' ' ')"
done

echo
echo "======== SSDT15: co robi komenda 0x2000B ========"
t=$(ls -d /tmp/tmp.*/ 2>/dev/null | head -1)
if [ -f "${t}SSDT15.dsl" ]; then
  grep -n -B4 -A25 '0x0002000B' "${t}SSDT15.dsl" | head -n 120
else
  echo "(brak zdekompilowanego SSDT15 — uruchom ponownie omen-kbd-probe.sh, sciezka byla /tmp/tmp.6tTik7sCXA)"
fi
