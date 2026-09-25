#!/usr/bin/env bash
# smoke.sh — test na FIZYCZNYM sprzecie. Przechodzi przez wszystkie efekty
# i mierzy to, czego atrapy nie sprawdza: czasy, dryf klatek, zuzycie pamieci.
#
#   bash test/smoke.sh [sekundy_animacji]     (domyslnie 30)
#
# Na koncu oddaje kontrole firmware'owi, chyba ze podasz KEEP=1.
set -uo pipefail
SECS="${1:-30}"
ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
export PYTHONPATH="$ROOT"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
K() { python3 -m omenkbd "$@"; }
sleep_() { python3 -c "import time;time.sleep($1)"; }
fail=0
ok()   { echo "  OK   $*"; }
bad()  { echo "  BLAD $*"; fail=$((fail+1)); }

echo "== 1. urzadzenie =="
if K status > /dev/null 2>&1; then K status | sed 's/^/  /'; else
  bad "demon nie odpowiada — uruchom: python3 -m omenkbd.engine.daemon -v"; exit 1; fi

PID=$(ps -eo pid,args | grep 'omenkbd[.]engine[.]daemon' | awk '{print $1}' | head -1)
frames() { K --json status | python3 -c "import json,sys;print(json.load(sys.stdin)['stats']['frames'])"; }
rss()    { awk '/VmRSS/{print $2}' /proc/$PID/status; }

echo "== 2. czas odpowiedzi (kryterium: < 100 ms) =="
for c in "all 00FF88" "all FF0055" "preset gaming" "gradient red blue"; do
  t0=$(python3 -c "import time;print(time.monotonic())")
  K $c >/dev/null
  ms=$(python3 -c "import time,sys;print(round((time.monotonic()-float(sys.argv[1]))*1000))" $t0)
  [ "$ms" -lt 100 ] && ok "$c -> ${ms} ms" || bad "$c -> ${ms} ms (limit 100)"
done

echo "== 3. wszystkie tryby z katalogu rysuja sie bez bledu =="
MODES=$(K --json effects | python3 -c "
import json,sys
print(' '.join(c['name'] for c in json.load(sys.stdin)['catalogue']))")
for e in $MODES; do
  [ "$e" = perkey ] && continue
  K effect $e >/dev/null 2>&1 && ok "tryb $e" || bad "tryb $e"
  sleep_ 0.35
done
for e in "gradient red blue --axis y" "keys W,A,S,D,Space red --base 101010"; do
  K $e >/dev/null 2>&1 && ok "$e" || bad "$e"
  sleep_ 0.35
done
echo "== 3b. parametry i przycinanie zakresu =="
K effect fire speed=99 >/dev/null 2>&1 \
  && [ "$(K --json status | python3 -c 'import json,sys;print(json.load(sys.stdin)["effect"]["speed"])')" != "99.0" ] \
  && ok "wartosc poza zakresem przycieta" || bad "wartosc poza zakresem nie przycieta"
K effect scanner color=00FF00 width=0.3 >/dev/null 2>&1 && ok "parametry z CLI" || bad "parametry z CLI"
for p in gaming typing wasd mods; do
  K preset $p >/dev/null 2>&1 && ok "preset $p" || bad "preset $p"
  sleep_ 0.4
done

echo "== 4. animacja przez ${SECS}s: dryf i pamiec (kryterium: bez dryfu i wyciekow) =="
K wave >/dev/null
f0=$(frames); r0=$(rss); t0=$(python3 -c "import time;print(time.monotonic())")
sleep_ "$SECS"
f1=$(frames); r1=$(rss); t1=$(python3 -c "import time;print(time.monotonic())")
python3 - "$t0" "$t1" "$f0" "$f1" "$r0" "$r1" <<'PY'
import sys
t0,t1,f0,f1,r0,r1=[float(x) for x in sys.argv[1:3]]+[int(x) for x in sys.argv[3:7]]
el=t1-t0; fr=f1-f0; exp=el/0.033
print(f"  {fr} klatek w {el:.1f}s = {fr/el:.2f} kl./s (limit 30.3)")
print(f"  dryf {(fr-exp)/exp*100:+.2f}%   RSS {r0} -> {r1} kB ({r1-r0:+d})")
sys.exit(0 if abs((fr-exp)/exp) < 0.02 and (r1-r0) < 512 else 1)
PY
[ $? = 0 ] && ok "animacja stabilna" || bad "dryf albo wyciek pamieci"

echo "== 5. bezczynnosc: efekt statyczny nie zjada CPU =="
K all 202060 >/dev/null
c0=$(awk '{print $14+$15}' /proc/$PID/stat); sleep_ 5; c1=$(awk '{print $14+$15}' /proc/$PID/stat)
[ $((c1-c0)) -le 2 ] && ok "$((c1-c0)) tickow CPU przez 5 s" || bad "$((c1-c0)) tickow — efekt statyczny nie powinien liczyc"

echo "== 6. profile =="
K profile save __smoke >/dev/null && ok "zapis" || bad "zapis"
K wave >/dev/null; K profile load __smoke >/dev/null
K status | grep -q 'static' && ok "odczyt" || bad "odczyt"
K profile delete __smoke >/dev/null && ok "usuniecie" || bad "usuniecie"

echo "== 7. bledy sa komunikatami, nie tracebackami (kryterium 7) =="
for bad_cmd in "all zielonkawy" "preset nie-ma" "keys NIEMA red" "profile load nie-ma"; do
  out=$(K $bad_cmd 2>&1)
  if echo "$out" | grep -q Traceback; then bad "$bad_cmd -> traceback"
  elif echo "$out" | grep -qi 'blad'; then ok "$bad_cmd -> \"$(echo "$out" | head -1 | cut -c1-60)\""
  else bad "$bad_cmd -> brak komunikatu"; fi
done

echo "== 8. resume (to, co robi hook wybudzenia) =="
K resume >/dev/null 2>&1 && ok "resume" || bad "resume"

echo "== 9. przelacznik sterowania BIOS / aplikacja =="
K control bios >/dev/null 2>&1 \
  && [ "$(K --json status | python3 -c 'import json,sys;print(json.load(sys.stdin)["control"])')" = bios ] \
  && ok "oddane do BIOS-u" || bad "oddane do BIOS-u"
sleep_ 1
K control app >/dev/null 2>&1 \
  && [ "$(K --json status | python3 -c 'import json,sys;print(json.load(sys.stdin)["control"])')" = app ] \
  && ok "odebrane przez aplikacje" || bad "odebrane przez aplikacje"

echo
if [ "${KEEP:-0}" = 1 ]; then echo "zostawiam biezacy efekt (KEEP=1)"; else
  K release >/dev/null && echo "kontrola oddana firmware'owi"; fi
echo
[ "$fail" = 0 ] && echo "WSZYSTKO OK" || echo "BLEDOW: $fail"
exit $fail
