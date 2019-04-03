use libcitadel::{Result,RealmFS};
use crossbeam_channel::Sender;
use cursive::{CbFunc, Cursive};
use std::sync::Arc;
use std::thread;
use cursive::event::EventResult;
use crate::dialogs::confirm_dialog;
use crate::ui::{DeferredAction, GlobalState};
use cursive::views::Dialog;
use crate::item_list::ItemList;
use crate::realmfs::fork_dialog::ForkDialog;
use crate::notes::NotesDialog;

type ActionCallback = Fn(&RealmFS)+Send+Sync;

#[derive(Clone)]
pub struct RealmFSAction {
    realmfs: RealmFS,
    sink: Sender<Box<CbFunc>>,
    callback: Arc<ActionCallback>
}

impl RealmFSAction {

    pub fn activate_realmfs(activated: bool) -> EventResult {
        if activated {
            return Self::deactivate_realmfs(activated);
        }
        Self::action(|r| {
            Self::log_fail("activating realmfs", || r.activate());
        })
    }

    pub fn deactivate_realmfs(activated: bool) -> EventResult {
        if !activated {
            return EventResult::Consumed(None);
        }

        EventResult::with_cb(|s| {
            let action = RealmFSAction::new(s, Arc::new(|r| {
                Self::log_fail("deactivating realmfs", || r.deactivate());
            }));

            if action.realmfs.is_in_use() {
                s.add_layer(Dialog::info("RealmFS is in use and cannot be deactivated").title("Cannot Deactivate"));
                return;
            }

            let title = "Deactivate RealmFS?";
            let msg = format!("Would you like to deactivate RealmFS '{}'?",action.realmfs.name());
            let dialog = confirm_dialog(title, &msg, move |_| action.run_action());
            s.add_layer(dialog);
        })
    }

    pub fn autoupdate_realmfs() -> EventResult {
        EventResult::Consumed(None)
    }

    pub fn autoupdate_all() -> EventResult {
        EventResult::Consumed(None)
    }

    pub fn resize_realmfs() -> EventResult {
        EventResult::Consumed(None)
    }

    pub fn seal_realmfs(sealed: bool) -> EventResult {
        if sealed {
            return EventResult::Consumed(None);
        }

        EventResult::with_cb(|s| {
            let action = RealmFSAction::new(s, Arc::new(|r| {
                Self::log_fail("sealing realmfs", || r.seal(None));
            }));
            if action.realmfs.is_sealed() {
                return;
            }
            if action.realmfs.is_activated() {
                s.add_layer(Dialog::info("Cannot seal realmfs because it is currently activated. Deactivate first").title("Cannot Seal"));
                return;
            }
            if !action.realmfs.has_sealing_keys() {
                s.add_layer(Dialog::info("Cannot seal realmfs because no keys are available to sign image.").title("Cannot Seal"));
                return;
            }
            let title = "Seal RealmFS?";
            let msg = format!("Would you like to seal RealmFS '{}'?", action.realmfs.name());
            let dialog = confirm_dialog(title, &msg, move |_| action.run_action());
            s.add_layer(dialog);
        })
    }

    pub fn unseal_realmfs(sealed: bool) -> EventResult {
        if !sealed {
            return EventResult::Consumed(None);
        }
        let title = "Unseal RealmFS?";
        let msg = "Do you want to unseal '$REALMFS'";

        Self::confirm_action(title, msg, |r| {
            Self::log_fail("unsealing realmfs", || r.unseal());
        })
    }

    pub fn delete_realmfs(user: bool) -> EventResult {
        if !user {
            return EventResult::Consumed(None);
        }
        let title = "Delete RealmFS?";
        let msg = "Are you sure you want to delete '$REALMFS'?";

        let cb = Self::wrap_callback(|r| {
            let manager = r.manager();
            if let Err(e) = manager.delete_realmfs(r) {
                warn!("error deleting realmfs: {}", e);
            }
        });

        EventResult::with_cb(move |s| {
            let action = RealmFSAction::new(s, cb.clone());
            let message = msg.replace("$REALMFS", action.realmfs.name());
            let dialog = confirm_dialog(title, &message, move |s| {
                if action.realmfs.is_in_use() {
                    s.add_layer(Dialog::info("Cannot delete RealmFS because it is currently in use.").title("Cannot Delete"));
                } else {
                    action.run_action()
                }
            });
            s.add_layer(dialog);
        })
    }

