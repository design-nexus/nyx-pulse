//! The sidebar: only places (chat filters, folders, Saved messages, Archive, Calls)
//! under coloured headings, and the signed-in account at its foot.
use crate::chat_list::{ChatFilter, Place};
use gtk::prelude::*;
use nyx_telegram_core::models::{ChatListId, ChatSummary};
use nyx_ui::sidebar::{self, NavItem};
use nyx_ui::widgets as w;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

type PlaceFn = Rc<dyn Fn(Place)>;

pub struct Places {
    pub root: gtk::Box,
    all: NavItem,
    unread: NavItem,
    personal: NavItem,
    groups: NavItem,
    channels: NavItem,
    folder_box: gtk::Box,
    folders: RefCell<Vec<(i32, NavItem)>>,
    saved: NavItem,
    archive: NavItem,
    calls: NavItem,
    active: RefCell<Place>,
    on_place: RefCell<Option<PlaceFn>>,
    account_avatar: gtk::Box,
    account_name: gtk::Label,
    account_status: gtk::Label,
    account_icon: gtk::Image,
}

impl Places {
    pub fn new() -> Rc<Self> {
        install_icons();
        let root = w::vbox(0);
        root.add_css_class("settings-navigation");
        root.add_css_class("sidebar");
        root.add_css_class("pulse-places");
        root.set_hexpand(false);
        let list = w::vbox(1);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&list)
            .build();
        root.append(&scroller);

        list.append(&rail_divider(sidebar::ACCENT_SECTION));
        let chats = sidebar::group_label("Chats", true);
        sidebar::set_section(&chats, sidebar::ACCENT_SECTION);
        list.append(&chats);
        let all = sidebar::nav_item("pulse-chats-symbolic", "All chats", "Every chat in your main list");
        let unread = sidebar::nav_item("mail-unread-symbolic", "Unread", "Chats with unread messages");
        let personal = sidebar::nav_item("avatar-default-symbolic", "Personal", "Chats with one person");
        let groups = sidebar::nav_item("pulse-groups-symbolic", "Groups", "Groups you're in");
        let channels = sidebar::nav_item("pulse-channels-symbolic", "Channels", "Channels you follow");
        for item in [&all, &unread, &personal, &groups, &channels] {
            list.append(&item.button);
        }

        list.append(&rail_divider("swatch-violet"));
        let folders =
            sidebar::group_label_with_add("Folders", false, "Create, edit and share folders", crate::folders::manage);
        sidebar::set_section(&folders, "swatch-violet");
        list.append(&folders);
        let folder_box = w::vbox(1);
        list.append(&folder_box);

        list.append(&rail_divider("swatch-amber"));
        let yours = sidebar::group_label("Yours", false);
        sidebar::set_section(&yours, "swatch-amber");
        list.append(&yours);
        let saved = sidebar::nav_item("user-bookmarks-symbolic", "Saved messages", "Messages you saved for yourself");
        let archive = sidebar::nav_item("pulse-archive-symbolic", "Archive", "Archived chats");
        let calls = sidebar::nav_item("call-start-symbolic", "Calls", "Your recent calls");
        for item in [&saved, &archive, &calls] {
            list.append(&item.button);
        }

