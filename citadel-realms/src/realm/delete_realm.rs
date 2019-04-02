use cursive::view::ViewWrapper;
use cursive::traits::{View,Boxable,Identifiable};
use cursive::views::{ViewBox, DummyView, PaddedView, TextView, Dialog, LinearLayout};
use cursive::Cursive;
use libcitadel::Realm;
use cursive::utils::markup::StyledString;
use crate::dialogs::{keyboard_navigation_adapter, DialogButtonAdapter};
use cursive::theme::ColorStyle;
use cursive::event::{Event, EventResult};
use crate::item_list::ItemList;

pub struct DeleteRealmDialog {
    inner: ViewBox,
    realm: Realm,
}

impl DeleteRealmDialog {

    pub fn call<F,R>(s: &mut Cursive, callback: F) -> R
        where F: FnOnce(&mut Self) -> R
    {
        s.call_on_id("delete-realm-dialog", callback)
            .expect("delete realm dialog not found")
    }

    pub fn open(s: &mut Cursive, realm: Realm) {
        let dialog = Self::new(realm)
            .with_id("delete-realm-dialog");
        s.add_layer(dialog);
    }

    fn new(realm: Realm) -> Self {
        let text = TextView::new(format!("Are you sure you want to delete realm '{}'?", realm.name()));
        let content = PaddedView::new((2,2,2,2), text);

        let dialog = Dialog::around(content)
            .title("Delete Realm?")
            .dismiss_button("Cancel")
            .button("Delete", Self::handle_delete);

        let inner = ViewBox::boxed(dialog.with_id("delete-dialog-inner"));

        DeleteRealmDialog { inner, realm }
    }

    fn handle_delete(s: &mut Cursive) {
        let dialog = Self::call(s, |v| v.create_ask_save_home());
        s.add_layer(dialog);
    }

    fn create_ask_save_home(&self) -> impl View {

        let content = PaddedView::new((2,2,2,2), LinearLayout::vertical()
            .child(TextView::new(format!("The home directory for this realm can be saved in:\n\n       /realms/removed/home-{}", self.realm.name())))
            .child(DummyView)
            .child(TextView::new("Would you like to save the home directory?"))
            .child(DummyView)
            .child(TextView::new(StyledString::styled("Or press [esc] to cancel removal of the realm.", ColorStyle::tertiary())))
            .fixed_width(60)
        );


        let dialog = Dialog::around(content)
            .title("Save home directory")
            .button("Yes", |s| Self::delete_realm(s, true))
            .button("No", |s| Self::delete_realm(s, false));

        keyboard_navigation_adapter(dialog, "ny")
    }

    fn delete_realm(s: &mut Cursive, save_home: bool) {
        s.pop_layer();
        Self::call(s, |v| {
            let manager = v.realm.manager();
            if let Err(e) = manager.delete_realm(&v.realm, save_home) {
                warn!("error deleting realm: {}", e);
            }
        });
        s.pop_layer();
        ItemList::<Realm>::call_reload("realms", s);
    }
}

impl DialogButtonAdapter for DeleteRealmDialog {
    fn inner_id(&self) -> &'static str {
        "delete-dialog-inner"
    }
}

impl ViewWrapper for DeleteRealmDialog {
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
        self.handle_event("cd", event)
    }
}
