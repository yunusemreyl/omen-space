# Sterowanie podświetleniem klawiatury — HP OMEN MAX 16 (płyta 8D41) pod Linuksem

Dokumentacja procesu dojścia do działającego rozwiązania: od nieudanych prób ze
sterownikami WMI, przez identyfikację właściwego interfejsu, po działające
sterowanie per-key. Zawiera ślepe uliczki, bo one też są wynikiem.

---

## TL;DR

Klawiatura w OMEN MAX 16 **nie jest** sterowana przez WMI, którego używają wszystkie
popularne linuksowe projekty dla OMEN-ów. Jest osobnym urządzeniem USB wystawiającym
standardowy **HID LampArray** (Usage Page `0x59`, „Lighting And Illumination").

Sterowanie sprowadza się do dwóch `ioctl`-i na `/dev/hidraw*`:

1. Feature report `6` → `AutonomousMode = 0` (odebranie kontroli firmware'owi)
2. Feature report `5` lub `4` → kolory lampek

Nie jest potrzebny żaden moduł jądra, DKMS ani wyłączanie Secure Boot.

---

## 1. Środowisko

| | |
|---|---|
| Model | OMEN MAX Gaming Laptop 16-ah0xxx |
| Płyta (`board_name`) | `8D41` |
| Rodzina | `103C_5335M7 HP OMEN` |
| BIOS | F.20, 2025-10-23 |
| System | Fedora Linux 44 (KDE Plasma) |
| Kernel | 7.1.10-200.fc44.x86_64 |
| Secure Boot | włączony |

Objaw wyjściowy: klawiatura świeci i **powoli pulsuje z żółtego w pomarańczowy** od
momentu instalacji Fedory. Żadne narzędzie linuksowe nie miało na to wpływu.

---

## 2. Ślepa uliczka: WMI „four zone"

### 2.1 Co próbowaliśmy

Punktem wyjścia był projekt `OmenLinux/omen-rgb-keyboard` (i pokrewne:
`pelrun/hp-omen-linux-module`, `ranisalt/…`, `kuterd/hp_omen_led_controler`,
`yunusemreyl/OmenCtl`). Wszystkie używają tego samego mechanizmu:

```
GUID BIOS:   5FB7F034-2C63-45e9-BE91-3D44E2C707E4
GUID zdarzeń: 95F24279-4D7B-4334-9387-ACCDC67EF61C

HPWMI_GAMING            = 0x020008
HPWMI_FOURZONE          = 0x020009
  subkomendy: 0x01 sprawdzenie wsparcia
              0x02 odczyt kolorów stref
              0x03 zapis kolorów stref
              0x04 odczyt jasności
              0x05 zapis jasności
HPWMI_GM_GET_KEYBOARD_TYPE = 0x2B  (w rodzinie 0x020008)
```

Bufor wywołania ma 128 bajtów; kolory czterech stref leżą jako kolejne trójki RGB
w okolicy offsetu 25.

### 2.2 Dlaczego to nie zadziałało

Mechanizm **działa** na tym sprzęcie — ale steruje **paskiem LED na obudowie**, nie
klawiaturą. W OMEN MAX HP przepiął logiczne „cztery strefy klawiatury" na listwę
w podstawie. Nazwy projektów mylą, bo powstawały dla laptopów, gdzie te strefy
faktycznie były klawiaturą.

Dodatkowe obserwacje:

* Mainline `hp-wmi` **nie zawiera** obsługi RGB. Patche z 2023 i 2024
  („Support omen backlight control wmi-acpi methods", „4 zone keyboard rgb",
  „Add multicolor LED support…") nigdy nie weszły do drzewa jądra.
* W mainline są tylko profile termiczne, ograniczone listą płyt (`84DA`, `8572`,
  zakresy `8600–88FF`, `8900–89EB`…), do której `8D41` się nie łapie.
* Załadowany był moduł out-of-tree `hp_rgb_lighting` (autorstwa zewnętrznego
  dewelopera, `/lib/modules/<wersja>/extra/hp-rgb-lighting.ko.xz`, podpisany, więc
  wchodzi mimo Secure Boot). Wystawia `/sys/devices/platform/hp-rgb-lighting/`
  z atrybutami `zone0`…`zone7`, `brightness`, `omen_mux`, `win_lock`. Osiem stref,
  z czego `zone0`–`zone3` aktywne (`FFFFFF`), `zone4`–`zone7` wyzerowane. Wszystkie
  dotyczą listwy, nie klawiatury.
* GUID `5FB7F034-…` jest na tym sprzęcie obsługiwany przez `hp-bioscfg`.

### 2.3 Pułapka diagnostyczna

Pierwsze sprawdzenie firmware'u dało fałszywy negatyw:

```bash
# ŹLE — gaming WMI nie siedzi w DSDT
cp /sys/firmware/acpi/tables/DSDT /tmp/dsdt.dat
iasl -d /tmp/dsdt.dat
grep -c '0x00020009' /tmp/dsdt.dsl     # → 0
```

Metody WMI są w **SSDT**, nie w DSDT. Poprawnie:

```bash
mkdir /tmp/ssdt && cd /tmp/ssdt
cp /sys/firmware/acpi/tables/SSDT* .
for f in SSDT*; do iasl -d "$f" >/dev/null 2>&1; done
grep -ohE '0x000200[0-9A-F]{2}' *.dsl | sort | uniq -c | sort -rn
```

Wynik na tym sprzęcie:

```
 34 0x00020008    (gaming / termika)
 12 0x00020009    (four zone → listwa LED)
  6 0x00020000
  5 0x0002000B    (metody TC00/TC01, transfer bufora 128 B — nie lighting)
```

**Wniosek:** brak trafień w DSDT nie dowodzi niczego. Zawsze przeszukuj wszystkie
tabele ACPI.

---

## 3. Identyfikacja właściwego urządzenia

Klawiatura w tym modelu jest podpięta po USB. Kluczowe rozróżnienie między
klawiaturą wbudowaną a czymkolwiek w stacji dokującej daje atrybut `removable`:

```bash
for u in /sys/bus/usb/devices/*; do
  [ -f "$u/idVendor" ] || continue
  printf '%s %s:%s removable=%s %s\n' "$(basename $u)" \
    "$(cat $u/idVendor)" "$(cat $u/idProduct)" \
    "$(cat $u/removable 2>/dev/null)" "$(cat $u/product 2>/dev/null)"
done
```

Wynik (fragment):

```
3-9      0d62:54bf  removable=fixed      HP Gaming Keyboard II     ← wbudowana
3-3.3.3  258a:0049  removable=removable  Gaming Keyboard           ← zewnętrzna (dok)
```

`0d62` = Darfon Electronics, typowy producent klawiatur laptopowych.
Urządzenie wystawia **pięć interfejsów HID**.

---

## 4. Analiza deskryptorów HID

```bash
for d in /sys/bus/hid/devices/*0D62:54BF*; do
  echo "### $(basename $d)"
  od -An -tx1 -v "$d/report_descriptor" | tr -d ' \n'; echo
done
```

Uwaga: `stat -c%s` na `report_descriptor` zwraca 4096 (rozmiar strony sysfs),
a nie rzeczywistą długość. Trzeba liczyć odczytane bajty.
Fedora nie ma domyślnie `xxd` — używaj `od -An -tx1 -v`.

| Interfejs | hidraw | Rozmiar | Zawartość |
|---|---|---|---|
| `1.0` | 3 | 47 B | Usage Page `0xFF00`, vendor: In/Out/Feature po 64 B |
| `1.1` | 4 | 59 B | zwykła klawiatura boot protocol |
| `1.2` | 5 | 179 B | mysz/consumer + NKRO (232 bity) + vendor `0xFF02` |
| `1.3` | 6 | 31 B | Usage Page `0xFF01`, vendor: Out 64 B, Feature 8 B |
| **`1.4`** | **7** | **292 B** | **Usage Page `0x59` — HID LampArray** |

Deskryptor interfejsu `1.4` (początek):

```
05 59   Usage Page (Lighting And Illumination)
09 01   Usage (LampArray)
a1 01   Collection (Application)
```

To standard opisany w HID Usage Tables (HUTRR84), ten sam, którego Windows używa
w Dynamic Lighting. Żadnego reverse engineeringu nie trzeba — protokół jest jawny.

### 4.1 Rozpisane raporty Feature

Wszystko little-endian, bez wyrównania, pierwszy bajt to Report ID.

**Report 1 — LampArrayAttributes (odczyt, 23 B)**

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 1 |
| 1 | u16 | LampCount |
| 3 | u32 | BoundingBoxWidthInMicrometers |
| 7 | u32 | BoundingBoxHeightInMicrometers |
| 11 | u32 | BoundingBoxDepthInMicrometers |
| 15 | u32 | LampArrayKind (1 = Keyboard) |
| 19 | u32 | MinUpdateIntervalInMicroseconds |

**Report 2 — LampAttributesRequest (zapis, 3 B)**

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 2 |
| 1 | u16 | LampId |

**Report 3 — LampAttributesResponse (odczyt, 29 B)**

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 3 |
| 1 | u16 | LampId |
| 3 | u32 | PositionXInMicrometers |
| 7 | u32 | PositionYInMicrometers |
| 11 | u32 | PositionZInMicrometers |
| 15 | u32 | UpdateLatencyInMicroseconds |
| 19 | u32 | LampPurposes (bitmapa: 1 Control, 2 Accent, 4 Branding, 8 Status, 16 Illumination, 32 Presentation) |
| 23 | u8 | RedLevelCount |
| 24 | u8 | GreenLevelCount |
| 25 | u8 | BlueLevelCount |
| 26 | u8 | IntensityLevelCount |
| 27 | u8 | IsProgrammable |
| 28 | u8 | InputBinding (HID usage klawisza) |

**Report 4 — LampMultiUpdate (zapis, 51 B)** — do 8 lampek naraz

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 4 |
| 1 | u8 | LampCount (≤ 8) |
| 2 | u8 | LampUpdateFlags (bit 0 = LampUpdateComplete) |
| 3 | u16 × 8 | LampIds |
| 19 | u8 × 32 | RGBI × 8 (Red, Green, Blue, Intensity) |

**Report 5 — LampRangeUpdate (zapis, 10 B)**

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 5 |
| 1 | u8 | LampUpdateFlags |
| 2 | u16 | LampIdStart |
| 4 | u16 | LampIdEnd |
| 6 | u8 × 4 | Red, Green, Blue, Intensity |

**Report 6 — LampArrayControl (zapis, 2 B)**

| Offset | Typ | Pole |
|---|---|---|
| 0 | u8 | Report ID = 6 |
| 1 | u8 | AutonomousMode (0 = kontrola hosta, 1 = firmware) |

### 4.2 Klucz do całej zagadki

Dopóki `AutonomousMode = 1`, kontroler klawiatury odgrywa własny efekt i **ignoruje
kolory wysyłane przez hosta**. To właśnie było fabryczne pulsowanie żółto-pomarańczowe.
Nie brakowało interfejsu — brakowało przejęcia kontroli.

---

## 5. Dostęp z userspace

Bez bibliotek, przez `ioctl` na `hidraw`:

```python
def _hidioc(nr, size):          # _IOC(WRITE|READ, 'H', nr, size)
    return (3 << 30) | (size << 16) | (ord('H') << 8) | nr

HIDIOCSFEATURE = lambda size: _hidioc(0x06, size)   # zapis Feature report
HIDIOCGFEATURE = lambda size: _hidioc(0x07, size)   # odczyt Feature report
```

Przy odczycie pierwszy bajt bufora musi zawierać numer raportu.

Wykrywanie właściwego `hidraw` (numeracja zmienia się między bootami) —
po prefiksie deskryptora:

```python
LAMPARRAY_PREFIX = bytes([0x05, 0x59, 0x09, 0x01, 0xA1, 0x01])
# /sys/class/hidraw/hidraw*/device/report_descriptor
```

---

## 6. Wyniki dla tego egzemplarza

```
lamp_count             = 120
width_um               = 342000      (342 mm)
height_um              = 125000      (125 mm)
depth_um               = 1000
kind                   = 1 (Keyboard)
min_update_interval_us = 33000       (~30 aktualizacji/s)
```

Pełne per-key: 120 adresowalnych lampek, sześć rzędów po 20, `id` od 0 do 119,
bez dziur w numeracji.

### 6.1 Błąd firmware'u: ignorowany Report 2

**Report 2 (żądanie o konkretną lampkę) jest ignorowany.** Kontroler trzyma własny
kursor i inkrementuje go przy każdym odczycie Reportu 3, niezależnie od tego,
o co pytano.

Dowód — dwa kolejne uruchomienia:

```
req    0 -> id    2      # pierwszy przebieg skończył się na id 1
req    1 -> id    3
...
req  117 -> id    7      # drugi przebieg podjął od miejsca, gdzie stanął pierwszy
req  118 -> id    8
```

**Obejście:** nie polegać na żądaniu. Czytać sekwencyjnie i ufać polu `LampId`
w odpowiedzi, aż zbierze się `LampCount` unikalnych identyfikatorów. Kursor zawija
się na końcu, więc start w dowolnym miejscu jest bezpieczny.

### 6.2 Mapa lampek

Firmware zna pełną geometrię i przypisanie klawiszy. `InputBinding` to prawdziwe
HID usages (`0x3a` = F1, `0x04` = A itd.).

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

Szczegóły: Spacja ma 5 lampek, lewy Shift 3, Enter 2. Lampki oznaczone `—`
(`InputBinding = 0x03` lub `0x00`) to diody bez przypisanego klawisza, w przerwach
między blokami. `0xe8` przy `LMeta` to klawisz OMEN.

---

## 7. Wdrożenie

### 7.1 Reguła udev — sterowanie bez roota

`/etc/udev/rules.d/99-hp-lamparray.rules`:

```
SUBSYSTEM=="hidraw", ENV{ID_USB_VENDOR_ID}=="0d62", \
  ENV{ID_USB_MODEL_ID}=="54bf", ENV{ID_USB_INTERFACE_NUM}=="04", \
  GROUP="omenkbd", MODE="0660"
```

Zawężenie do interfejsu `04` jest istotne: bez niego otwierasz też interfejsy,
przez które idą naciśnięcia klawiszy, a nie ma powodu dawać do nich dostępu.

> **POPRAWKA.** Wcześniejsza wersja tego dokumentu podawała regułę opartą na
> `ATTRS{idVendor}` + `ATTRS{idProduct}` + `ATTRS{bInterfaceNumber}` i twierdziła,
> że jest zweryfikowana. **Ta wersja nie może zadziałać.** W udev wszystkie klucze
> w formie mnogiej (`ATTRS`, `KERNELS`, `SUBSYSTEMS`, `DRIVERS`) muszą być
> spełnione przez **to samo** urządzenie nadrzędne, a te atrybuty leżą na dwóch
> różnych:
>
> ```
> .../usb3/3-9            idVendor, idProduct
> .../usb3/3-9/3-9:1.4    bInterfaceNumber
> ```
>
> Reguła jest poprawna składniowo, `udevadm verify` jej nie odrzuca, ale nigdy
> nie trafia — grupa zostaje domyślna, a proces dostaje `EACCES`. To, co
> zweryfikowano empirycznie w pierwotnej sesji, to reguła **bez** zawężenia do
> interfejsu (sam `TAG+="uaccess"` na całym `VID:PID`), i stąd wzięło się błędne
> przekonanie, że wersja zawężona też działa.
>
> Właściwości `ID_USB_*` ustawia wbudowany `usb_id` **na samym urządzeniu
> hidraw**, więc `ENV{}` nie podlega temu ograniczeniu, a `ID_USB_INTERFACE_NUM`
> zastępuje `bInterfaceNumber` z taką samą precyzją. Sprawdzenie, co reguła
> faktycznie łapie:
>
> ```bash
> udevadm info -q property -n /dev/hidraw7 | grep ID_USB
> stat -c %G /dev/hidraw7
> ```
>
> Wniosek ogólny: reguły udev **trzeba weryfikować po skutku na urządzeniu**
> (`stat -c %G`), a nie po tym, że skrypt instalacyjny przeszedł bez błędu.

Przeładowanie (samo `trigger` bez `--action=add` bywa niewystarczające dla ACL):

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

### 7.2 Usługa systemd — start i powrót z uśpienia

Firmware po wyjściu z S3 wraca do własnego efektu, więc kontrolę trzeba przejąć
ponownie.

`/etc/systemd/system/omen-kbd.service`:

```ini
[Unit]
Description=Podswietlenie klawiatury HP OMEN (HID LampArray)
After=multi-user.target suspend.target hibernate.target hybrid-sleep.target suspend-then-hibernate.target

[Service]
Type=oneshot
RemainAfterExit=yes
EnvironmentFile=/etc/omen-kbd.conf
ExecStartPre=/usr/bin/sleep 2
ExecStart=/usr/local/bin/omen-kbd all ${COLOR} --intensity ${INTENSITY}

[Install]
WantedBy=multi-user.target suspend.target hibernate.target hybrid-sleep.target suspend-then-hibernate.target
```

Mechanizm powrotu z uśpienia: jednostka jest `WantedBy=suspend.target`
**i** `After=suspend.target`. W transakcji usypiania systemd uruchamia jednostki
uporządkowane *po* `suspend.target` dopiero po zakończeniu operacji uśpienia,
czyli przy wybudzeniu. `ExecStartPre=sleep 2` daje kontrolerowi czas na inicjalizację.

`/etc/omen-kbd.conf`:

```
COLOR=FFFFFF
INTENSITY=200
```

### 7.3 Użycie

```bash
omen-kbd info                            # informacje o urządzeniu
omen-kbd map                             # mapa lampek (buduje cache)
omen-kbd all 00FF88                      # jeden kolor
omen-kbd keys W,A,S,D FF0000 --rest 101010
omen-kbd gradient FF0000 0000FF --axis x
omen-kbd preset gaming
omen-kbd wave                            # Ctrl+C kończy
omen-kbd release                         # oddaj kontrolę firmware'owi
```

Mapa jest cache'owana w `/var/cache/omen-kbd-map.json` (lub `~/.cache/`), więc
120 odczytów z firmware'u dzieje się raz.

---

## 8. Wnioski i pułapki

1. **Nazwa projektu nie mówi, czym on steruje.** `omen-rgb-keyboard` i
   `hp_rgb_lighting` obsługują na tym sprzęcie listwę na obudowie. Weryfikuj
   empirycznie, co się zapala.
2. **Sprawdzaj wszystkie tabele ACPI, nie tylko DSDT.** Gaming WMI HP siedzi w SSDT.
3. **`removable` w sysfs** to najprostszy sposób odróżnienia sprzętu wbudowanego
   od tego w stacji dokującej.
4. **Zbyt duża zgodność ze standardem bywa problemem.** OpenRGB nie wykrywa tej
   klawiatury, bo działa na sterownikach pisanych pod protokoły konkretnych
   producentów, a nie ma generycznego sterownika HID LampArray.
5. **Nie ufaj deklaracjom firmware'u.** Report 2 jest tu przyjmowany bez błędu
   i całkowicie ignorowany. Zawsze weryfikuj, czy to, o co prosiłeś, to to,
   co dostałeś.
6. **`stat` na `report_descriptor` kłamie** — zwraca rozmiar strony sysfs (4096).
7. Mainline `hp-wmi` nie ma i nigdy nie miał obsługi RGB; wszystkie patche
   odpadły w recenzji.

---

## 9. Materiały

* [HID Lighting and Illumination Page (0x59) — HUTRR84, usb.org](https://www.usb.org/sites/default/files/hutrr84_-_lighting_and_illumination_page.pdf) — specyfikacja LampArray
* [Dynamic lighting / LampArray — Microsoft Learn](https://learn.microsoft.com/en-us/windows/uwp/devices-sensors/lighting-dynamic-lamparray) — czytelny opis modelu raportów
* [xz-dev/hid-rgb-ctl](https://github.com/xz-dev/hid-rgb-ctl) — gotowe narzędzie do HID LampArray pod Linuksem
* [PATCH v4: HID: generic: add LampArray support via hid-lamparray helper](https://lkml.iu.edu/2602.2/07495.html) — patch mający wystawić takie urządzenia przez `/sys/class/leds`
* [OmenLinux/omen-rgb-keyboard](https://github.com/OmenLinux/omen-rgb-keyboard) — stałe WMI four-zone
* [PATCH v3: HP: wmi: added support for 4 zone keyboard rgb](https://lkml.iu.edu/hypermail/linux/kernel/2407.0/08119.html) — nieprzyjęty patch four-zone
* [drivers/platform/x86/hp/hp-wmi.c](https://github.com/torvalds/linux/blob/master/drivers/platform/x86/hp/hp-wmi.c) — mainline, bez obsługi RGB
