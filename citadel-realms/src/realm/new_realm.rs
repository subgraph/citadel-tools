use cursive::views::{ViewBox, SelectView, EditView, TextView, ViewRef, Dialog, TextContent};
use cursive::traits::{View,Identifiable,Finder};
use cursive::view::ViewWrapper;
use libcitadel::{RealmFS, GLOBAL_CONFIG, Realm, RealmManager};
use cursive::Cursive;
use crate::dialogs::{Validatable, DialogButtonAdapter, FieldDialogBuilder, ValidatorResult};
use cursive::theme::ColorStyle;
use cursive::event::{EventResult, Event};
use cursive::utils::markup::StyledString;
use libcitadel::terminal::Base16Scheme;
use std::sync::Arc;
use crate::item_list::ItemList;
use std::rc::Rc;

pub struct NewRealmDialog {
    manager: Arc<RealmManager>,
    message_content: TextContent,
    inner: ViewBox,
}

impl NewRealmDialog {

    const OK_BUTTON: usize = 1;

    fn get_dialog(s: &mut Cursive) -> ViewRef<NewRealmDialog> {
        s.find_id::<NewRealmDialog>("new-realm-dialog")
            .expect("could not find NewRealmDialog instance")
    }

    fn call_dialog<F,R>(s: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut NewRealmDialog) -> R
    {
        s.call_on_id("new-realm-dialog", f).expect("call_on_id(new-realm-dialog)")
    }

    pub fn open(s: &mut Cursive, manager: Arc<RealmManager>) {
        let mut dialog = NewRealmDialog::new(manager);
        dialog.name_updated();
        s.add_layer(dialog.with_id("new-realm-dialog"));
    }

    fn new(manager: Arc<RealmManager>) -> Self {

        let message_content = TextContent::new("");
        let text = "Provide a name for the new realm and choose the RealmFS to use as the root filesystem.";
        let dialog = FieldDialogBuilder::new(&["Realm Name", "", "RealmFS"], text)
            .title("New Realm")
            .id("new-realm-dialog-inner")
            .field(TextView::new_with_content(message_content.clone()).no_wrap())
            .edit_view("new-realm-name", 24)
            .field(Self::create_realmfs_select(manager.clone()))
            .build(Self::handle_ok)
            .validator("new-realm-name", |content| {
                let ok = content.is_empty() || Realm::is_valid_name(content);
                ValidatorResult::create(ok, |s| Self::call_dialog(s, |v| v.name_updated()))
            });

        NewRealmDialog { inner: ViewBox::boxed(dialog), message_content: message_content.clone(), manager }
    }

    fn create_realmfs_select(manager: Arc<RealmManager>) -> impl View {
        let mut select = SelectView::new().popup();
        let default_realmfs = GLOBAL_CONFIG.realmfs();
        let mut default_idx = 0;

        for (n,realmfs) in manager.realmfs_list()
            .into_iter()
            .filter(|r| r.is_user_realmfs())
            .enumerate() {

                if realmfs.name() == default_realmfs {
                    default_idx = n;
                }
                select.add_item(format!("{}-realmfs.img", realmfs.name()), Some(realmfs))
        }
        select.add_item("[ new realmfs... ]", None);
        select.set_selection(default_idx);
        select.set_on_submit({
            let manager = manager.clone();
            move |s,v: &Option<RealmFS>| {
                if v.is_some() {
                    return;
                }
                NewRealmDialog::get_dialog(s)
                    .call_on_realmfs_select(|v| v.set_selection(default_idx));

                let content = s.call_on_id("new-realm-name", |v: &mut EditView| v.get_content()).expect("new-realm-name");

                NewRealmFSDialog::open(s, manager.clone(), &content);
            }
        });

        select.with_id("new-realm-realmfs")
    }

    fn reload_realmfs(&mut self, name: &str) {
        let list = self.manager.realmfs_list()
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>();

        self.call_on_realmfs_select(move |v| {
            v.clear();
            let mut selected = 0;
            for (idx,realmfs) in list {
                if realmfs.name() == name {
                    selected = idx;
                }
                v.add_item(format!("{}-realmfs.img", realmfs.name()), Some(realmfs));
            }
            v.add_item("[ new realmfs... ]", None);
            v.set_selection(selected);
        });
    }


    fn set_ok_button_enabled(&mut self, enabled: bool) {
        self.set_button_enabled(Self::OK_BUTTON, enabled);
    }

    fn realm_name_exists(&self, name: &str) -> bool {
        self.manager.realm_by_name(name).is_some()
    }

