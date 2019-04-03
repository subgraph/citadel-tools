use std::rc::Rc;

use cursive::{
    Printer, Vec2, Cursive,
    align::HAlign,
    direction::Direction,
    event::{
        EventResult, Event, Key
    },
    theme::ColorStyle,
    traits::{
        View,Identifiable,Finder,Boxable,Scrollable
    },
    utils::markup::StyledString,
    view::ViewWrapper,
    views::{ViewBox, LinearLayout, TextView, DummyView, PaddedView, Dialog, Button, SelectView},
};

use libcitadel::{RealmConfig, RealmFS, Realm, OverlayType, terminal::Base16Scheme, RealmManager};

use crate::theme::ThemeChooser;
use crate::dialogs::DialogButtonAdapter;
use cursive::direction::Absolute;
use std::sync::Arc;
use crate::item_list::ItemList;

pub struct ConfigDialog {
    manager: Arc<RealmManager>,
    realm: Realm,
    scheme: Option<String>,
    realmfs: Option<String>,
    overlay: OverlayType,
    realmfs_list: Vec<String>,
    inner: ViewBox,
}

fn color_scheme(config: &RealmConfig) -> &Base16Scheme {
    if let Some(name) = config.terminal_scheme() {
        if let Some(scheme) = Base16Scheme::by_name(name) {
            return scheme;
        }
    }
    Base16Scheme::by_name("default-dark").unwrap()
}

impl ConfigDialog {

    const APPLY_BUTTON: usize = 0;
    const RESET_BUTTON: usize = 1;

    pub fn open(s: &mut Cursive, realm: &Realm) {
        let name = realm.name().to_string();
        let dialog = ConfigDialog::new(&name, realm.clone());
        s.add_layer(dialog.with_id("config-dialog"));
        ConfigDialog::call_dialog(s, |d| d.reset_changes());
    }

    fn call_dialog<F,R>(s: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut ConfigDialog) -> R
    {
        s.call_on_id("config-dialog", f).expect("call_on_id(config-dialog)")
    }

    fn call<F,R>(f: F) -> impl Fn(&mut Cursive) -> R
        where F: Fn(&mut ConfigDialog) -> R,
    {
        let cb = Rc::new(Box::new(f));
        move |s: &mut Cursive| {
            let cb = cb.clone();
            s.call_on_id("config-dialog", move |v| {
                (cb)(v)
            }).unwrap()

        }
    }

    fn realmfs_list(manager: &RealmManager) -> Vec<RealmFS> {
        manager.realmfs_list()
            .into_iter()
            .filter(|r| {
            r.metainfo().channel().is_empty() || r.metainfo().channel() == "realmfs-user"
        }).collect()
    }

    pub fn new(name: &str, realm: Realm) -> Self {
        let config = realm.config();
        let manager = realm.manager();

        let realmfs_list = ConfigDialog::realmfs_list(&manager);
        let realmfs_names = realmfs_list.iter().map(|r| r.name().to_string()).collect();

        let content = LinearLayout::vertical()
            .child(TextView::new(format!("Configuration options for Realm '{}'\n\nUse <Apply> button to save changes.", name)))
            .child(DummyView)
            .child(ConfigDialog::header("Options"))
            .child(DummyView)
            .child(RealmOptions::with_config(&config).with_id("realm-options"))
            .child(DummyView)
            .child(ConfigDialog::realmfs_widget(realmfs_list))
            .child(ConfigDialog::overlay_widget(&config))
            .child(ConfigDialog::colorscheme_widget(&config))
            .scrollable();

        let dialog = Dialog::around(PaddedView::new((2,2,1,0), content))
            .title("Realm Config")
            .button("Apply", |s| {
                s.call_on_id("config-dialog", |d: &mut ConfigDialog| d.apply_changes());
                ItemList::<Realm>::call_update_info("realms", s);
                s.pop_layer();
            })
            .button("Reset", |s| {
                s.call_on_id("config-dialog", |d: &mut ConfigDialog| d.reset_changes());
            })
            .dismiss_button("Cancel")
            .with_id("config-dialog-inner");

        ConfigDialog { manager, realm, scheme: None, realmfs:None, overlay: OverlayType::None, realmfs_list: realmfs_names, inner: ViewBox::boxed(dialog) }
    }

