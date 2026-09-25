# Co zostało zmienione w systemie — omen-kbd

Ten dokument opisuje **wszystko, co instalacja `omen-kbd` zrobiła poza katalogiem
projektu**: jakie konta, pliki, usługi i reguły powstały w systemie, po co, oraz
jak to w całości wycofać. Jest pisany z myślą o tym, że ktoś (Ty za pół roku,
albo ktoś inny) będzie chciał zrozumieć stan maszyny bez przegrzebywania całej
historii rozmowy.

Stan opisany tutaj sprawdzony na żywo **28.08.2026** na Fedorze 44. Jeśli coś
się nie zgadza z rzeczywistością — system mógł się zmienić od tego czasu;
komendy diagnostyczne w sekcji „Jak sprawdzić" pokażą stan faktyczny.

---

## 1. W skrócie

| | |
|---|---|
| Co to jest | Sterowanie per-key RGB klawiatury HP OMEN MAX 16 (HID LampArray) |
| Jak działa | Demon jako usługa systemowa (`omenkbd`) + CLI/GUI/tray jako Ty |
| Gdzie kod | `/home/rj/Apps/OmenRgbKeyboard/` (to repozytorium) + kopia w `/usr/local/lib/omen-kbd/` |
| Nowe konto | `omenkbd` — użytkownik systemowy, bez powłoki logowania |
| Nowe grupy | `omenkbd` (Ty należysz), `omenkbd-input` (tylko demon) |
| Root dotknięty | tak — patrz sekcja 4 |
| Reactive typing | włączony, czyta tylko wbudowaną klawiaturę |
| Jak odinstalować | `bash packaging/uninstall.sh` — patrz sekcja 9 |

---

## 2. Dlaczego w ogóle demon jako osobny użytkownik

To jest decyzja, która tłumaczy większość poniższych zmian, więc krótkie
uzasadnienie na start. Powiedziałeś wprost: ufasz kodowi, który przejrzałeś,
ale chcesz, żeby **inne aplikacje** na Twoim koncie nie miały dostępu do tego,
co robi ta.

Konkretnie chodziło o reactive typing (klawisz świeci pod palcem) — a to jest
z definicji odczyt naciśnięć klawiszy, czyli keylogger. Gdyby demon działał
jako `rj` (Twoje konto), **każda** aplikacja uruchomiona jako `rj` mogłaby
sięgnąć do tego samego urządzenia i podsłuchać, co piszesz — hasła w terminalu,
w przeglądarce, wszędzie. Dlatego demon dostał **własne konto systemowe** i
dostęp do klawiszy ma tylko ono. Twoje aplikacje (w tym CLI i GUI tej samej
apki) rozmawiają z demonem przez gniazdo — mogą zmieniać kolory, ale nie mają
wglądu w to, co naciskasz.

---

## 3. Kto uruchamia co, jako kto, i kiedy

To jest model wykonania w skrócie — który proces działa jako który
użytkownik, kto go startuje i w jakim momencie. Warto to mieć jasno, bo
właśnie z pomieszania „jako kto" wzięła się większość problemów opisanych w
sekcji 8.

| Proces | Jednostka / sposób startu | Jako kto | Kiedy startuje |
|---|---|---|---|
| Demon (`omen-kbd-daemon`) | `omen-kbd.service` (systemd **systemowy**) | `omenkbd` | przy **boocie** maszyny, niezależnie od logowania |
| Tray (`omen-kbd-gui --hidden`) | `omen-kbd-tray.service` (systemd **użytkownika**, `rj`) | `rj` | przy **logowaniu** do sesji graficznej |
| CLI (`omen-kbd ...`) | ręcznie, z terminala | `rj` (Ty) | na żądanie, kiedy wpiszesz komendę |
| Okno GUI (`omen-kbd-gui`) | z menu aplikacji albo z traya | `rj` (Ty) | na żądanie |
| Hook wybudzenia | `/usr/lib/systemd/system-sleep/omen-kbd`, wołany przez `systemd-logind` | `root` | po każdym wybudzeniu z S3 |

### Dlaczego to jest rozbite na dwie różne jednostki systemd