    pub fn fork_realmfs() -> EventResult {
        EventResult::with_cb(move |s| {
            let realmfs = RealmFSAction::current_realmfs(s);
            ForkDialog::open(s, realmfs);
        })
    }

    pub fn update_realmfs() -> EventResult {
        EventResult::with_cb(move |s| {
            let realmfs = Self::current_realmfs(s);
            if Self::confirm_sealing_keys(s, &realmfs) {
                Self::defer_realmfs_update(s, realmfs);
            }
        })
    }

    pub fn confirm_sealing_keys(s: &mut Cursive, realmfs: &RealmFS) -> bool {
        if !realmfs.has_sealing_keys() {
            let dialog = Dialog::info(format!("Cannot update {}-realmfs.img because no sealing keys are available", realmfs.name()))
                .title("No sealing keys");
            s.add_layer(dialog);
            return false;
        }
        true
    }

    pub fn defer_realmfs_update(s: &mut Cursive, realmfs: RealmFS)  {
        let deferred = DeferredAction::UpdateRealmFS(realmfs);
        s.with_user_data(|gs: &mut GlobalState| gs.set_deferred(deferred));
        s.quit();
    }

    pub fn edit_notes() -> EventResult {

        EventResult::with_cb(|s| {
            let realmfs = Self::current_realmfs(s);
            let desc = format!("{}-realmfs.img", realmfs.name());
            let notes = realmfs.notes().unwrap_or_default();
            NotesDialog::open(s, &desc, notes, move |s, notes| {
                if let Err(e) = realmfs.save_notes(notes) {
                    warn!("error saving notes file for {}-realmfs.img: {}", realmfs.name(), e);
                }
                ItemList::<RealmFS>::call_reload("realmfs", s);
            });

        })

    }

    fn log_fail<F,R>(msg: &str, f: F) -> bool
        where F: Fn() -> Result<R>
    {
        if let Err(e) = f() {
            warn!("error {}: {}", msg, e);
            false
        } else {
            true
        }
    }

    pub fn action<F>(callback: F) -> EventResult
        where F: Fn(&RealmFS), F: 'static + Send+Sync,
    {
        EventResult::with_cb({
            let callback = Arc::new(callback);
            move |s| {
                let action = RealmFSAction::new(s, callback.clone());
                action.run_action();
            }
        })
    }

    fn wrap_callback<F>(callback: F) -> Arc<ActionCallback>
        where F: Fn(&RealmFS), F: 'static + Send + Sync,
    {
        Arc::new(callback)
    }

    pub fn confirm_action<F>(title: &'static str, message: &'static str, callback: F) -> EventResult
        where F: Fn(&RealmFS), F: 'static + Send+Sync,
    {
        let callback = Arc::new(callback);

        EventResult::with_cb(move |s| {
            let action = RealmFSAction::new(s, callback.clone());
            let message = message.replace("$REALMFS", action.realmfs.name());
            let dialog = confirm_dialog(title, &message, move |_| action.run_action());
            s.add_layer(dialog);
        })
    }

    fn new(s: &mut Cursive, callback: Arc<ActionCallback>) -> RealmFSAction {
        let realmfs = Self::current_realmfs(s);
        let sink = s.cb_sink().clone();
        RealmFSAction { realmfs, sink, callback }
    }

    fn current_realmfs(s: &mut Cursive) -> RealmFS {
        ItemList::<RealmFS>::call("realmfs", s, |v| v.selected_item().clone())
    }

    fn run_action(&self) {
        let action = self.clone();
        thread::spawn(move || {
            (action.callback)(&action.realmfs);
            action.sink.send(Box::new(Self::update)).unwrap();
        });
    }

    fn update(s: &mut Cursive) {
        ItemList::<RealmFS>::call_reload("realmfs", s);
    }
}
