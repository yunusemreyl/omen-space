#!/usr/bin/env bash
# omen-rgb-test.sh — krok 4: EKSPERYMENT. Mapujemy zone0..zone7 na fizyczne elementy.
# Uruchom w terminalu (nie przez tee — potrzebne pytania):  sudo bash omen-rgb-test.sh
# Zapisuje do sysfs, ale wszystkie oryginalne wartosci sa przywracane na koncu (Ctrl+C tez).

D=/sys/devices/platform/hp-rgb-lighting
[ -d "$D" ] || { echo "brak $D — czy modul hp_rgb_lighting jest zaladowany?"; exit 1; }

declare -A ORIG
for z in 0 1 2 3 4 5 6 7; do ORIG[$z]=$(cat "$D/zone$z" 2>/dev/null); done
OB=$(cat "$D/brightness" 2>/dev/null)

restore() {
  echo; echo ">>> przywracam stan wyjsciowy..."
  for z in 0 1 2 3 4 5 6 7; do echo "${ORIG[$z]}" > "$D/zone$z" 2>/dev/null; done
  echo "$OB" > "$D/brightness" 2>/dev/null
  echo "gotowe."
}
trap restore EXIT INT TERM

w() { # w <plik> <wartosc>
  if echo "$2" > "$D/$1" 2>/tmp/rgbfail; then
    printf '  zapis %-10s <- %-8s OK   (odczyt: %s)\n' "$1" "$2" "$(cat "$D/$1")"
  else
    printf '  zapis %-10s <- %-8s BLAD: %s\n' "$1" "$2" "$(cat /tmp/rgbfail)"
  fi
}

echo "=== stan wyjsciowy ==="
echo "brightness=$OB"
for z in 0 1 2 3 4 5 6 7; do echo "zone$z=${ORIG[$z]}"; done

echo
echo "=== TEST 1: jaki format przyjmuje brightness ==="
for v in 100 255 1 50; do w brightness "$v"; done
echo "brightness po testach = $(cat "$D/brightness")"
read -rp "Czy cokolwiek zmienilo jasnosc? (klawiatura / pasek / nic): " A1

echo
echo "=== TEST 2: ustawiam brightness na maksimum i gaszę wszystkie strefy ==="
w brightness 100
for z in 0 1 2 3 4 5 6 7; do echo 000000 > "$D/zone$z" 2>/dev/null; done
sleep 1
read -rp "Co teraz jest zgaszone? (klawiatura / pasek / oba / nic): " A2

echo
echo "=== TEST 3: kazda strefa po kolei na CZERWONO, reszta zgaszona ==="
for z in 0 1 2 3 4 5 6 7; do
  for y in 0 1 2 3 4 5 6 7; do echo 000000 > "$D/zone$y" 2>/dev/null; done
  echo FF0000 > "$D/zone$z" 2>/dev/null
  sleep 0.3
  read -rp "  zone$z = FF0000 -> co swieci na czerwono? [enter=nic]: " R
  echo "    ZAPIS: zone$z => ${R:-nic}"
done

echo
echo "=== TEST 4: kolejnosc skladowych (czy to RGB czy BGR) ==="
for z in 0 1 2 3 4 5 6 7; do echo 000000 > "$D/zone$z" 2>/dev/null; done
echo 0000FF > "$D/zone0" 2>/dev/null
read -rp "  zone0 = 0000FF -> jaki widzisz kolor? (niebieski/czerwony/inny): " A4

echo
echo "=== TEST 5: czy strefy 4-7 w ogole cokolwiek robia ==="
for z in 4 5 6 7; do echo 00FF00 > "$D/zone$z" 2>/dev/null; done
sleep 1
read -rp "  zone4-7 = 00FF00 -> cokolwiek zielonego? (tak/nie, gdzie): " A5

echo
echo "=== TEST 6: win_lock (moze podswietlac klawisz Win) ==="
echo "win_lock przed: $(cat "$D/win_lock" 2>/dev/null | tr -d '\0')"
w win_lock 1; sleep 1
read -rp "  win_lock=1 -> zmiana? [enter=nic]: " A6
w win_lock 0

echo
echo "================ PODSUMOWANIE (skopiuj to) ================"
echo "T1 brightness: $A1"
echo "T2 gaszenie:   $A2"
echo "T4 0000FF:     $A4"
echo "T5 zone4-7:    $A5"
echo "T6 win_lock:   $A6"
echo "==========================================================="
