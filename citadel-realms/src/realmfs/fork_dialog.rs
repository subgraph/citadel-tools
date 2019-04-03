use libcitadel::{RealmManager, RealmFS};
use cursive::views::{TextContent, ViewBox, TextView, EditView, Dialog};
use cursive::traits::{Identifiable, View,Finder};
use std::sync::Arc;
use crate::dialogs::{FieldDialogBuilder, Validatable, ValidatorResult, DialogButtonAdapter};
use cursive::Cursive;
use cursive::utils::markup::StyledString;
use cursive::theme::ColorStyle;
use cursive::event::{EventResult, Event};
use cursive::view::ViewWrapper;
use std::rc::Rc;
use crate::item_list::ItemList;

pub struct ForkDialog {
    realmfs: RealmFS,
    manager: Arc<RealmManager>,
    message_content: TextContent,
    inner: ViewBox,
}

impl ForkDialog {
    const OK_BUTTON: usize = 1;
    fn call_dialog<F,R>(s: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut ForkDialog) -> R
    {
        s.call_on_id("fork-realmfs-dialog", f).expect("call_on_id(fork-realmfs-dialog)")
    }

    /*
    fn get_dialog(s: &mut Cursive) -> ViewRef<ForkDialog> {
        s.find_id::<ForkDialog>("fork-realmfs-dialog")
            .expect("could not find ForkDialog instance")
    }
    */

    pub fn open(s: &mut Cursive, realmfs: RealmFS) {
        let mut dialog = ForkDialog::new(realmfs);
        dialog.name_updated();
        s.add_layer(dialog.with_id("fork-realmfs-dialog"));
    }

    fn new(realmfs: RealmFS) -> Self {
        let message_content = TextContent::new("");
        let text = format!("Fork {}-realmfs.img to produce a new RealmFS image. Provide a name for the new image.", realmfs.name());
        let dialog = FieldDialogBuilder::new(&["Name", ""], &text)
            .title("Fork RealmFS")
            .id("fork-realmfs-inner")
            .field(TextView::new_with_content(message_content.clone()).no_wrap())
            .edit_view("new-realmfs-name", 24)
            .build(Self::handle_ok)
            .validator("new-realmfs-name", |content| {
                let ok = content.is_empty() || RealmFS::is_valid_name(content);
                ValidatorResult::create(ok, |s| Self::call_dialog(s, |v| v.name_updated()))

            });
        let manager = realmfs.manager();
        ForkDialog { realmfs, inner: ViewBox::boxed(dialog), message_content: message_content.clone(), manager }
    }

    fn set_ok_button_enabled(&mut self, enabled: bool) {
        self.set_button_enabled(Self::OK_BUTTON, enabled);
    }

    fn handle_ok(s: &mut Cursive) {
        let is_enabled = ForkDialog::call_dialog(s, |d|  d.button_enabled(Self::OK_BUTTON));
        if !is_enabled {
            return;
        }
        let name = Self::call_dialog(s, |v| v.name_edit_content());
        if !RealmFS::is_valid_name(&name) {
            s.add_layer(Dialog::info("RealmFS name is invalid.").title("Invalid Name"));
        }
        let realmfs = Self::call_dialog(s, |v| v.realmfs.clone());
        if let Err(e) = realmfs.fork(&name) {
           let msg = format!("Failed to fork RealmFS '{}' to '{}': {}", realmfs.name(), name, e);
            warn!(msg.as_str());
            s.pop_layer();
            s.add_layer(Dialog::info(msg.as_str()));
            return;
        }

        s.pop_layer();
        ItemList::<RealmFS>::call_reload("realmfs", s);
    }

    fn name_updated(&mut self) {

        let content = self.name_edit_content();
        let msg = if content.is_empty() {
            self.set_ok_button_enabled(false);
            StyledString::styled("Enter a name", ColorStyle::tertiary())
        } else if self.manager.realmfs_by_name(&content).is_some() {
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
    fn call_id<V: View, F: FnOnce(&mut V) -> R, R>(&mut self, id: &str, callback: F) -> R
    {
        self.call_on_id(id, callback)
            .unwrap_or_else(|| panic!("failed call_on_id({})", id))
    }
}

impl ViewWrapper for ForkDialog {
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

impl DialogButtonAdapter for ForkDialog {
    fn inner_id(&self) -> &'static str {
        "fork-realmfs-inner"
    }
}
