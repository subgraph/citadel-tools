use libcitadel::{Realm, RealmManager, Result, RealmFS};
use crossbeam_channel::Sender;
use std::sync::Arc;
use cursive::{CbFunc, Cursive};
use cursive::event::{EventResult};
use std::thread;
use crate::realm::config_realm::ConfigDialog;
use crate::ui::{DeferredAction, GlobalState};
use crate::realm::delete_realm::DeleteRealmDialog;
use crate::realm::new_realm::NewRealmDialog;
use crate::dialogs::confirm_dialog;
use crate::item_list::ItemList;
use crate::notes::NotesDialog;
use cursive::views::Dialog;
use crate::realmfs::RealmFSAction;

type ActionCallback = Fn(&Realm)+Send+Sync;

#[derive(Clone)]
pub struct RealmAction {
    realm: Realm,
    sink: Sender<Box<CbFunc>>,
    callback: Arc<ActionCallback>
}

impl RealmAction {

    pub fn set_realm_as_current() -> EventResult {
        Self::action(|r| {
            let manager = r.manager();
            Self::log_fail("setting current realm", || manager.set_current_realm(r));
        })
    }

    pub fn restart_realm(realm_active: bool) -> EventResult {
        if !realm_active {
            return EventResult::Consumed(None);
        }

        let title = "Restart Realm?";
        let msg = "Do you want to restart realm '$REALM'?";

        Self::confirm_action(title, msg, |r| {
            let manager = r.manager();
            if !Self::log_fail("stopping realm", || manager.stop_realm(r)) {
                return;
            }
            Self::log_fail("re-starting realm", || manager.start_realm(r));
        })
    }

    pub fn start_or_stop_realm(realm_active: bool) -> EventResult {
        if realm_active {
            Self::stop_realm()
        } else {
            Self::start_realm()
        }
    }

    fn stop_realm() -> EventResult {
        let title = "Stop Realm?";
        let msg = "Do you want to stop realm '$REALM'?";

        Self::confirm_action(title, msg, |r| {
            let manager = r.manager();
            Self::log_fail("stopping realm", || manager.stop_realm(r));
        })
    }

    fn start_realm() -> EventResult {
        Self::action(|r| {
            let manager = r.manager();
            Self::log_fail("starting realm", || manager.start_realm(r));
        })
    }

    pub fn open_terminal() -> EventResult {
        let title = "Open Terminal?";
        let msg = "Open terminal in realm '$REALM'?";
        Self::confirm_action(title, msg, |r| {
            let manager = r.manager();
            if !r.is_active() && !Self::log_fail("starting realm", || manager.start_realm(r)) {
                return;
            }
            Self::log_fail("starting terminal", || manager.launch_terminal(r));
        })
    }

    pub fn open_shell(root: bool) -> EventResult {
        EventResult::with_cb(move |s| {
            let realm = RealmAction::current_realm(s);
            let deferred = DeferredAction::RealmShell(realm.clone(), root);
            s.with_user_data(|gs: &mut GlobalState| gs.set_deferred(deferred));
            s.quit();
        })
    }

    pub fn update_realmfs() -> EventResult {
        let title = "Update RealmFS?";
        let msg = "Update $REALMFS-realmfs.img?";
        EventResult::with_cb(move |s| {
            if let Some(realmfs) = Self::current_realmfs(s) {
                if RealmFSAction::confirm_sealing_keys(s, &realmfs) {
                    let msg = msg.replace("$REALMFS", realmfs.name());
                    let dialog = confirm_dialog(title, &msg, move |siv| {
                        RealmFSAction::defer_realmfs_update(siv, realmfs.clone());
                    });
                    s.add_layer(dialog);
                }
            }
        })

    }

    pub fn configure_realm() -> EventResult {
        EventResult::with_cb(move |s| {
            let realm = RealmAction::current_realm(s);
            ConfigDialog::open(s, &realm);
        })
    }

    pub fn new_realm(manager: Arc<RealmManager>) -> EventResult {
        EventResult::with_cb(move |s| NewRealmDialog::open(s, manager.clone()))
    }

    pub fn delete_realm() -> EventResult {
        EventResult::with_cb(move |s| {
            let realm = RealmAction::current_realm(s);
            if !realm.is_system() {
                if realm.has_realmlock() {
                    let dialog = Dialog::info(format!("Cannot delete realm-{} because it has a .realmlock file.\n\
                        If you really want to remove this realm delete this file first", realm.name()))
                        .title("Cannot Delete");
                    s.add_layer(dialog);
                    return;
                }
                DeleteRealmDialog::open(s, realm.clone());
            }
        })
    }

    pub fn edit_notes() -> EventResult {

        EventResult::with_cb(|s| {
            let realm = RealmAction::current_realm(s);
            let desc = format!("realm-{}", realm.name());
            let notes = realm.notes().unwrap_or_default();
            NotesDialog::open(s, &desc, notes, move |s, notes| {
                if let Err(e) = realm.save_notes(notes) {
                    warn!("error saving notes file for realm-{}: {}", realm.name(), e);
                }
                ItemList::<Realm>::call_reload("realms", s);
            });

        })

    }

    fn log_fail<F>(msg: &str, f: F) -> bool
        where F: Fn() -> Result<()>
    {
        if let Err(e) = f() {
            warn!("error {}: {}", msg, e);
            false
        } else {
            true
        }
    }

    pub fn action<F>(callback: F) -> EventResult
        where F: Fn(&Realm), F: 'static + Sync+Send
    {
        EventResult::with_cb({
            let callback = Arc::new(callback);
            move |s| { RealmAction::new(s, callback.clone()).run_action(); }
        })
    }

    pub fn confirm_action<F>(title: &'static str, message: &'static str, callback: F) -> EventResult
        where F: Fn(&Realm), F: 'static + Send+Sync,
    {
        EventResult::with_cb({
            let callback = Arc::new(callback);
            move |s| {
                let action = RealmAction::new(s, callback.clone());
                let message = message.replace("$REALM", action.realm.name());
                let dialog = confirm_dialog(title, &message, move |_| action.run_action());
                s.add_layer(dialog);
            }
        })
    }

    fn new(s: &mut Cursive, callback: Arc<ActionCallback>) -> RealmAction {
        let realm = RealmAction::current_realm(s);
        let sink = s.cb_sink().clone();
        RealmAction { realm, sink, callback }
    }

    fn current_realmfs(s: &mut Cursive) -> Option<RealmFS> {
        let realm = Self::current_realm(s);
        let name = realm.config().realmfs().to_string();
        realm.manager().realmfs_by_name(&name)
    }

    fn current_realm(s: &mut Cursive) -> Realm {
        ItemList::<Realm>::call("realms", s, |v| v.selected_item().clone())
    }

    fn run_action(&self) {
        let action = self.clone();
        thread::spawn(move || {
            (action.callback)(&action.realm);
            action.sink.send(Box::new(RealmAction::update)).unwrap();
        });
    }

    fn update(s: &mut Cursive) {
        ItemList::<Realm>::call_reload("realms", s);
    }
}