    fn has_changes(&mut self) -> bool {
        let config = self.realm.config();
        if self.realmfs != config.realmfs || self.scheme != config.terminal_scheme || config.overlay() != self.overlay {
            return true;
        }
        drop(config);
        self.call_on_options(|v| v.has_changes())
    }

    fn update_buttons(&mut self) {
        let dirty = self.has_changes();
        self.set_button_enabled(Self::APPLY_BUTTON, dirty);
        self.set_button_enabled(Self::RESET_BUTTON, dirty);
    }

    fn call_on_options<F,R>(&mut self, f: F) -> R
        where F: FnOnce(&mut RealmOptions) -> R
    {
        self.call_id("realm-options", f)
    }

    fn call_on_scheme_button<R,F: FnOnce(&mut Button) -> R>(&mut self, f: F) -> R {
        self.call_id("scheme-button", f)
    }

    fn call_on_overlay_select<R,F: FnOnce(&mut SelectView<OverlayType>) -> R>(&mut self, f: F) -> R {
        self.call_id("overlay-select", f)
    }

    fn call_on_realmfs_select<R,F: FnOnce(&mut SelectView<RealmFS>)->R>(&mut self, f: F) -> R {
        self.call_id("realmfs-select", f)
    }

    fn call_id<V: View, F: FnOnce(&mut V) -> R, R>(&mut self, id: &str, callback: F) -> R
    {
        self.call_on_id(id, callback)
            .unwrap_or_else(|| panic!("failed call_on_id({})", id))
    }

    pub fn reset_changes(&mut self) {

        let config = self.realm.config();

        self.realmfs = config.realmfs.clone();
        self.scheme = config.terminal_scheme.clone();
        self.overlay = config.overlay();

        let realmfs_name = config.realmfs().to_string();
        drop(config);

        self.set_realmfs_selection(&realmfs_name);
        self.set_overlay_selection(self.overlay);

        let scheme_name = self.realm.config().terminal_scheme().unwrap_or("default-dark").to_string();
        self.call_on_scheme_button(|b| b.set_label(scheme_name.as_str()));

        self.call_on_options(|v| v.reset_changes());
        self.update_buttons();
        self.call_on_dialog(|d| d.take_focus(Direction::none()));

    }

    fn set_realmfs_selection(&mut self, name: &str) {
        for (i,n) in self.realmfs_list.iter().enumerate() {
            if n.as_str() == name {
                self.call_on_realmfs_select(|v| v.set_selection(i));
                return;
            }
        }
    }

    fn set_overlay_selection(&mut self, overlay: OverlayType) {
        let idx = ConfigDialog::overlay_index(overlay);
        self.call_on_overlay_select(|v| v.set_selection(idx));
    }

    pub fn apply_changes(&mut self) {
        let realm = self.realm.clone();

        let scheme_changed = realm.config().terminal_scheme != self.scheme;
        realm.with_mut_config(|c| {
            c.terminal_scheme = self.scheme.clone();
            c.realmfs = self.realmfs.clone();
            c.set_overlay(self.overlay);

            self.call_on_options(|v| v.save_config(c));
        });

        let path = self.realm.base_path_file("config");

        if let Err(e) = self.realm.config().write_config(&path) {
            warn!("Error writing config file {}: {}", path.display(), e);
        }
        info!("Config file written to {}", path.display());
        if scheme_changed {
            self.apply_colorscheme();
        }
    }


    fn apply_colorscheme(&self) {
        let config = self.realm.config();
        let scheme = color_scheme(&config).clone();
        drop(config);
        if let Err(e) = scheme.apply_to_realm(&self.manager, &self.realm) {
            warn!("error writing color scheme: {}", e);
        }
    }

    fn colorscheme_widget(config: &RealmConfig) -> impl View {
        let scheme = color_scheme(&config).clone();
        let scheme_name = scheme.name().to_string();
        let scheme_button = Button::new(scheme_name, move |s| {
            let chooser = ThemeChooser::new(Some(scheme.clone()), |s,theme| {
                s.pop_layer();
                s.call_on_id("config-dialog", |v: &mut ConfigDialog| v.set_scheme(theme));
            });
            s.add_layer(chooser);
        }).with_id("scheme-button");

        LinearLayout::horizontal()
            .child(ConfigDialog::header("Color Scheme"))
            .child(DummyView.fixed_width(10))
            .child(scheme_button)
            .child(DummyView)

    }

