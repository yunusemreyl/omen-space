# Brief: aplikacja do sterowania podświetleniem klawiatury HP OMEN MAX 16 (płyta 8D41)

Dokument dla instancji Claude'a, która ma zbudować aplikację. Jest samowystarczalny —
wszystkie fakty poniżej zostały **empirycznie zweryfikowane na fizycznym sprzęcie**,
nie pochodzą z dokumentacji ani z założeń. Nie trzeba niczego reverse-engineerować
od nowa. Protokół działa, istnieje działający prototyp CLI w Pythonie.

---

## 1. Cel

Zbudować pełnoprawną aplikację desktopową (Linux) do sterowania per-key RGB
w klawiaturze HP OMEN MAX 16. Obecny stan: działający, ale surowy skrypt CLI.
Docelowo: narzędzie z GUI, profilami, efektami i poprawną integracją z systemem.

---

## 2. Zweryfikowane fakty sprzętowe

| | |
|---|---|
| Laptop | OMEN MAX Gaming Laptop 16-ah0xxx, `board_name` = `8D41` |
| System testowy | Fedora 44, kernel 7.1.10, KDE Plasma, Secure Boot **włączony** |
| Klawiatura | USB `0d62:54bf` („HP Gaming Keyboard II", Darfon), `removable=fixed` |
| Interfejs sterujący | USB interface **`04`** → `/dev/hidraw*` |
| Protokół | **HID LampArray**, Usage Page `0x59` (Lighting And Illumination) |
| Liczba lampek | **120**, `id` 0–119, bez dziur |
| Geometria | 342 × 125 mm, 6 rzędów po 20 lampek |
| Limit odświeżania | `MinUpdateInterval` = **33 000 µs** (~30 kl./s) |
| Wymagania | brak — żadnego modułu jądra, DKMS, podpisów MOK |

**Czego NIE używać:** WMI `0x020009` („four zone"), na którym opierają się
`OmenLinux/omen-rgb-keyboard`, `pelrun/hp-omen-linux-module`, `hp_rgb_lighting`
i pokrewne. Na tej płycie ten interfejs steruje **listwą LED w obudowie**, a nie
klawiaturą. Mainline `hp-wmi` nie ma obsługi RGB w ogóle.

---

## 3. Protokół — kompletna specyfikacja

Wszystko przez Feature reports na `hidraw`. Little-endian, bez wyrównania,
pierwszy bajt = Report ID.

### Wykrywanie urządzenia

Numeracja `/dev/hidrawN` zmienia się między bootami i przy przepięciu stacji
dokującej. **Nie hardkodować.** Identyfikować po prefiksie deskryptora:

```python
LAMPARRAY_PREFIX = bytes([0x05, 0x59, 0x09, 0x01, 0xA1, 0x01])
#  05 59 = Usage Page (Lighting And Illumination)
#  09 01 = Usage (LampArray)
#  a1 01 = Collection (Application)
# szukać w /sys/class/hidraw/hidraw*/device/report_descriptor
```

Uwaga: `stat` na `report_descriptor` zwraca 4096 (rozmiar strony sysfs), nie
rzeczywistą długość — liczyć odczytane bajty.

### ioctl

```python
def _hidioc(nr, size):          # _IOC(_IOC_WRITE|_IOC_READ, 'H', nr, size)
    return (3 << 30) | (size << 16) | (ord('H') << 8) | nr

HIDIOCSFEATURE = lambda size: _hidioc(0x06, size)
HIDIOCGFEATURE = lambda size: _hidioc(0x07, size)
```

Przy odczycie pierwszy bajt bufora musi zawierać numer raportu.

### Report 1 — LampArrayAttributes (odczyt, 23 B)

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 1 |
| 1 | u16 | LampCount |
| 3 | u32 | BoundingBoxWidth (µm) |
| 7 | u32 | BoundingBoxHeight (µm) |
| 11 | u32 | BoundingBoxDepth (µm) |
| 15 | u32 | LampArrayKind (1 = Keyboard) |
| 19 | u32 | MinUpdateInterval (µs) |

### Report 2 — LampAttributesRequest (zapis, 3 B)

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 2 |
| 1 | u16 | LampId |

**⚠ Na tym firmware jest IGNOROWANY — patrz sekcja 4.1.**

### Report 3 — LampAttributesResponse (odczyt, 29 B)

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 3 |
| 1 | u16 | LampId |
| 3 | u32 | PositionX (µm) |
| 7 | u32 | PositionY (µm) |
| 11 | u32 | PositionZ (µm) |
| 15 | u32 | UpdateLatency (µs) |
| 19 | u32 | LampPurposes (1 Control, 2 Accent, 4 Branding, 8 Status, 16 Illumination, 32 Presentation) |
| 23 | u8 | RedLevelCount |
| 24 | u8 | GreenLevelCount |
| 25 | u8 | BlueLevelCount |
| 26 | u8 | IntensityLevelCount |
| 27 | u8 | IsProgrammable |
| 28 | u8 | **InputBinding** — HID usage klawisza (Keyboard page 0x07) |

### Report 4 — LampMultiUpdate (zapis, 51 B) — do 8 lampek

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 4 |
| 1 | u8 | LampCount (≤ 8) |
| 2 | u8 | LampUpdateFlags (bit 0 = LampUpdateComplete) |
| 3 | u16 × 8 | LampIds |
| 19 | u8 × 32 | RGBI × 8 (Red, Green, Blue, Intensity) |

Przy aktualizacji wielu paczek: `LampUpdateComplete = 0` we wszystkich poza ostatnią,
`1` w ostatniej. Wtedy kontroler zatrzaskuje całą klatkę naraz i nie ma rozjeżdżania.

### Report 5 — LampRangeUpdate (zapis, 10 B)

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 5 |
| 1 | u8 | LampUpdateFlags |
| 2 | u16 | LampIdStart |
| 4 | u16 | LampIdEnd |
| 6 | u8 × 4 | Red, Green, Blue, Intensity |

Najtańszy sposób na jednolity kolor całej klawiatury: jeden zapis `0 → 119`.

### Report 6 — LampArrayControl (zapis, 2 B)

| Off | Typ | Pole |
|---|---|---|
| 0 | u8 | ID = 6 |
| 1 | u8 | AutonomousMode (0 = kontrola hosta, 1 = firmware) |

---

## 4. Pułapki — przeczytać przed pisaniem kodu

### 4.1 Report 2 jest ignorowany (błąd firmware'u)

Kontroler **nie honoruje** żądania o konkretną lampkę. Trzyma własny kursor
i inkrementuje go przy każdym odczycie Reportu 3, niezależnie od tego, o co pytano.
Kursor zawija się na końcu i **przeżywa między procesami** — kolejne uruchomienie
programu podejmuje od miejsca, gdzie skończyło poprzednie.

Dowód z dwóch kolejnych uruchomień:

```
req    0 -> id    2
req    1 -> id    3
...
req  117 -> id    7     ← nowy proces, kursor kontynuuje
req  118 -> id    8
```

**Obejście:** czytać sekwencyjnie i ufać polu `LampId` w odpowiedzi. Zbierać do słownika
`id → atrybuty`, aż uzbiera się `LampCount` unikalnych wpisów (limit prób ~2 × LampCount).
Start w dowolnym miejscu jest bezpieczny.

**Konsekwencja projektowa:** mapę lampek budować **raz** i cache'ować na dysku.
120 odczytów to sporo `ioctl`-i i nie ma sensu robić tego przy każdym starcie GUI.

### 4.2 AutonomousMode

Dopóki `AutonomousMode = 1`, firmware odgrywa własny efekt (fabrycznie: powolne
pulsowanie żółty↔pomarańczowy) i **ignoruje kolory z hosta bez zgłaszania błędu**.
Zapisy „przechodzą", tylko nic nie robią. Każda sesja sterowania musi zacząć się
od Reportu 6 z wartością 0.

### 4.3 Powrót z uśpienia

Po S3 kontroler wraca do `AutonomousMode = 1`. Aplikacja musi wykryć wybudzenie
i ponownie przejąć kontrolę oraz odtworzyć ostatni stan. W systemd działa wzorzec
`WantedBy=suspend.target` **+** `After=suspend.target` (jednostki uporządkowane
po `suspend.target` startują dopiero po zakończeniu operacji uśpienia) plus ~2 s
zwłoki na inicjalizację kontrolera. W aplikacji długo żyjącej lepiej słuchać
`PrepareForSleep` na D-Bus (`org.freedesktop.login1`).

### 4.4 Limit 33 ms

Nie wysyłać klatek częściej niż `MinUpdateInterval`. Animacje ograniczyć do 30 kl./s.

### 4.5 Uprawnienia

Domyślnie `/dev/hidraw*` jest tylko dla roota. Reguła udev zawężona do interfejsu
LampArray (nie do całego VID:PID — przez pozostałe interfejsy idą naciśnięcia
klawiszy i nie ma powodu ich otwierać):

```
SUBSYSTEM=="hidraw", ENV{ID_USB_VENDOR_ID}=="0d62", \
  ENV{ID_USB_MODEL_ID}=="54bf", ENV{ID_USB_INTERFACE_NUM}=="04", \
  GROUP="omenkbd", MODE="0660"
```

Przeładowanie wymaga `udevadm trigger --action=add` — samo `trigger` nie odświeża ACL.

**Nie używać `ATTRS{}` do połączenia `idVendor` z `bInterfaceNumber`.** Klucze
w formie mnogiej muszą trafić w to samo urządzenie nadrzędne, a te atrybuty leżą
na różnych (`3-9` i `3-9:1.4`) — taka reguła przechodzi `udevadm verify`, ale
nigdy nie dopasowuje. `ENV{ID_USB_*}` jest ustawiane na samym urządzeniu hidraw
i tego ograniczenia nie ma. Po instalacji sprawdzić skutek: `stat -c %G /dev/hidrawN`.

### 4.6 OpenRGB tego nie obsłuży

OpenRGB opiera się na sterownikach pisanych pod protokoły konkretnych producentów
i nie ma generycznego sterownika HID LampArray. Nie liczyć na integrację przez
OpenRGB; ewentualnie rozważyć napisanie pluginu do niego jako osobny wątek.

---

## 5. Mapa klawiatury (zweryfikowana)

Firmware zwraca pełną geometrię i przypisanie klawiszy. `InputBinding` to prawdziwe
HID usages z Keyboard page (`0x04`=A … `0x1d`=Z, `0x1e`–`0x27`=1–0, `0x3a`–`0x45`=F1–F12,
`0xe0`–`0xe7`=modyfikatory).

```
y= 9000 (20)  0:Esc 1:— 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10
              12:F11 13:F12 14:— 15:Delete 16:— 17:Menu 18:— 19:—
y=25000 (20)  20:`~ 21:— 22:1 23:2 24:3 25:4 26:5 27:6 28:7 29:8 30:9 31:0
              32:-_ 33:=+ 34:Backspace 35:Backspace 36:NumLock 37:KP/ 38:KP* 39:KP-
y=44000 (20)  40:Tab 41:Tab 42:— 43:Q 44:W 45:E 46:R 47:T 48:Y 49:U 50:I 51:O
              52:P 53:[{ 54:]} 55:\| 56:KP7 57:KP8 58:KP9 59:KP+
y=62000 (20)  60:CapsLock 61:CapsLock 62:— 63:A 64:S 65:D 66:F 67:G 68:H 69:J
              70:K 71:L 72:;: 73:'" 74:Enter 75:Enter 76:KP4 77:KP5 78:KP6 79:KP+
y=81000 (20)  80:LShift 81:LShift 82:LShift 83:Z 84:X 85:C 86:V 87:B 88:N 89:M
              90:,< 91:.> 92:/? 93:RShift 94:RShift 95:RShift 96:KP1 97:KP2 98:KP3 99:KPEnter
y=99000 (20)  100:LCtrl 101:— 102:Omen 103:LMeta 104:LAlt 105:Space 106:Space
              107:Space 108:Space 109:Space 110:RAlt 111:— 112:Left 113:Down
              114:Up 115:Right 116:KP0 117:KP0 118:KP. 119:KPEnter
```

Uwagi: Spacja ma 5 lampek, lewy Shift 3, prawy Shift 3, Enter 2, Tab 2, CapsLock 2,
Backspace 2. Lampki `—` (`InputBinding` = `0x00` lub `0x03`) to diody bez klawisza,
w przerwach między blokami. `0xe8` to klawisz OMEN.

**Ta mapa dotyczy jednego egzemplarza.** Aplikacja ma ją odczytywać z urządzenia,
a nie hardkodować — inne warianty klawiatury (układ ISO/ANSI, wersje bez bloku
numerycznego) będą miały inną. Powyższe traktować jako dane testowe i wzorzec
do walidacji parsera.

---

## 6. Istniejący prototyp

Działający CLI w Pythonie (~350 linii, bez zależności zewnętrznych) realizuje:
wykrywanie urządzenia, odczyt atrybutów, budowę i cache mapy, jednolity kolor,
kolorowanie po nazwach klawiszy, gradienty wzdłuż osi X/Y, presety, tęczową falę,
zwolnienie kontroli. Do tego instalator z regułą udev i jednostką systemd.

Prototyp jest referencją dla protokołu, **nie** wzorcem architektonicznym —
aplikacja docelowa powinna mieć własną, sensowną strukturę.

---

## 7. Zakres aplikacji docelowej

### Musi mieć

1. **Warstwa urządzenia** — wykrywanie po deskryptorze, obsługa braku uprawnień
   z czytelnym komunikatem, odporność na przepięcie USB i zmianę numeru `hidraw`.
2. **Model mapy** — odczyt z cache'em i wersjonowaniem (klucz: `LampCount` +
   `BoundingBox`), z ręcznym odświeżeniem.
3. **Silnik efektów** — statyczny kolor, gradient, fala, oddech, per-key.
   Klatkowanie ograniczone do `MinUpdateInterval`, wysyłka paczkami po 8
   z poprawnym `LampUpdateComplete`.
4. **Reactive typing** — klawisz rozbłyska pod palcem i gaśnie. Wymaga czytania
   `/dev/input/event*` (grupa `input` lub demon). Mapowanie: kod klawisza evdev →
   HID usage → `InputBinding` → `LampId`. Uwaga: jednemu klawiszowi odpowiada
   czasem kilka lampek (Spacja: 5).
5. **Profile** — zapis/odczyt nazwanych konfiguracji, przełączanie.
6. **Trwałość** — odtworzenie stanu po starcie systemu i po wybudzeniu
   (D-Bus `PrepareForSleep`, nie polling).
7. **Instalacja** — udev + jednostka systemd (user albo system, do decyzji),
   deinstalator wycofujący wszystkie zmiany.

### Do rozważenia

* GUI: GTK4/libadwaita (naturalne dla Fedory) albo Qt6 (spójne z KDE, na którym
  to działa). Wybór uzasadnić.
* Wizualny edytor per-key — klikalny układ klawiatury rysowany z rzeczywistych
  współrzędnych XY z firmware'u, nie z hardkodowanego obrazka. To wyróżnik:
  działa na każdym wariancie klawiatury bez dorabiania grafik.
* Ikona w trayu, szybkie przełączanie profili.
* Reakcja na stan systemu: poziom baterii, tryb wydajności, powiadomienia.
* Pakowanie: RPM dla Fedory i/lub Flatpak (uwaga: Flatpak i dostęp do `hidraw`
  wymaga przemyślenia — prawdopodobnie potrzebny portal albo `--device=all`).

### Poza zakresem

* Listwa LED w obudowie (osobny kanał, WMI `0x020009` — obsługiwana przez
  istniejące moduły, można ewentualnie zintegrować później).
* Sterowanie wentylatorami i profilami termicznymi.
* Cokolwiek wymagającego modułu jądra lub wyłączania Secure Boot.

---

## 8. Kryteria akceptacji

1. Uruchomienie na czystym systemie bez roota (po instalacji reguły udev).
2. Ustawienie jednolitego koloru w < 100 ms od startu procesu przy ciepłym cache.
3. Animacja przez 10 minut bez dryfu, zacięć i narastania zużycia pamięci.
4. Po `systemctl suspend` i wybudzeniu stan wraca automatycznie w < 5 s.
5. Odłączenie i podpięcie stacji dokującej (zmienia numerację `hidraw`)
   nie psuje działania.
6. Odinstalowanie przywraca system do stanu wyjściowego, a klawiaturę
   do fabrycznego pulsowania.
7. Czytelny komunikat, a nie traceback, gdy: brak urządzenia, brak uprawnień,
   urządzenie zniknęło w trakcie działania.

---

## 9. Materiały źródłowe

* [HID Lighting and Illumination Page (0x59) — HUTRR84, usb.org](https://www.usb.org/sites/default/files/hutrr84_-_lighting_and_illumination_page.pdf) — normatywna specyfikacja
* [Dynamic lighting / LampArray — Microsoft Learn](https://learn.microsoft.com/en-us/windows/uwp/devices-sensors/lighting-dynamic-lamparray) — czytelniejszy opis modelu raportów
* [xz-dev/hid-rgb-ctl](https://github.com/xz-dev/hid-rgb-ctl) — istniejące narzędzie do LampArray pod Linuksem, warto zobaczyć przed pisaniem od zera
* [PATCH v4: HID: generic: add LampArray support via hid-lamparray helper](https://lkml.iu.edu/2602.2/07495.html) — patch mający wystawić takie urządzenia przez `/sys/class/leds`; jeśli wejdzie do jądra, pojawi się alternatywna ścieżka dostępu i warto ją wtedy obsłużyć obok `hidraw`
* HID Usage Tables, Keyboard/Keypad Page (0x07) — do mapowania `InputBinding`