**Demon jest jednostką systemową** (`/etc/systemd/system/`), nie
użytkownika, celowo — żeby wstawał **przed** zalogowaniem się na komputer, i
klawiatura świeciła już na ekranie logowania, a nie dopiero po wpisaniu
hasła. Systemowe jednostki nie potrzebują żadnej sesji użytkownika, żeby
działać — dlatego demon dostał **własne konto** (`omenkbd`), a nie chodzi
jako `root` (zbędne uprawnienia) ani jako `rj` (wymagałoby Twojej sesji i
łamałoby izolację opisaną w sekcji 2).

**Tray jest jednostką użytkownika** (`~/.config/systemd/user/`), bo ikona w
zasobniku systemowym z natury wymaga działającej sesji graficznej — nie ma
sensu jej uruchamiać, zanim ktokolwiek się zaloguje. Chodzi jako `rj`, bo to
zwykły klient demona: łączy się do gniazda `/run/omen-kbd/omen-kbd.sock`
dokładnie tak samo jak CLI, nie ma żadnych specjalnych uprawnień.

**Hook wybudzenia chodzi jako `root`**, bo tak działają wszystkie skrypty w
`/usr/lib/systemd/system-sleep/` — to wymóg mechanizmu `systemd-logind`, nie
nasza decyzja. Skrypt nie robi nic uprzywilejowanego poza tym: budzi się,
wysyła jedną komendę `resume` do gniazda demona (dokładnie to samo, co zrobiłby
`omen-kbd resume` z linii poleceń) i kończy działanie.

### Jak to wygląda w praktyce po starcie systemu

```
BOOT
 │
 ├─ systemd (system) startuje omen-kbd.service
 │    └─ proces jako omenkbd, otwiera /dev/hidraw7, zaklada /run/omen-kbd/omen-kbd.sock
 │       (klawiatura swieci na ekranie logowania)
 │
LOGOWANIE (rj)
 │
 ├─ systemd --user (sesja rj) startuje omen-kbd-tray.service
 │    └─ proces jako rj, laczy sie do gniazda demona, pokazuje ikone
 │
 └─ Ty w terminalu: `omen-kbd effect fire`
      └─ krotkotrwaly proces jako rj, wysyla jedna komende do gniazda, konczy sie
```

Żaden z tych procesów **nie musi** znać drugiego bezpośrednio — wszystkie
rozmawiają wyłącznie przez gniazdo unixowe. Możesz zabić GUI, zamknąć
terminal, wylogować się — demon (i światło na klawiaturze) działa dalej,
bo żyje w zupełnie innym drzewie procesów (systemowym, nie sesji użytkownika).

## 4. Zmiany w systemie (poza repozytorium)

### 4.1 Konto i grupy

```bash
useradd --system --gid omenkbd --no-create-home --shell /sbin/nologin omenkbd
groupadd --system omenkbd
groupadd --system omenkbd-input
usermod -aG omenkbd rj                # Ty — dostęp do gniazda demona
usermod -aG omenkbd-input omenkbd     # tylko demon — dostęp do klawiszy
```

Stan faktyczny:

```
uid=958(omenkbd) gid=957(omenkbd) grupy=957(omenkbd),956(omenkbd-input)
omenkbd:x:957:rj
omenkbd-input:x:956:omenkbd
```

`rj` **nie** jest w `omenkbd-input` — to jest sedno całej izolacji. Konto
`omenkbd` nie ma powłoki logowania (`/sbin/nologin`) i nie ma katalogu
domowego — istnieje wyłącznie po to, żeby uruchamiać ten jeden proces.

### 4.2 Pliki systemowe (właściciel `root`)

| Ścieżka | Co to |
|---|---|
| `/etc/systemd/system/omen-kbd.service` | jednostka demona |
| `/etc/udev/rules.d/99-hp-lamparray.rules` | dostęp do diod (`/dev/hidraw*`) |
| `/etc/udev/rules.d/99-hp-lamparray-input.rules` | dostęp do klawiszy (`/dev/input/*`) |
| `/usr/lib/systemd/system-sleep/omen-kbd` | hook: przejęcie kontroli po wybudzeniu |
| `/usr/local/bin/omen-kbd` | CLI — cienki wrapper wołający Python |
| `/usr/local/bin/omen-kbd-daemon` | uruchamia demona |
| `/usr/local/bin/omen-kbd-gui` | uruchamia GUI/tray |
| `/usr/local/lib/omen-kbd/` | **kopia kodu** z tego repozytorium (patrz 4.5) |
| `/usr/local/share/doc/omen-kbd/` | kopie README i dokumentacji |
| `/usr/local/share/applications/omen-kbd.desktop` | wpis w menu aplikacji |
| `/usr/local/share/icons/hicolor/scalable/apps/omen-kbd.svg` | ikona |

