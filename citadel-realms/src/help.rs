use cursive::views::{DummyView, LinearLayout, TextView, PaddedView, OnEventView, Panel};
use cursive::traits::{View,Boxable};
use cursive::utils::markup::StyledString;
use cursive::theme::ColorStyle;
use cursive::align::HAlign;

const REALM_SCREEN: usize = 1;

pub fn help_panel(screen: usize) -> impl View {

    let content = if screen == REALM_SCREEN {
            LinearLayout::vertical()
            .child(help_header("Realms Commands"))
                .child(DummyView)
                .child(TextView::new(autostart_text()))
                .child(DummyView)

                .child(help_item_autostart("Enter", "Set selected realm as Current."))
                .child(help_item_autostart("$ #", "Open user/root shell in selected realm."))
                .child(help_item_autostart("t", "Open terminal for selected realm."))
                .child(help_item_autostart("s", "Start/Stop selected realm."))
                .child(help_item("c", "Configure selected realm."))
                .child(help_item("d", "Delete selected realm."))
                .child(help_item("n", "Create a new realm."))
                .child(help_item("r", "Restart currently selected realm."))
                .child(help_item("u", "Open shell to update RealmFS image of selected realm."))
                .child(help_item(".", "Toggle display of system realms."))
                .child(DummyView)
        } else {
            LinearLayout::vertical()
                .child(help_header("RealmsFS Image Commands"))
                .child(DummyView)
                .child(help_item("n", "Create new RealmFS as fork of selected image."))
                .child(help_item("s", "Seal selected RealmFS image."))
                .child(help_item("u", "Open shell to update selected RealmFS image."))
                .child(help_item(".", "Toggle display of system RealmFS images."))
                .child(DummyView)
        }

        .child(help_header("Global Commands"))
        .child(DummyView)
        .child(help_item("Space", "Toggle between Realms and RealmFS views."))
        .child(help_item("q", "Exit application."))
        .child(help_item("l", "Toggle visibility of log panel."))
        .child(help_item("L", "Display full sized log view."))
        .child(help_item("T", "Select a UI color theme."))
        .child(DummyView)
        .child(TextView::new(footer_text()));


    let content = PaddedView::new((2,2,1,1), content);
    let panel = Panel::new(content)
        .title("Help");

    OnEventView::new(panel)
        .on_pre_event('?', |s| { s.pop_layer(); })
        .on_pre_event('h', |s| { s.pop_layer(); })
}

fn autostart_text() -> StyledString {
    let mut text = StyledString::styled("[", ColorStyle::tertiary());
    text.append(autostart_icon());
    text.append_styled("] Start Realm if not currently running", ColorStyle::tertiary());
    text
}

fn footer_text() -> StyledString {
    StyledString::styled("'q' or ESC to close help panel", ColorStyle::tertiary())
}

fn autostart_icon() -> StyledString {
    StyledString::styled("*", ColorStyle::title_primary())
}

fn help_item_autostart(keys: &str, help: &str) -> impl View {
    _help_item(keys, help, true)
}

fn help_item(keys: &str, help: &str) -> impl View {
    _help_item(keys, help, false)
}

fn _help_item(keys: &str, help: &str, start: bool) -> impl View {
    let keys = StyledString::styled(keys, ColorStyle::secondary());
    let mut text = if start {
        autostart_icon()
    } else {
        StyledString::plain(" ")
    };
    text.append_plain(" ");
    text.append_plain(help);

    LinearLayout::horizontal()
        .child(TextView::new(keys).h_align(HAlign::Right).fixed_width(8))
        .child(DummyView.fixed_width(4))
        .child(TextView::new(text))
}

fn help_header(text: &str) -> impl View {
    let text = StyledString::styled(text, ColorStyle::title_primary());
    TextView::new(text).h_align(HAlign::Left)
}