    fn overlay_index(overlay: OverlayType) -> usize {
        match overlay {
            OverlayType::None => 0,
            OverlayType::TmpFS => 1,
            OverlayType::Storage => 2,
        }
    }

    fn overlay_widget(config: &RealmConfig) -> impl View {
        let overlay = config.overlay();
        let overlay_select = SelectView::new().popup()
            .item("No overlay", OverlayType::None)
            .item("tmpfs overlay", OverlayType::TmpFS)
            .item("Storage partition", OverlayType::Storage)
            .selected(ConfigDialog::overlay_index(overlay))
            .on_submit(|s,v| { s.call_on_id("config-dialog", |d: &mut ConfigDialog| d.set_overlay(*v)); })
            .with_id("overlay-select");

        LinearLayout::horizontal()
            .child(ConfigDialog::header("Overlay"))
            .child(DummyView.fixed_width(15))
            .child(overlay_select)
            .child(DummyView)
    }


    fn realmfs_widget(realmfs_list: Vec<RealmFS>) -> impl View {
        let mut realmfs_select = SelectView::new().popup().on_submit(|s,v: &RealmFS|{
            let name = v.name().to_string();
            s.call_on_id("config-dialog", move |d: &mut ConfigDialog| d.set_realmfs(&name));
        });
        for realmfs in realmfs_list {
                realmfs_select.add_item(format!("{}-realmfs.img", realmfs.name()), realmfs);
        }
        LinearLayout::horizontal()
            .child(ConfigDialog::header("RealmFS Image"))
            .child(DummyView.fixed_width(9))
            .child(realmfs_select.with_id("realmfs-select"))
            .child(DummyView)
    }

    pub fn set_realmfs(&mut self, name: &str) {
        self.realmfs = Some(name.to_string());
        self.update_buttons();
    }

    pub fn set_scheme(&mut self, scheme: &Base16Scheme) {
        self.scheme = Some(scheme.slug().to_string());
        self.call_on_id("scheme-button", |v: &mut Button| v.set_label(scheme.name()));
        self.update_buttons();
    }

    pub fn set_overlay(&mut self, overlay: OverlayType) {
        self.overlay = overlay;
        self.update_buttons();
    }

    fn header(text: &str) -> impl View {
        let text = StyledString::styled(text, ColorStyle::title_primary());
        TextView::new(text).h_align(HAlign::Left)
    }
}

impl DialogButtonAdapter for ConfigDialog {
    fn inner_id(&self) -> &'static str {
        "config-dialog-inner"
    }
}

impl ViewWrapper for ConfigDialog {
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
        self.handle_event("arc", event)
    }
}

struct OptionEntry {
    label: String,
    original: Option<bool>,
    value: Option<bool>,
    default: bool,
    accessor: Box<Accessor>,
}

impl OptionEntry {
    fn new<F>(label: &str, accessor: F) -> OptionEntry
        where F: 'static + Fn(&mut RealmConfig) -> &mut Option<bool>
    {
        let label = label.to_string();
        OptionEntry { label, original: None, value: None, default: false, accessor: Box::new(accessor)}
    }

    fn is_default(&self) -> bool {
        self.value.is_none()
    }

    fn resolve_default(&self, config: &mut RealmConfig) -> bool
    {
        match config.parent {
            Some(ref mut parent) => match (self.accessor)(parent) {
                &mut Some(v) => v,
                None => self.resolve_default(parent),
            },
            None => false,
        }
    }

    fn save(&self, config: &mut RealmConfig) {
        let var = (self.accessor)(config);
        *var = self.value;
    }

    fn load(&mut self, config: &mut RealmConfig) {
        let var = (self.accessor)(config);
        self.value = *var;
        self.original = *var;
        self.default = self.resolve_default(config);
    }

    fn set(&mut self, v: bool) {
        if v != self.default || self.original == Some(v) {
            self.value = Some(v);
        } else {
            self.value = None;
        }
    }

    fn get(&self) -> bool {
        match self.value {
            Some(v) => v,
            None => self.default,
        }
    }