### 4.3 Pliki użytkownika (właściciel `rj`)

| Ścieżka | Co to |
|---|---|
| `~/.config/systemd/user/omen-kbd-tray.service` | jednostka traya (per-użytkownik, bo ikona wymaga sesji graficznej) |

### 4.4 Katalogi danych demona (właściciel `omenkbd`)

| Ścieżka | Co trzyma | Tryb |
|---|---|---|
| `/run/omen-kbd/` | gniazdo unixowe (`omen-kbd.sock`) | `0750`, grupa `omenkbd` |
| `/var/lib/omen-kbd/` | ostatni stan, profile (`state.json`, `reactive.json`, `profiles/`) | `0755` |
| `/var/cache/omen-kbd/` | cache mapy lampek (żeby nie czytać 120 lampek z firmware'u przy każdym starcie) | `0755` |

Same katalogi dostaje demon od `systemd` przez `RuntimeDirectory=`,
`StateDirectory=`, `CacheDirectory=` w jednostce — nie trzeba ich tworzyć
ręcznie, systemd robi to przy starcie usługi i usuwa `/run/omen-kbd` przy jej
zatrzymaniu.

### 4.5 Skąd bierze się `/usr/local/lib/omen-kbd`

To jest **kopia** katalogu `omenkbd/` z tego repozytorium, zrobiona przez
instalator (`cp -r`). Kod demona faktycznie uruchamiany w systemie pochodzi
stąd, nie z `~/Apps/OmenRgbKeyboard` bezpośrednio. Konsekwencja: **zmiana kodu
w repozytorium nie ma efektu, dopóki nie uruchomisz instalatora ponownie** —
`bash packaging/install.sh` nadpisuje tę kopię i restartuje usługę.

### 4.6 Pakiet systemowy

```
python3-pyside6-6.11.1-4.fc44.x86_64
```

Zainstalowany przez `dnf` w ramach instalacji (GUI potrzebuje Qt6). Jedyna
rzecz, której `uninstall.sh` **nie** usuwa automatycznie — patrz sekcja 9.

### 4.7 Jednostki systemd — stan

```
omen-kbd.service        enabled, active   (system)
omen-kbd-tray.service   enabled, active   (user, sesja rj)
```

---

## 5. Reguły udev — co dokładnie robią

### 5.1 Dostęp do diod (`99-hp-lamparray.rules`)

Zawężona do **jednego konkretnego interfejsu USB** tej klawiatury (interfejs
`04` — LampArray), nie do całego urządzenia. Pozostałe cztery interfejsy tego
samego `VID:PID` (przez które lecą naciśnięcia klawiszy, ruch myszki
wskaźnikowej itd.) pozostają poza tą regułą.

Efekt: `/dev/hidraw7` (numer może się zmienić po restarcie/przepięciu doku —
kod wykrywa urządzenie po deskryptorze, nie po numerze) należy do grupy
`omenkbd`, tryb `0660`.

### 5.2 Dostęp do klawiszy (`99-hp-lamparray-input.rules`)

Zawężona do tego samego `VID:PID` **i** do urządzeń faktycznie przenoszących
pisanie (`ID_INPUT_KEYBOARD=1`) — z sześciu urządzeń wejściowych tej
klawiatury tylko dwa (`event13`, `event18`) łapią się na tę regułę; wskaźnik
dotykowy, klawisze multimedialne i sterowanie radiem zostają poza nią.

Efekt: te dwa węzły należą do grupy `omenkbd-input`, tryb `0640`. Reszta
klawiatur w systemie (BY Tech, Compx, Bluetooth) nie jest tą regułą objęta —
reactive świeci tylko po tym, co naciskasz na **wbudowanej** klawiaturze.

---

## 6. Jak sprawdzić, że wszystko działa

```bash
# stan usługi
systemctl status omen-kbd.service
systemctl --user status omen-kbd-tray.service

# czy demon odpowiada
omen-kbd status

# kto ma dostęp do czego
getent group omenkbd omenkbd-input
stat -c '%U:%G %A' /dev/hidraw7          # numer moze byc inny
getfacl -p /dev/hidraw7                  # patrz sekcja 8.2 — ACL potrafi klamac

# dziennik demona
journalctl -u omen-kbd.service -n 40 --no-pager
```

### 6.1 Ważne zastrzeżenie: grupa `input`

Sprawdzone na tym koncie: `rj` należy do grupy systemowej **`input`**
(niezależnie od `omen-kbd`, prawdopodobnie z jakiegoś innego programu). Ta
grupa daje dostęp do **wszystkich** urządzeń wejściowych w systemie, każdemu
procesowi działającemu jako `rj`. `omen-kbd` z niej nie korzysta i nigdy Cię
do niej nie dopisał — ale jeśli kiedyś zależy Ci na pełnej izolacji (żeby
*żadna* Twoja aplikacja nie miała globalnego dostępu do klawiatur), warto
wiedzieć, że ta grupa istnieje niezależnie:

```bash
id -nG | tr ' ' '\n' | grep -x input && echo "jestes w grupie input"
```

Wypisanie się: `sudo gpasswd -d rj input`, potem przelogowanie. To nie jest
coś, co `omen-kbd` powinien ruszać automatycznie — to Twoja decyzja.

---

## 7. Reactive typing — co realnie czyta

Demon (jako `omenkbd`) otwiera `/dev/input/event13` i `/dev/input/event18` —
**wyłącznie** te dwa węzły, wyłącznie gdy reactive jest włączony
(`omen-kbd reactive on`). Surowy kod naciśniętego klawisza zamienia się od
razu na `{lamp_id: znacznik_czasu}` i nic poza tym nie zostaje — brak logów,
brak bufora historii nacisnięć. Żadna komenda demona (w tym `status`) nie
zwraca tego stanu przez gniazdo — jest to pilnowane testami automatycznymi
(`test/test_resilience.py::TestNoKeystrokeSideChannel`).

Wyłączenie samego dostępu do klawiszy, bez ruszania reszty:

```bash
sudo rm /etc/udev/rules.d/99-hp-lamparray-input.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=input
omen-kbd reactive off
```

---

## 8. Historia debugowania — napotkane problemy i jak zostały rozwiązane

Ta sekcja jest tu **specjalnie**, żeby te same błędy nie musiały być
odkrywane drugi raz. Każdy z nich naprawdę wystąpił podczas tej instalacji.

### 8.1 Reguła udev z `ATTRS{}` nigdy nie trafiała

**Objaw:** demon w pętli logował `EACCES` mimo poprawnie napisanej (i
przechodzącej `udevadm verify`) reguły łączącej `ATTRS{idVendor}`,
`ATTRS{idProduct}` i `ATTRS{bInterfaceNumber}`.

**Przyczyna:** w udev wszystkie klucze w formie mnogiej (`ATTRS`, `KERNELS`,
`SUBSYSTEMS`, `DRIVERS`) muszą być spełnione przez **to samo** urządzenie
nadrzędne w hierarchii sysfs. `idVendor`/`idProduct` leżą na jednym poziomie
drzewa (`.../usb3/3-9`), a `bInterfaceNumber` na innym
(`.../usb3/3-9/3-9:1.4`). Taka reguła jest składniowo poprawna i nigdy się
nie dopasowuje.

**Naprawa:** dopasowanie przez `ENV{ID_USB_VENDOR_ID}`, `ENV{ID_USB_MODEL_ID}`,
`ENV{ID_USB_INTERFACE_NUM}` — te właściwości ustawia `usb_id` **na samym
urządzeniu**, więc problem wspólnego rodzica nie występuje.

**Wniosek na przyszłość:** regułę udev weryfikuje się po **skutku na
urządzeniu** (`stat -c %G`, `getfacl`), nigdy po tym, że `udevadm verify`
przeszedł bez błędu.

### 8.2 POSIX ACL przesłaniała poprawną grupę

**Objaw:** reguła udev już trafiała (grupa na `/dev/hidraw7` była poprawnie
`omenkbd`), a demon nadal dostawał `EACCES`.

**Przyczyna:** na węźle wisiała ACL pozostała po wcześniejszej regule z
`uaccess`:
```
user::rw-   user:rj:rw-   group::---   mask::rw-   other::---
```
`ls -l` pokazywał `crw-rw---- root omenkbd` — ale to `rw-` w miejscu grupy
jest **maską ACL**, nie uprawnieniem grupy. Faktyczny wpis `group::` był
pusty. `chmod` z reguły udev zmienia właśnie maskę, nie ten wpis — problem
sam się nie naprawiał.

**Naprawa:** reguły dokładają jawny wpis ACL dla grupy:
```
RUN+="/usr/bin/setfacl -m g:omenkbd:rw $env{DEVNAME}"
```

**Wniosek na przyszłość:** jeśli grupa na urządzeniu jest poprawna, a mimo to
`EACCES`, sprawdź `getfacl -p <urządzenie>` — szczególnie `group::`.

### 8.3 Weryfikacja instalatora dawała fałszywy alarm

Skoro sam `stat -c %G` nie wykrywa problemu z ACL (bo grupa faktycznie jest
poprawna), instalator przestał sprawdzać nazwę grupy i zaczął **faktycznie
otwierać urządzenie jako `omenkbd`** (`runuser -u omenkbd -- python3 -c
'os.open(...)'`). Przy niepowodzeniu wypisuje pełne `getfacl` i rozróżnia
przyczynę (ACL vs zła reguła).

### 8.4 Gniazdo miało tryb `0600`

**Objaw:** demon działał, urządzenie było otwarte, ale `omen-kbd status`
(nawet z prawidłową grupą w sesji) dostawał `PermissionError` przy `connect()`
do gniazda.

**Przyczyna:** kod gniazda pochodził jeszcze z wcześniejszego modelu (demon
jako usługa *użytkownika*), gdzie `chmod(sock, 0o600)` miał sens — tylko
właściciel miał je czytać. Po przejściu na demona-jako-osobnego-użytkownika
ten tryb odcinał **każdego**, łącznie z członkami grupy `omenkbd`.

**Naprawa:** `chmod(sock, 0o660)`. Granica uprawnień jest na katalogu
`/run/omen-kbd` (`0750`, grupa `omenkbd`), nie na samym gnieździe.

### 8.5 Klient szukał gniazda w złym miejscu

**Objaw:** nawet po naprawie trybu gniazda, klient czasem meldował „demon nie
działa", mimo że usługa była aktywna.

**Przyczyna:** wybór ścieżki gniazda sprawdzał `os.path.exists()` na **samym
pliku gniazda**. Katalog nadrzędny (`/run/omen-kbd`) ma tryb `0750` — dla
kogoś bez grupy `omenkbd` w bieżącej sesji `exists()` na czymkolwiek w środku
zwraca `False`, bo nie da się nawet zajrzeć do katalogu. Klient szedł dalej,
nie znajdował nic sensownego i mylił brak uprawnień z brakiem demona.

**Naprawa:** sprawdzanie istnienia **katalogu** `/run/omen-kbd`, nie pliku
gniazda w środku.

### 8.6 Brak uprawnień do gniazda dawał goły traceback

`PermissionError` przy `connect()` nie był w ogóle obsłużony w kliencie —
leciał surowy traceback Pythona zamiast czytelnego komunikatu. Do tego próba
automatycznego wystartowania demona (sensowna reakcja na „demona nie ma")
przesłaniała trafną diagnozę swoim ogólnym komunikatem, gdy demon *był*
uruchomiony, tylko sesja nie miała jeszcze grupy.

**Naprawa:** osobny typ wyjątku (`SocketPermission`) dla „demon działa, ale
nie masz uprawnień do gniazda" — autostart się przy nim nie uruchamia, a
komunikat podpowiada `sg omenkbd -c "omen-kbd status"` jako obejście bez
wylogowywania.

### 8.7 Grupy przypisują się przy logowaniu, nie od razu

To nie jest błąd w kodzie, tylko właściwość Linuksa, która kilka razy
wyglądała jak awaria: `usermod -aG omenkbd rj` dopisuje grupę **w bazie**
natychmiast, ale bieżąca sesja terminala/graficzna (uruchomiona wcześniej)
nadal ma stary zestaw grup w pamięci procesu. Trzeba się faktycznie
przelogować (wylogować i zalogować ponownie — sam restart usługi nic nie
zmienia), żeby nowa sesja odziedziczyła zaktualizowaną grupę.

Obejście bez przelogowania: `sg omenkbd -c 'polecenie'` — podnosi grupę tylko
dla jednej powłoki, nie dla całej sesji (więc GUI/tray uruchomione w starej
sesji dalej nie zobaczą gniazda, dopóki się nie przelogujesz naprawdę).

### 8.8 Demon zasypywał dziennik tym samym błędem

Zanim to naprawiono, każda nieudana próba połączenia z urządzeniem (co ~5 s w
pętli ponawiania) logowała pełny `ERROR`. Po godzinie dziennik miał setki
identycznych linii, w których trudno było znaleźć coś nowego.

**Naprawa:** demon loguje dany komunikat błędu tylko **przy zmianie** — kolejne
identyczne niepowodzenia idą po cichu, a komunikat wraca, gdy problem się
zmieni albo zniknie i pojawi ponownie.

### 8.9 Wyciek stanu testowego do prawdziwej konfiguracji

Podczas pisania testów dla reactive typing jedna ścieżka konfiguracji
(`REACTIVE_PATH`) nie była odizolowana od reszty (`STATE_PATH`,
`PROFILE_DIR` już były). Ręczne testowanie zapisało `enabled: true` do
**prawdziwego** `~/.config/omen-kbd/reactive.json`, co potem powodowało, że
*każdy* test demona w całym pakiecie (nie tylko testy reactive) dziedziczył
ten stan — łącznie z testami zupełnie niezwiązanymi z tą funkcją.

**Naprawa:** pełna izolacja ścieżek konfiguracyjnych w harnessie testowym +
skasowanie wyciekniętego pliku. Czysto teoretyczna lekcja: gdy dodajesz nową
ścieżkę stanu, sprawdź, czy testy ją izolują tak samo jak istniejące.

---

## 9. Jak odinstalować

Z katalogu repozytorium, **bez** `sudo` z przodu (skrypt sam poprosi o hasło
raz):

```bash
bash packaging/uninstall.sh
```

Co to robi, w kolejności:

1. Oddaje kontrolę firmware'owi klawiatury (wraca fabryczne pulsowanie).
2. Wyłącza i usuwa `omen-kbd-tray.service` (Twoja sesja) oraz
   `omen-kbd.service` (systemowa).
3. Usuwa **obie** reguły udev (`99-hp-lamparray.rules` i
   `99-hp-lamparray-input.rules`) — po tym `/dev/hidraw*` i
   `/dev/input/event*` wracają do stanu „tylko root".
4. Usuwa hook wybudzenia (`/usr/lib/systemd/system-sleep/omen-kbd`).
5. Usuwa pliki programu (`/usr/local/lib/omen-kbd`, `/usr/local/bin/omen-kbd*`,
   wpis w menu, ikonę).
6. Usuwa stan, profile i cache (`/var/lib/omen-kbd`, `/var/cache/omen-kbd`) —
   chyba że podasz `--keep-config`.
7. Usuwa konto `omenkbd` i grupy `omenkbd`, `omenkbd-input`.

### Czego `uninstall.sh` **nie** rusza

- **`python3-pyside6`** — mogłeś go chcieć niezależnie od tej apki.
  Ręcznie: `sudo dnf remove python3-pyside6`
- **Grupa `input`** dla `rj` — to nie jest coś, co ta apka założyła (patrz
  sekcja 6.1), więc nie jest to naprawiane przy odinstalowaniu.
- **To repozytorium** (`~/Apps/OmenRgbKeyboard`) — zostaje na dysku, to Twój
  kod źródłowy, nie coś do sprzątnięcia.

### Opcje

```bash
bash packaging/uninstall.sh --keep-config   # zostaw profile i cache
bash packaging/uninstall.sh --legacy        # dodatkowo usun slady bardzo starego prototypu
```

---

## 10. Gdzie szukać więcej

| Plik | Co zawiera |
|---|---|
| [README.md](README.md) | pełna dokumentacja: instalacja, architektura, tryby świecenia, testy |
| [BRIEF-omen-rgb-app.md](BRIEF-omen-rgb-app.md) | specyfikacja protokołu HID LampArray |
| [omen-max-8d41-keyboard-rgb.md](omen-max-8d41-keyboard-rgb.md) | historia reverse-engineeringu protokołu (ślepe uliczki, pułapki sprzętowe) |
| `packaging/*.rules`, `packaging/*.service` | dokładna treść reguł i jednostek — z komentarzami wyjaśniającymi każdą decyzję |
| `test/test_resilience.py` | testy pilnujące m.in. braku kanału bocznego przez gniazdo |
