//! One offline prompt per window; recovery dismisses the stale prompt.
use crate::daemon_client::{self, DaemonStatus};
use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn bind(window: &adw::ApplicationWindow) {
    let window = window.downgrade();
    let pending: Rc<RefCell<Option<adw::MessageDialog>>> = Rc::new(RefCell::new(None));
    daemon_client::subscribe_daemon_status(move |status| {
        if status == DaemonStatus::Online {
            // Release the borrow before close(), which can emit a response.
            let dialog = pending.borrow_mut().take();
            if let Some(dialog) = dialog {
                dialog.close();
            }
            return;
        }
        if pending.borrow().is_some() {
            return;
        }
        let Some(window) = window.upgrade() else { return; };
        let dialog = adw::MessageDialog::new(
            Some(&window),
            Some(crate::i18n::t("daemon_offline_title")),
            Some(crate::i18n::t("daemon_offline_msg")),
        );
        dialog.add_response("cancel", crate::i18n::t("btn_cancel"));
        dialog.add_response("restart", crate::i18n::t("btn_restart"));
        dialog.set_response_appearance("restart", adw::ResponseAppearance::Suggested);
        let state = Rc::downgrade(&pending);
        dialog.connect_response(None, move |dialog, response| {
            if let Some(state) = state.upgrade() {
                state.borrow_mut().take();
            }
            if response == "restart" {
                // Waiting for authentication and systemd must not block GTK.
                daemon_client::get_runtime().spawn(async {
                    if tokio::process::Command::new("pkexec")
                        .args(["systemctl", "restart", "omen-space-daemon"])
                        .status()
                        .await
                        .is_err()
                    {
                        let _ = tokio::process::Command::new("systemctl")
                            .args(["restart", "omen-space-daemon"])
                            .status()
                            .await;
                    }
                });
            }
            dialog.close();
        });
        *pending.borrow_mut() = Some(dialog.clone());
        dialog.present();
    });
}