    fn create_realm(&self, name: &str, realmfs_name: &str) {
        let realm = match self.manager.new_realm(&name) {
            Ok(realm) => realm,
            Err(e) => {
                warn!("failed to create realm: {}", e);
                return;
            }
        };
        realm.with_mut_config(|c| c.realmfs = Some(realmfs_name.to_string()));
        let config = realm.config();
        if let Err(err) = config.write() {
            warn!("error writing config file for new realm: {}", err);
        }
        let scheme_name = config.terminal_scheme().unwrap_or("default-dark").to_string();
        if let Some(scheme) = Base16Scheme::by_name(&scheme_name) {
            if let Err(e) = scheme.apply_to_realm(&self.manager, &realm) {
                warn!("error writing scheme files: {}", e);
            }
        }
    }

    fn handle_ok(s: &mut Cursive) {
        let is_enabled = NewRealmDialog::call_dialog(s, |d|  d.button_enabled(NewRealmDialog::OK_BUTTON));
        if !is_enabled {
            return;
        }

        let mut dialog = NewRealmDialog::get_dialog(s);
        let name = dialog.call_on_name_edit(|v| v.get_content());
        if !Realm::is_valid_name(&name) {
            s.add_layer(Dialog::info("Realm name is invalid."));
            return;
        }

        if dialog.realm_name_exists(&name) {
            s.add_layer(Dialog::info("Realm realm with that name already exists."));
            return;
        }

        let selection = dialog.call_on_realmfs_select(|v| {
            v.selection().expect("realmfs selection list was empty")
        });

        let realmfs = match *selection {
            Some(ref realmfs) => realmfs,
            None => { return; },
        };
        s.pop_layer();
        dialog.create_realm(name.as_str(), realmfs.name());
        ItemList::<Realm>::call_reload("realms", s);
    }

    fn name_updated(&mut self) {
        let content = self.call_on_name_edit(|v| v.get_content());
        let msg = if content.is_empty() {
            self.set_ok_button_enabled(false);
            StyledString::styled("Enter a realm name", ColorStyle::tertiary())
        } else if self.manager.realm_by_name(&content).is_some() {
            self.set_ok_button_enabled(false);
            StyledString::styled(format!("Realm '{}' already exists",content), ColorStyle::title_primary())
        } else {
            self.set_ok_button_enabled(true);
            format!("realm-{}", content).into()
        };
        self.message_content.set_content(msg);
    }

    fn call_on_name_edit<F,R>(&mut self, f: F) -> R
        where F: FnOnce(&mut EditView) -> R
    {
        self.call_id("new-realm-name", f)

    }

    fn call_on_realmfs_select<F,R>(&mut self, f: F) -> R
        where F: FnOnce(&mut SelectView<Option<RealmFS>>) -> R
    {
        self.call_id("new-realm-realmfs", f)
    }

    fn call_id<V: View, F: FnOnce(&mut V) -> R, R>(&mut self, id: &str, callback: F) -> R
    {
        self.call_on_id(id, callback)
            .unwrap_or_else(|| panic!("failed call_on_id({})", id))
    }
}

impl DialogButtonAdapter for NewRealmDialog {
    fn inner_id(&self) -> &'static str {
        "new-realm-dialog-inner"
    }
}

impl ViewWrapper for NewRealmDialog {
    type V = View;

    fn with_view<F, R>(&self, f: F) -> Option<R>
        where F: FnOnce(&Self::V) -> R
    {
        Some(f(&*self.inner))
    }

    fn with_view_mut<F, R>(&mut self, f: F) -> Option<R>
        where F: FnOnce(&mut Self::V) -> R
    {
        Some(f(&mut *self.inner))
    }

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        self.handle_event("co", event)
    }
}

struct NewRealmFSDialog {
    inner: ViewBox,
    manager: Arc<RealmManager>,
    message_content: TextContent,
}

impl NewRealmFSDialog {
    const OK_BUTTON: usize = 1;

    fn get_dialog(s: &mut Cursive) -> ViewRef<NewRealmFSDialog> {
        s.find_id::<NewRealmFSDialog>("new-realmfs-dialog")
            .expect("could not find NewRealmFSDialog instance")
    }

    fn call_dialog<F,R>(s: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut NewRealmFSDialog) -> R
    {
        s.call_on_id("new-realmfs-dialog", f).expect("call_on_id(new-realmfs-dialog)")
    }

    pub fn open(s: &mut Cursive, manager: Arc<RealmManager>, name: &str) {
        let mut dialog = NewRealmFSDialog::new(manager, name);
        dialog.name_updated();
        s.add_layer(dialog.with_id("new-realmfs-dialog"));
    }

