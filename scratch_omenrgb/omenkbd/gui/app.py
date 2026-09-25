"""Punkt wejscia GUI.

Aplikacja zyje w trayu: zamkniecie okna je chowa, nie konczy procesu.
Demon jest calkowicie niezalezny — zabicie GUI nie gasi podswietlenia.
"""

import sys


def main(argv=None):
    from .. import i18n

    try:
        from PySide6.QtWidgets import QApplication, QMessageBox, QSystemTrayIcon
    except ImportError:
        sys.stderr.write(i18n.t('app.needs_pyside6'))
        return 1

    from ..client import Client, NoDaemon
    from .icon import app_icon
    from .tray import Tray
    from .window import MainWindow

    app = QApplication(sys.argv if argv is None else argv)
    app.setApplicationName('omen-kbd')
    app.setApplicationDisplayName(i18n.t('window.title'))
    app.setDesktopFileName('omen-kbd')
    # Zamkniecie okna nie moze konczyc procesu — zyjemy w trayu.
    app.setQuitOnLastWindowClosed(False)

    icon = app_icon()
    app.setWindowIcon(icon)

    client = Client()
    try:
        client.call('ping')
    except NoDaemon as e:
        QMessageBox.critical(
            None, i18n.t('window.title'),
            i18n.t('app.cannot_reach_daemon', err=e))
        return 1

    window = MainWindow(client)
    tray = None
    if QSystemTrayIcon.isSystemTrayAvailable():
        tray = Tray(icon, window, client, app)
        tray.show()
    else:
        # Bez traya zamkniecie okna musi konczyc program, inaczej zostaje
        # niewidoczny proces bez sposobu na dotarcie do niego.
        window.quitting = True
        app.setQuitOnLastWindowClosed(True)

    start_hidden = '--hidden' in (argv or sys.argv) and tray is not None
    if not start_hidden:
        window.show()

    return app.exec()


if __name__ == '__main__':
    sys.exit(main())