        let foot = gtk::Button::new();
        foot.add_css_class("account-foot");
        foot.set_tooltip_text(Some("Your account: switch, add or sign out"));
        let face = w::hbox(10);
        let account_avatar = w::hbox(0);
        face.append(&account_avatar);
        let text = w::vbox(1);
        text.add_css_class("compact-hide");
        text.set_hexpand(true);
        let account_name = w::label("Telegram", "account-name");
        account_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&account_name);
        let status = w::hbox(4);
        let account_icon = gtk::Image::from_icon_name("network-offline-symbolic");
        account_icon.add_css_class("account-state");
        status.append(&account_icon);
        let account_status = w::label("Connecting…", "account-status");
        account_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.append(&account_status);
        text.append(&status);
        face.append(&text);
        let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
        chevron.add_css_class("compact-hide");
        chevron.add_css_class("dim");
        face.append(&chevron);
        foot.set_child(Some(&face));
        foot.update_property(&[gtk::accessible::Property::Label("Account")]);
        root.append(&foot);
        foot.connect_clicked(account_menu);

        let places = Rc::new(Self {
            root,
            all,
            unread,
            personal,
            groups,
            channels,
            folder_box,
            folders: RefCell::new(Vec::new()),
            saved,
            archive,
            calls,
            active: RefCell::new(Place::Chats(ChatListId::Main, ChatFilter::All)),
            on_place: RefCell::new(None),
            account_avatar,
            account_name,
            account_status,
            account_icon,
        });
        let fixed = [
            (&places.all, Place::Chats(ChatListId::Main, ChatFilter::All)),
            (&places.unread, Place::Chats(ChatListId::Main, ChatFilter::Unread)),
            (&places.personal, Place::Chats(ChatListId::Main, ChatFilter::Personal)),
            (&places.groups, Place::Chats(ChatListId::Main, ChatFilter::Groups)),
            (&places.channels, Place::Chats(ChatListId::Main, ChatFilter::Channels)),
            (&places.saved, Place::Saved),
            (&places.archive, Place::Chats(ChatListId::Archive, ChatFilter::All)),
            (&places.calls, Place::Calls),
        ];
        for (item, place) in fixed {
            let weak = Rc::downgrade(&places);
            item.button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.choose(place);
                }
            });
        }
        places.set_account(None);
        places.set_folders(&[]);
        places
    }

    pub fn connect_place(&self, f: impl Fn(Place) + 'static) {
        *self.on_place.borrow_mut() = Some(Rc::new(f));
    }

    pub fn choose(&self, place: Place) {
        *self.active.borrow_mut() = place;
        self.mark_active();
        let f = self.on_place.borrow().clone();
        if let Some(f) = f {
            f(place);
        }
    }

    /// Back to All chats without telling the list (it resets itself).
    pub fn reset(self: &Rc<Self>) {
        *self.active.borrow_mut() = Place::Chats(ChatListId::Main, ChatFilter::All);
        self.set_folders(&[]);
        self.mark_active();
    }

    fn mark_active(&self) {
        let active = *self.active.borrow();
        let fixed = [
            (&self.all, Place::Chats(ChatListId::Main, ChatFilter::All)),
            (&self.unread, Place::Chats(ChatListId::Main, ChatFilter::Unread)),
            (&self.personal, Place::Chats(ChatListId::Main, ChatFilter::Personal)),
            (&self.groups, Place::Chats(ChatListId::Main, ChatFilter::Groups)),
            (&self.channels, Place::Chats(ChatListId::Main, ChatFilter::Channels)),
            (&self.saved, Place::Saved),
            (&self.archive, Place::Chats(ChatListId::Archive, ChatFilter::All)),
            (&self.calls, Place::Calls),
        ];
        for (item, place) in fixed {
            item.set_active(place == active);
        }
        for (id, item) in self.folders.borrow().iter() {
            item.set_active(active == Place::Chats(ChatListId::Folder(*id), ChatFilter::All));
        }
    }

    pub fn set_folders(self: &Rc<Self>, infos: &[Value]) {
        while let Some(child) = self.folder_box.first_child() {
            self.folder_box.remove(&child);
        }
        self.folders.borrow_mut().clear();
        for (i, info) in infos.iter().enumerate() {
            let Some(id) = info["id"].as_i64().map(|id| id as i32) else { continue };
            let name = crate::folders::name(info);
            let item = sidebar::nav_item("folder-symbolic", &name, &format!("Chats in {name}"));
            if let Some(content) = item.button.child() {
                if let Some(icon) = content.first_child() {
                    icon.set_visible(false);
                }
                let dot = w::hbox(0);
                dot.add_css_class("folder-dot");
                dot.set_valign(gtk::Align::Center);
                dot.set_halign(gtk::Align::Center);
                sidebar::set_section(&dot, &folder_section(info, i));
                if let Some(content) = content.downcast_ref::<gtk::Box>() {
                    content.prepend(&dot);
                }
            }
            item.set_count(crate::folders::unread(id).filter(|n| *n > 0).map(|n| n as u64));
            let weak = Rc::downgrade(self);
            item.button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.choose(Place::Chats(ChatListId::Folder(id), ChatFilter::All));
                }
            });
            self.folder_box.append(&item.button);
            self.folders.borrow_mut().push((id, item));
        }
        if self.folders.borrow().is_empty() {
            let hint = w::label("Group chats your way with +", "folders-hint");
            hint.add_css_class("compact-hide");
            hint.set_visible(!self.root.has_css_class("compact"));
            self.folder_box.append(&hint);
        }
        let active = *self.active.borrow();
        if let Place::Chats(ChatListId::Folder(id), _) = active
            && !self.folders.borrow().iter().any(|(f, _)| *f == id)
        {
            self.choose(Place::Chats(ChatListId::Main, ChatFilter::All));
        }
        self.mark_active();
        self.refresh_compact();
    }

    /// Folder counts come from Telegram; the rest from the chats loaded so far.
    pub fn refresh_counts(&self, chats: &[ChatSummary]) {
        let main: Vec<&ChatSummary> = chats.iter().filter(|c| c.list_position(ChatListId::Main).is_some()).collect();
        let unread = |c: &ChatSummary| c.unread_count > 0 || c.is_marked_unread();
        let messages: i64 = main.iter().filter(|c| !c.is_muted).map(|c| c.unread_count.max(0) as i64).sum();
        self.all.set_badge(Some(messages as u64));
        self.unread.set_count(Some(main.iter().filter(|c| unread(c)).count() as u64));
        let of = |kind: ChatFilter| main.iter().filter(|c| kind.keeps(c) && unread(c)).count() as u64;
        self.personal.set_count(Some(of(ChatFilter::Personal)));
        self.groups.set_count(Some(of(ChatFilter::Groups)));
        self.channels.set_count(Some(of(ChatFilter::Channels)));
        let archived = chats.iter().filter(|c| c.list_position(ChatListId::Archive).is_some()).count() as i64;
        self.archive.set_count(Some(crate::folders::archive_total().unwrap_or(archived).max(0) as u64));
        for (id, item) in self.folders.borrow().iter() {
            item.set_count(crate::folders::unread(*id).filter(|n| *n > 0).map(|n| n as u64));
        }
        self.refresh_compact();
    }

    /// Readouts set while the rail shows must stay hidden.
    fn refresh_compact(&self) {
        if self.root.has_css_class("compact") {
            self.set_compact(true);
        }
    }

    /// The icon rail, with a bar in each group's colour where the group starts.
    pub fn set_compact(&self, compact: bool) {
        sidebar::set_compact(&self.root, compact);
        fn walk(widget: &gtk::Widget, compact: bool) {
            if widget.has_css_class("compact-show") {
                widget.set_visible(compact);
            }
            let mut child = widget.first_child();
            while let Some(c) = child {
                walk(&c, compact);
                child = c.next_sibling();
            }
        }
        walk(self.root.upcast_ref(), compact);
    }

    pub fn set_account(&self, me: Option<&Value>) {
        while let Some(child) = self.account_avatar.first_child() {
            self.account_avatar.remove(&child);
        }
        let name = me
            .map(|u| format!("{} {}", u["first_name"].as_str().unwrap_or(""), u["last_name"].as_str().unwrap_or("")))
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Telegram".into());
        let photo = me
            .and_then(|u| u["profile_photo"]["small"]["local"]["path"].as_str())
            .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
            .map(str::to_string);
        self.account_avatar.append(&crate::chat_list::avatar(&name, photo.as_deref(), 28));
        self.account_name.set_text(&name);
        self.root.update_property(&[gtk::accessible::Property::Description(&name)]);
    }

    /// "Connected", "Connecting…", "Waiting for network…".
    pub fn set_connection(&self, text: &str, connected: bool) {
        self.account_status.set_text(text);
        self.account_icon.set_icon_name(Some(if connected { "emblem-ok-symbolic" } else { "network-offline-symbolic" }));
        if connected {
            self.account_icon.add_css_class("connected");
        } else {
            self.account_icon.remove_css_class("connected");
        }
    }
}

