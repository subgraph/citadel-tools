use std::sync::Arc;


use cursive::{
    Printer,
    event::{EventResult, Event, Key},
    utils::markup::StyledString,
    theme::{ColorStyle,PaletteColor, ColorType, Effect, Style},
};


use libcitadel::{Realm, RealmManager, RealmConfig, RealmFS};


use self::actions::RealmAction;
use crate::item_list::{ItemListContent, Selector, InfoRenderer, ItemRenderState, ItemList};
use std::rc::Rc;

mod actions;
mod new_realm;
mod delete_realm;
mod config_realm;

pub struct RealmListContent {
    show_system_realms: bool,
    manager: Arc<RealmManager>,
}

impl RealmListContent {

    pub fn new(manager: Arc<RealmManager>) -> Self {
        RealmListContent { show_system_realms: false, manager }
    }

    fn realm_fg_color(realm: &Realm, current: ColorStyle, selected: bool, focused: bool) -> ColorType {
        if realm.is_active() {
            if focused {
                if realm.is_system() {
                    PaletteColor::Tertiary.into()
                } else {
                    PaletteColor::Secondary.into()
                }
            } else {
                current.front
            }
        } else if selected {
            PaletteColor::View.into()
        } else {
            PaletteColor::Primary.into()
        }
    }

    fn draw_color_style(selected: bool, focused: bool) -> ColorStyle {
        if selected {
            if focused {
                ColorStyle::highlight()
            } else {
                ColorStyle::highlight_inactive()
            }
        } else {
            ColorStyle::primary()
        }
    }
    fn draw_realm(&self, width: usize, printer: &Printer, realm: &Realm, selected: bool) {
        let w = realm.name().len() + 2;
        let mut cstyle = Self::draw_color_style(selected, printer.focused);
        let prefix = if realm.is_current() { "> " } else { "  " };
        printer.print((0,0), prefix);
        cstyle.front = Self::realm_fg_color(realm, cstyle, selected, printer.focused);
        printer.with_color(cstyle, |p| {
            if realm.is_active() {
                printer.with_effect(Effect::Bold, |p| p.print((2,0), realm.name()));
            } else {
                p.print((2,0), realm.name());
            }
        } );

        if width > w {
            printer.with_selection(selected, |p| p.print_hline((w, 0), width - w, " "));
        }
    }
}

impl ItemListContent<Realm> for RealmListContent {
    fn items(&self) -> Vec<Realm> {
        if self.show_system_realms {
            self.manager.realm_list()
        } else {
            self.manager.realm_list()
                .into_iter()
                .filter(|r| !r.is_system())
                .collect()
        }
    }

    fn reload(&self, selector: &mut Selector<Realm>) {
        selector.load_and_keep_selection(self.items(), |r1,r2| r1.name() == r2.name());
    }

    fn draw_item(&self, width: usize, printer: &Printer, item: &Realm, selected: bool) {
        self.draw_realm(width, printer, item, selected);
    }

    fn update_info(&mut self, realm: &Realm, state: Rc<ItemRenderState>) {
        RealmInfoRender::new(state, realm).render()
    }

    fn on_event(&mut self, item: Option<&Realm>, event: Event) -> EventResult {
        let realm_active = item.map(|r| r.is_active()).unwrap_or(false);
        match event {
            Event::Key(Key::Enter) => RealmAction::set_realm_as_current(),
            Event::Char('s') => RealmAction::start_or_stop_realm(realm_active),
            Event::Char('t') => RealmAction::open_terminal(),
            Event::Char('r') => RealmAction::restart_realm(realm_active),
            Event::Char('c') => RealmAction::configure_realm(),
            Event::Char('n') => RealmAction::new_realm(self.manager.clone()),
            Event::Char('d') => RealmAction::delete_realm(),
            Event::Char('e') => RealmAction::edit_notes(),
            Event::Char('$') => RealmAction::open_shell(false),
            Event::Char('#') => RealmAction::open_shell(true),
            Event::Char('u') => RealmAction::update_realmfs(),
            Event::Char('.') => {
                self.show_system_realms = !self.show_system_realms;
                EventResult::with_cb(|s| ItemList::<Realm>::call_reload("realms", s))
            },

            _ => EventResult::Ignored,
        }
    }
}