    fn new(manager: Arc<RealmManager>, name: &str) -> Self {
        let message_content = TextContent::new("");

        let text = "Create a new RealmFS to use with the new realm by forking an existing RealmFS.";
        let mut dialog = FieldDialogBuilder::new(&["RealmFS Name","","Fork From"], text)
            .title("New RealmFS")
            .id("new-realmfs-dialog-inner")
            .height(16)
            .field(TextView::new_with_content(message_content.clone()).no_wrap())
            .edit_view("new-realmfs-name", 24)
            .field(Self::create_realmfs_select(&manager))
            .build(Self::handle_ok)
            .validator("new-realmfs-name", |content| {
                let ok = content.is_empty() || RealmFS::is_valid_name(content);
                ValidatorResult::create(ok, |s| Self::call_dialog(s, |v| v.name_updated()))
            });

        let name = Self::choose_realmfs_name(&manager, name);
        dialog.call_on_id("new-realmfs-name", |v: &mut EditView| v.set_content(name));

        let inner = ViewBox::boxed(dialog);

        NewRealmFSDialog{ inner, manager, message_content }
    }

    fn name_updated(&mut self) {
        let content = self.call_on_name_edit(|v| v.get_content());
        let msg = if content.is_empty() {
            self.set_ok_button_enabled(false);
            StyledString::styled("Enter a name for new RealmFS", ColorStyle::tertiary())
        } else if self.manager.realmfs_name_exists(&content) {
            self.set_ok_button_enabled(false);
            StyledString::styled(format!("RealmFS '{}' already exists",content), ColorStyle::title_primary())
        } else {
            self.set_ok_button_enabled(true);
            format!("{}-realmfs.img", content).into()
        };
        self.message_content.set_content(msg);
    }

    fn name_edit_content(&mut self) -> Rc<String> {
        self.call_on_name_edit(|v| v.get_content())
    }

    fn call_on_name_edit<F,R>(&mut self, f: F) -> R
        where F: FnOnce(&mut EditView) -> R
    {
        self.call_id("new-realmfs-name", f)

    }
    fn call_on_realmfs_select<F,R>(&mut self, f: F) -> R
        where F: FnOnce(&mut SelectView<RealmFS>) -> R
    {
        self.call_id("new-realmfs-source", f)
    }

    fn call_id<V: View, F: FnOnce(&mut V) -> R, R>(&mut self, id: &str, callback: F) -> R
    {
        self.call_on_id(id, callback)
            .unwrap_or_else(|| panic!("failed call_on_id({})", id))
    }

    fn set_ok_button_enabled(&mut self, enabled: bool) {
        self.set_button_enabled(Self::OK_BUTTON, enabled);
    }

    fn choose_realmfs_name(manager: &RealmManager, name: &str) -> String {
        let name = if name.is_empty() {
            "new-name"
        } else {
            name
        };
        Self::find_unique_name(manager, name)
    }

    fn find_unique_name(manager: &RealmManager, orig_name: &str) -> String {


        let mut name = orig_name.to_string();
        let mut num = 1;
        while manager.realmfs_name_exists(&name) {
            name = format!("{}{}", orig_name, num);
            num += 1;
        }
        name
    }

    fn create_realmfs_select(manager: &RealmManager) -> impl View {
        let default_realmfs = GLOBAL_CONFIG.realmfs();
        let mut select = SelectView::new().popup();
        let mut default_idx = 0;
        for (idx,realmfs) in manager.realmfs_list().into_iter().enumerate() {
            if realmfs.name() == default_realmfs {
                default_idx = idx;
            }
            select.add_item(format!("{}-realmfs.img", realmfs.name()), realmfs);
        }
        select.set_selection(default_idx);
        select.with_id("new-realmfs-source")
    }


    fn handle_ok(s: &mut Cursive) {
        let mut dialog = Self::get_dialog(s);

        if !dialog.button_enabled(Self::OK_BUTTON) {
            return;
        }

        let name = dialog.name_edit_content();

        if !RealmFS::is_valid_name(&name) {
            s.add_layer(Dialog::info("RealmFS name is invalid.").title("Invalid Name"));
        }

        if let Some(realmfs) = dialog.call_on_realmfs_select(|v| v.selection()) {
            if let Err(e) = realmfs.fork(&name) {
                let msg = format!("Failed to fork RealmFS '{}' to '{}': {}", realmfs.name(), name, e);
                s.pop_layer();
                s.add_layer(Dialog::info(msg.as_str()).title("Fork Failed"));
                return;
            }
        }
        NewRealmDialog::call_dialog(s, |v| v.reload_realmfs(&name));
        s.pop_layer();

    }
}


impl ViewWrapper for NewRealmFSDialog {
    type V = View;

    fn with_view<F, R>(&self, f: F) -> Option<R>
        where F: FnOnce(&Self::V) -> R
    {
        Some(f(&*self.inner))
    }

    fn with_view_mut<F, R>(&mut self, f: F) -> Option<R>
        where F: FnOnce(&mut Self::V) -> R
    {
        Some(f(&mut *self.inner))
    }

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        self.handle_event("co", event)
    }
}

impl DialogButtonAdapter for NewRealmFSDialog {
    fn inner_id(&self) -> &'static str {
        "new-realmfs-dialog-inner"
    }
}