/// Where a group starts in the icon rail: a short bar in its colour.
fn rail_divider(section: &str) -> gtk::Separator {
    let bar = gtk::Separator::new(gtk::Orientation::Horizontal);
    bar.add_css_class("nav-divider");
    bar.add_css_class("compact-show");
    bar.add_css_class(&format!("section-{section}"));
    bar.set_visible(false);
    bar
}

/// A folder's colour: the one chosen in Telegram, or the next swatch.
fn folder_section(info: &Value, index: usize) -> String {
    const TELEGRAM: [&str; 7] = ["red", "coral", "violet", "green", "teal", "blue", "pink"];
    match info["color_id"].as_i64() {
        Some(c) if (0..7).contains(&c) => format!("swatch-{}", TELEGRAM[c as usize]),
        _ => sidebar::rotating_section(index + 2),
    }
}

fn account_menu(button: &gtk::Button) {
    let (p, body) = crate::actions::popover(button.upcast_ref(), None);
    crate::actions::menu_item(&p, &body, "Profile…", crate::account_tools::profile);
    crate::actions::menu_item(&p, &body, "Account, privacy and tools…", crate::features::hub);
    let mgr = nyx_telegram_core::account::AccountManager::new(nyx_core::paths::data_dir(), nyx_core::paths::cache_dir());
    let active = crate::prefs::get().active_account_id;
    let others: Vec<String> = mgr.list_accounts().into_iter().filter(|id| *id != active).collect();
    if !others.is_empty() {
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    for (i, id) in others.into_iter().enumerate() {
        crate::actions::menu_item(&p, &body, &format!("Switch to account {}", i + 2), move || {
            crate::window::switch_account(id.clone())
        });
    }
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    crate::actions::menu_item(&p, &body, "Add an account", crate::window::add_account);
    crate::actions::menu_item(&p, &body, "Sign out…", crate::window::log_out);
    p.popup();
}

/// Symbolic icons the icon theme may not have, written to the cache and added to
/// the theme's search path. Filled shapes: GTK fills every path of a symbolic icon.
fn install_icons() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let dir = nyx_core::paths::cache_dir().join("pulse-icons");
    for (name, body) in ICONS {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\"><g fill=\"#000000\">{body}</g></svg>\n"
        );
        let path = dir.join(format!("{name}.svg"));
        if std::fs::read_to_string(&path).ok().as_deref() != Some(svg.as_str()) {
            let _ = nyx_core::cmd::atomic_write(&path, &svg);
        }
    }
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(dir);
    }
}