#[derive(Clone)]
struct RealmInfoRender<'a> {
    state: Rc<ItemRenderState>,
    realm: &'a Realm,
}

impl <'a> RealmInfoRender <'a> {
    fn new(state: Rc<ItemRenderState>, realm: &'a Realm) -> Self {
        RealmInfoRender { state, realm }
    }

    fn render(&mut self) {
        self.render_realm();
        let config = self.realm.config();
        self.render_realmfs_info(&config);
        self.render_options(&config);
        self.render_notes();
    }

    fn render_realm(&mut self) {

        self.heading("Realm")
            .print("   ")
            .render_name();

        if self.realm.is_active() {
            self.print("  Running");
            if let Some(pid) = self.realm.leader_pid() {
                self.print(format!(" (Leader pid: {})", pid));
            }
        }
        self.newlines(2);
    }

    fn render_name(&self) -> &Self {
        if self.realm.is_system() && self.realm.is_active() {
            self.dim_bold_style();
        } else if self.realm.is_system() {
            self.dim_style();
        } else if self.realm.is_active() {
            self.activated_style();
        } else {
            self.plain_style();
        }

        self.print(self.realm.name()).pop();
        self
    }

    fn render_realmfs_info(&mut self, config: &RealmConfig) {
        let name = config.realmfs();

        let realmfs = match self.realm.manager().realmfs_by_name(name) {
            Some(realmfs) => realmfs,
            None => return,
        };

        if realmfs.is_activated() {
            self.activated_style();
        } else {
            self.plain_style();
        }

        self.heading("RealmFS")
            .print(" ")
            .print(format!("{}-realmfs.img", realmfs.name()))
            .pop();

        if self.detached(&realmfs) {
            self.alert_style().print("  Need restart for updated RealmFS").pop();
        }

        self.newlines(2);

        if let Some(mount) = self.realm.realmfs_mountpoint() {
            self.print("   Mount: ").dim_style().println(format!("{}", mount)).pop();
            self.newline();
        }

    }

    fn detached(&self, realmfs: &RealmFS) -> bool {
        if !self.realm.is_active() {
            return false;
        }
        let mountpoint = match self.realm.realmfs_mountpoint() {
            Some(mountpoint) => mountpoint,
            None => return false,
        };

        if let Some(activation) = realmfs.activation() {
            if activation.is_mountpoint(&mountpoint) {
                return false;
            }
        };
        true
    }

    fn render_options(&mut self, config: &RealmConfig) {
        let mut line_one = String::new();
        let mut line_two = StyledString::new();
        let underline = Style::from(Effect::Underline);
        let mut option = |name, value, enabled: bool| {
            if !enabled { return };
            match value {
                None => {
                    line_one.push_str(name);
                    line_one.push(' ');
                },
                Some(_) => {
                    line_two.append_styled(name, underline);
                    line_two.append_plain(" ");
                },
            };
        };

        option("Network", config.use_network, config.network());
        option("X11", config.use_x11, config.x11());
        option("Wayland", config.use_wayland, config.wayland());
        option("Sound", config.use_sound, config.sound());
        option("GPU", config.use_gpu, config.gpu());
        option("KVM", config.use_kvm, config.kvm());
        option("SharedDir", config.use_shared_dir, config.shared_dir());
        option("EphemeralHome", config.use_ephemeral_home, config.ephemeral_home());

        self.heading("Options")
            .newlines(2)
            .print("   ")
            .println(line_one);

        if !line_two.is_empty() {
            self.print("   ").append(line_two).newline();
        }
        self.newline();
    }

    fn render_notes(&self) {
        let notes = match self.realm.notes() {
            Some(notes) => notes,
            None => return,
        };

        self.heading("Notes").newlines(2).dim_style();

        for line in notes.lines() {
            self.print("      ").println(line);
        }
        self.pop();
    }
}

impl <'a> InfoRenderer for RealmInfoRender<'a> {
    fn state(&self) -> Rc<ItemRenderState> {
        self.state.clone()
    }
}