    fn toggle(&mut self) {
        self.set(!self.get())
    }

    fn reset(&mut self) {
        self.value = self.original;
    }

    fn is_dirty(&self) -> bool {
        self.value != self.original
    }
}

struct RealmOptions {
    last_size: Vec2,
    entries: Vec<OptionEntry>,
    selection: usize,

}

type Accessor = 'static + (Fn(&mut RealmConfig) -> &mut Option<bool>);

impl RealmOptions {

    fn new() -> Self {
        RealmOptions {
            last_size: Vec2::zero(),
            entries: RealmOptions::create_entries(),
            selection: 0,
        }
    }

    fn with_config(config: &RealmConfig) -> Self {
        let mut widget = Self::new();
        let mut config = config.clone();
        widget.load_config(&mut config);
        widget
    }

    pub fn save_config(&self, config: &mut RealmConfig) {
        for e in &self.entries {
            e.save(config);
        }
    }

    fn load_config(&mut self, config: &mut RealmConfig) {
        for e in &mut self.entries {
            e.load(config);
        }
    }


    fn create_entries() -> Vec<OptionEntry> {
        vec![
            OptionEntry::new("Use GPU in Realm", |c| &mut c.use_gpu),
            OptionEntry::new("Use Wayland in Realm", |c| &mut c.use_wayland),
            OptionEntry::new("Use X11 in Realm", |c| &mut c.use_x11),
            OptionEntry::new("Use Sound in Realm", |c| &mut c.use_sound),
            OptionEntry::new("Mount /Shared directory in Realm", |c| &mut c.use_shared_dir),
            OptionEntry::new("Realm has network access", |c| &mut c.use_network),
            OptionEntry::new("Use KVM (/dev/kvm) in Realm", |c| &mut c.use_kvm),
            OptionEntry::new("Use ephemeral tmpfs mount for home directory", |c| &mut c.use_ephemeral_home),
        ]
    }

    fn draw_entry(&self, printer: &Printer, idx: usize) {
        let entry = &self.entries[idx];
        let selected = idx == self.selection;
        let cursor = if selected && printer.focused { "> " } else { "  " };
        let check = if entry.get() { "[X]" } else { "[ ]" };
        let line = format!("{}{}  {}", cursor, check, entry.label);
        if entry.is_default() {
            printer.with_color(ColorStyle::tertiary(), |p| p.print((0,idx), &line));
        } else {
            printer.print((0, idx), &line);
        }
    }

    fn selection_up(&mut self) -> EventResult {
        if self.selection > 0 {
            self.selection -= 1;
            EventResult::Consumed(None)
        } else {
            EventResult::Ignored
        }
    }

    fn selection_down(&mut self) -> EventResult {
        if self.selection + 1 < self.entries.len() {
            self.selection += 1;
            EventResult::Consumed(None)
        } else {
            EventResult::Ignored
        }
    }

    fn select_last(&mut self) {
        if !self.entries.is_empty() {
            self.selection = self.entries.len() - 1
        }
    }

    fn toggle_entry(&mut self) -> EventResult {
        self.entries[self.selection].toggle();
        EventResult::with_cb(ConfigDialog::call(|v| v.update_buttons()))
    }


    fn has_changes(&self) -> bool {
        self.entries.iter().any(OptionEntry::is_dirty)
    }

    fn reset_changes(&mut self) {
        for entry in &mut self.entries {
            entry.reset();
        }
        self.selection = 0;
    }
}


impl View for RealmOptions {
    fn draw(&self, printer: &Printer) {
        for idx in 0..self.entries.len() {
            self.draw_entry(printer, idx);
        }
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
    }

    fn required_size(&mut self, _: Vec2) -> Vec2 {
        Vec2::new(60, self.entries.len())
    }

    fn on_event(&mut self, event: Event) -> EventResult {
        match event {
            Event::Key(Key::Up) => self.selection_up(),
            Event::Key(Key::Down) => self.selection_down(),
            Event::Key(Key::Enter) | Event::Char(' ') => self.toggle_entry(),
            _ => EventResult::Ignored,
        }
    }

    fn take_focus(&mut self, source: Direction) -> bool {
        if source == Direction::Abs(Absolute::Down) {
            self.select_last();
        }
        true
    }
}