const ICONS: &[(&str, &str)] = &[
    (
        "pulse-chats-symbolic",
        r#"<path fill-rule="evenodd" d="M5.5 3h13A3.5 3.5 0 0 1 22 6.5v8a3.5 3.5 0 0 1-3.5 3.5H12l-5.2 3.7A.5.5 0 0 1 6 21.3V18h-.5A3.5 3.5 0 0 1 2 14.5v-8A3.5 3.5 0 0 1 5.5 3zm0 2A1.5 1.5 0 0 0 4 6.5v8A1.5 1.5 0 0 0 5.5 16H8v2.2l3.4-2.2h7.1a1.5 1.5 0 0 0 1.5-1.5v-8A1.5 1.5 0 0 0 18.5 5z"/>"#,
    ),
    (
        "pulse-groups-symbolic",
        r#"<circle cx="9" cy="8" r="3.6"/><path d="M2 19.4C2 15.9 5.1 13.8 9 13.8s7 2.1 7 5.6v.6H2z"/><circle cx="17.2" cy="8.6" r="2.7"/><path d="M17.6 13c2.6.2 4.4 2 4.4 4.7v.8h-4.3c-.2-2.2-1.1-4.1-2.7-5.3.8-.1 1.7-.2 2.6-.2z"/>"#,
    ),
    (
        "pulse-channels-symbolic",
        r#"<path d="M3.2 9h2.9l8.4-4.6a.6.6 0 0 1 .9.5v14.2a.6.6 0 0 1-.9.5L6.1 15H3.2A1.2 1.2 0 0 1 2 13.8v-3.6A1.2 1.2 0 0 1 3.2 9z"/><path d="M6.6 16h2.6l1.3 4a.8.8 0 0 1-.8 1.1H8.4a.8.8 0 0 1-.8-.6z"/><rect x="17.5" y="11.1" width="4.5" height="1.8" rx=".9"/><rect x="17.2" y="6.4" width="4.4" height="1.8" rx=".9" transform="rotate(-30 19.4 7.3)"/><rect x="17.2" y="15.8" width="4.4" height="1.8" rx=".9" transform="rotate(30 19.4 16.7)"/>"#,
    ),
    (
        "pulse-archive-symbolic",
        r#"<path fill-rule="evenodd" d="M3.2 3.5h17.6c.7 0 1.2.5 1.2 1.2v3.1c0 .7-.5 1.2-1.2 1.2H3.2C2.5 9 2 8.5 2 7.8V4.7c0-.7.5-1.2 1.2-1.2zM3.5 10.5h17V18a2.5 2.5 0 0 1-2.5 2.5H6A2.5 2.5 0 0 1 3.5 18zm6 2.5a1 1 0 0 0 0 2h5a1 1 0 0 0 0-2z"/>"#,
    ),
];

#[cfg(test)]
pub fn check_places(chat: &ChatSummary) {
    let places = Places::new();
    let mut unread = chat.clone();
    unread.unread_count = 3;
    places.refresh_counts(&[unread]);
    assert_eq!(places.all.button.child().and_then(|c| c.last_child()).and_downcast::<gtk::Label>().unwrap().text(), "3");
    let chosen = Rc::new(RefCell::new(None));
    let c = chosen.clone();
    places.connect_place(move |p| *c.borrow_mut() = Some(p));
    places.calls.button.emit_clicked();
    assert_eq!(*chosen.borrow(), Some(Place::Calls));
    assert!(places.calls.button.has_css_class("active"));
    assert!(!places.all.button.has_css_class("active"));
    assert_eq!(ChatFilter::Groups.keeps(chat), chat.chat_type == nyx_telegram_core::models::ChatType::BasicGroup);
}
