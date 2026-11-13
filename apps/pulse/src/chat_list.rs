//! The chat list between the sidebar and the conversation: the chats of the place
//! chosen in the sidebar, matching messages while the header search has text, and
//! recent calls under Calls.
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use nyx_telegram_core::client::parse_message;
use nyx_telegram_core::models::{ChatId, ChatListId, ChatSummary, ChatType, MessageContent, MessageSummary};
use nyx_ui::widgets as w;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Which chats of a list a place shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatFilter {
    All,
    Unread,
    Personal,
    Groups,
    Channels,
}

impl ChatFilter {
    pub fn keeps(self, chat: &ChatSummary) -> bool {
        match self {
            ChatFilter::All => true,
            ChatFilter::Unread => chat.unread_count > 0 || chat.is_marked_unread(),
            ChatFilter::Personal => chat.chat_type == ChatType::Private,
            ChatFilter::Groups => matches!(chat.chat_type, ChatType::BasicGroup | ChatType::Supergroup),
            ChatFilter::Channels => chat.chat_type == ChatType::Channel,
        }
    }
}

/// A place in the sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Chats(ChatListId, ChatFilter),
    Saved,
    Calls,
}

struct Inner {
    place: Place,
    search_query: String,
    all_chats: Vec<ChatSummary>,
    active_chat_id: Option<ChatId>,
    buttons: HashMap<ChatId, gtk::Button>,
    on_select: Option<Rc<dyn Fn(ChatId)>>,
}

type ChangedFn = Rc<dyn Fn()>;

pub struct ChatList {
    pub root: gtk::Box,
    chats_heading: gtk::Label,
    rows: gtk::Box,
    found_heading: gtk::Label,
    found: gtk::Box,
    calls: gtk::Box,
    empty: gtk::Box,
    load_limit: Cell<i32>,
    search_revision: Rc<Cell<u64>>,
    calls_revision: Rc<Cell<u64>>,
    changed_pending: Rc<Cell<bool>>,
    on_changed: RefCell<Option<ChangedFn>>,
    inner: Rc<RefCell<Inner>>,
}

impl ChatList {
    pub fn new(search: &gtk::SearchEntry) -> Rc<Self> {
        let root = w::vbox(0);
        root.add_css_class("chat-list-pane");
        root.set_hexpand(false);
        let content = w::vbox(0);
        content.add_css_class("chat-list");
        let chats_heading = w::label("CHATS", "list-heading");
        chats_heading.set_visible(false);
        content.append(&chats_heading);
        let rows = w::vbox(1);
        content.append(&rows);
        let found_heading = w::label("MESSAGES", "list-heading");
        found_heading.set_visible(false);
        content.append(&found_heading);
        let found = w::vbox(1);
        content.append(&found);
        let calls = w::vbox(1);
        calls.set_visible(false);
        content.append(&calls);
        let empty = w::vbox(0);
        empty.set_vexpand(true);
        content.append(&empty);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&content)
            .build();
        root.append(&scroller);

        let inner = Rc::new(RefCell::new(Inner {
            place: Place::Chats(ChatListId::Main, ChatFilter::All),
            search_query: String::new(),
            all_chats: Vec::new(),
            active_chat_id: None,
            buttons: HashMap::new(),
            on_select: None,
        }));
        let list = Rc::new(Self {
            root,
            chats_heading,
            rows,
            found_heading,
            found,
            calls,
            empty,
            load_limit: Cell::new(50),
            search_revision: Default::default(),
            calls_revision: Default::default(),
            changed_pending: Default::default(),
            on_changed: RefCell::new(None),
            inner,
        });
        let weak = Rc::downgrade(&list);
        scroller.connect_edge_reached(move |_, edge| {
            if edge == gtk::PositionType::Bottom
                && let Some(list) = weak.upgrade()
            {
                list.load_more();
            }
        });
        let weak = Rc::downgrade(&list);
        search.connect_search_changed(move |entry| {
            if let Some(list) = weak.upgrade() {
                list.inner.borrow_mut().search_query = entry.text().trim().to_lowercase();
                list.rebuild_list();
                list.search_messages(entry.text().trim().to_string());
            }
        });
        list.rebuild_list();
        list
    }

    /// Called (once per main-loop turn) whenever the chats or their counts change.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        *self.on_changed.borrow_mut() = Some(Rc::new(f));
    }

    fn changed(&self) {
        if self.changed_pending.replace(true) {
            return;
        }
        let f = self.on_changed.borrow().clone();
        let pending = self.changed_pending.clone();
        glib::idle_add_local_once(move || {
            pending.set(false);
            if let Some(f) = f {
                f();
            }
        });
    }

    pub fn set_place(&self, place: Place) {
        self.inner.borrow_mut().place = place;
        self.load_limit.set(50);
        self.rebuild_list();
        match place {
            Place::Chats(ChatListId::Main, _) => {}
            Place::Chats(list, _) => crate::window::with_service(|s| s.load_chat_list(list, self.load_limit.get())),
            Place::Saved => crate::window::open_saved_messages(),
            Place::Calls => self.load_calls(),
        }
    }

    pub fn place(&self) -> Place {
        self.inner.borrow().place
    }

    fn load_more(&self) {
        if self.place() == Place::Calls {
            return;
        }
        let limit = self.load_limit.get().saturating_add(50).min(10000);
        self.load_limit.set(limit);
        let list = self.current_list();
        crate::window::with_service(|s| s.load_chat_list(list, limit));
    }

    /// Sets the callback invoked when a conversation is selected.
    pub fn connect_chat_selected<F: Fn(ChatId) + 'static>(&self, callback: F) {
        self.inner.borrow_mut().on_select = Some(Rc::new(callback));
    }

    pub fn get_chat(&self, chat_id: ChatId) -> Option<ChatSummary> {
        self.inner.borrow().all_chats.iter().find(|c| c.id == chat_id).cloned()
    }

    pub fn reset(&self) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.active_chat_id = None;
            inner.all_chats.clear();
            inner.place = Place::Chats(ChatListId::Main, ChatFilter::All);
        }
        self.load_limit.set(50);
        self.rebuild_list();
        self.changed();
    }

    pub fn get_all_chats(&self) -> Vec<ChatSummary> {
        self.inner.borrow().all_chats.clone()
    }

    pub fn select_chat(&self, chat_id: ChatId) {
        Self::do_select_chat(&self.inner, chat_id);
    }

    fn do_select_chat(inner: &Rc<RefCell<Inner>>, chat_id: ChatId) {
        {
            let mut inr = inner.borrow_mut();
            inr.active_chat_id = Some(chat_id);
            for (&id, btn) in inr.buttons.iter() {
                if id == chat_id {
                    btn.add_css_class("active");
                } else {
                    btn.remove_css_class("active");
                }
            }
        }
        let cb = inner.borrow().on_select.clone();
        if let Some(cb) = cb {
            cb(chat_id);
        }
    }

    /// The Telegram list the place shows (Main for Saved messages and Calls).
    pub fn current_list(&self) -> ChatListId {
        match self.inner.borrow().place {
            Place::Chats(list, _) => list,
            _ => ChatListId::Main,
        }
    }

    pub fn clear_selection(&self) {
        self.inner.borrow_mut().active_chat_id = None;
        for button in self.inner.borrow().buttons.values() {
            button.remove_css_class("active");
        }
    }

    pub fn update_chats(&self, chats: &[ChatSummary]) {
        {
            let mut inner = self.inner.borrow_mut();
            for chat in chats {
                if let Some(old) = inner.all_chats.iter_mut().find(|c| c.id == chat.id) {
                    *old = chat.clone();
                } else {
                    inner.all_chats.push(chat.clone());
                }
            }
        }
        self.rebuild_list();
        self.changed();
        if self.inner.borrow().active_chat_id.is_none() {
            let chosen = {
                let inner = self.inner.borrow();
                let list = match inner.place {
                    Place::Chats(list, _) => list,
                    _ => ChatListId::Main,
                };
                inner
                    .all_chats
                    .iter()
                    .filter(|c| c.list_position(list).is_some())
                    .find(|c| Some(c.id) == crate::prefs::get().last_chat_id)
                    .map(|c| c.id)
            };
            if let Some(id) = chosen {
                self.select_chat(id);
            }
        }
    }

    /// Whether the current place and search show `chat`.
    fn shows(inner: &Inner, chat: &ChatSummary) -> bool {
        let query = &inner.search_query;
        if !query.is_empty() {
            let list = match inner.place {
                Place::Chats(list, _) => list,
                _ => ChatListId::Main,
            };
            return chat.list_position(list).is_some() && matches_query(chat, query);
        }
        match inner.place {
            Place::Chats(list, filter) => chat.list_position(list).is_some() && filter.keeps(chat),
            Place::Saved => is_saved_messages(chat),
            Place::Calls => false,
        }
    }

    fn sort_key(inner: &Inner, chat: &ChatSummary) -> u64 {
        let list = match inner.place {
            Place::Chats(list, _) => list,
            _ => ChatListId::Main,
        };
        chat.list_position(list).map(|p| p.0).unwrap_or(0)
    }

    pub fn upsert_chat(&self, chat: &ChatSummary) {
        {
            let mut inner = self.inner.borrow_mut();
            if let Some(old) = inner.all_chats.iter_mut().find(|c| c.id == chat.id) {
                *old = chat.clone();
            } else {
                inner.all_chats.push(chat.clone());
            }
            let keys: HashMap<ChatId, u64> = inner.all_chats.iter().map(|c| (c.id, Self::sort_key(&inner, c))).collect();
            inner.all_chats.sort_by_key(|c| std::cmp::Reverse(keys.get(&c.id).copied().unwrap_or(0)));
        }
        let old = self.inner.borrow_mut().buttons.remove(&chat.id);
        let focused = old.as_ref().is_some_and(|b| b.has_focus());
        if let Some(old) = old
            && let Some(parent) = old.parent().and_downcast::<gtk::Box>()
        {
            parent.remove(&old);
        }
        let shown = Self::shows(&self.inner.borrow(), chat);
        if shown {
            let (projected, preceding, active) = {
                let inner = self.inner.borrow();
                let list = match inner.place {
                    Place::Chats(list, _) => list,
                    _ => ChatListId::Main,
                };
                let projected = chat.for_list(list).unwrap_or_else(|| chat.clone());
                let index = inner.all_chats.iter().position(|c| c.id == chat.id).unwrap_or(0);
                let preceding = inner.all_chats[..index]
                    .iter()
                    .rev()
                    .find(|c| Self::shows(&inner, c))
                    .and_then(|c| inner.buttons.get(&c.id).cloned());
                (projected, preceding, inner.active_chat_id == Some(chat.id))
            };
            let button = self.row_for(&projected, active);
            self.rows.insert_child_after(&button, preceding.as_ref());
            if focused {
                button.grab_focus();
            }
            self.inner.borrow_mut().buttons.insert(chat.id, button);
        }
        self.refresh_empty();
        self.changed();
    }

    pub fn remove_chat(&self, id: i64) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.all_chats.retain(|c| c.id != id);
            if inner.active_chat_id == Some(id) {
                inner.active_chat_id = None;
            }
        }
        let button = self.inner.borrow_mut().buttons.remove(&id);
        if let Some(button) = button
            && let Some(parent) = button.parent().and_downcast::<gtk::Box>()
        {
            parent.remove(&button);
        }
        self.refresh_empty();
        self.changed();
    }

    fn row_for(&self, chat: &ChatSummary, active: bool) -> gtk::Button {
        let button = create_chat_item(chat, active);
        let weak = Rc::downgrade(&self.inner);
        let id = chat.id;
        button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self::do_select_chat(&inner, id)
            }
        });
        bind_row_actions(&button, chat.id);
        button
    }

    fn rebuild_list(&self) {
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        self.inner.borrow_mut().buttons.clear();
        let (mut chats, active_id) = {
            let inner = self.inner.borrow();
            let list = match inner.place {
                Place::Chats(list, _) => list,
                _ => ChatListId::Main,
            };
            let chats: Vec<ChatSummary> = inner
                .all_chats
                .iter()
                .filter(|c| Self::shows(&inner, c))
                .map(|c| c.for_list(list).unwrap_or_else(|| c.clone()))
                .collect();
            (chats, inner.active_chat_id)
        };
        chats.sort_by_key(|c| std::cmp::Reverse(c.order));
        for chat in chats {
            let button = self.row_for(&chat, active_id == Some(chat.id));
            self.rows.append(&button);
            self.inner.borrow_mut().buttons.insert(chat.id, button);
        }
        let place = self.place();
        self.calls.set_visible(place == Place::Calls && self.inner.borrow().search_query.is_empty());
        self.refresh_empty();
    }

    fn refresh_empty(&self) {
        while let Some(child) = self.empty.first_child() {
            self.empty.remove(&child);
        }
        let (searching, place) = {
            let inner = self.inner.borrow();
            (!inner.search_query.is_empty(), inner.place)
        };
        let has_rows = self.rows.first_child().is_some();
        self.chats_heading.set_visible(searching && has_rows);
        if has_rows || self.found.first_child().is_some() || (place == Place::Calls && !searching && self.calls.first_child().is_some()) {
            return;
        }
        let (icon, title, desc) = if searching {
            ("edit-find-symbolic", "Nothing found", "No chats match. Messages that match show here as Telegram finds them.")
        } else {
            match place {
                Place::Chats(ChatListId::Main, ChatFilter::All) => ("pulse-chats-symbolic", "No chats yet", "Chats appear here once Telegram has loaded them."),
                Place::Chats(ChatListId::Main, ChatFilter::Unread) => ("mail-unread-symbolic", "All caught up", "No chats have unread messages."),
                Place::Chats(ChatListId::Main, ChatFilter::Personal) => ("avatar-default-symbolic", "No personal chats", "Chats with one person show here."),
                Place::Chats(ChatListId::Main, ChatFilter::Groups) => ("pulse-groups-symbolic", "No groups", "Groups you're in show here."),
                Place::Chats(ChatListId::Main, ChatFilter::Channels) => ("pulse-channels-symbolic", "No channels", "Channels you follow show here."),
                Place::Chats(ChatListId::Archive, _) => ("pulse-archive-symbolic", "Archive is empty", "Chats you archive wait here."),
                Place::Chats(ChatListId::Folder(_), _) => ("folder-symbolic", "No chats in this folder", "Add chats to the folder from a chat's menu."),
                Place::Saved => ("user-bookmarks-symbolic", "Saved messages", "Opening the chat you keep for yourself…"),
                Place::Calls => ("call-start-symbolic", "No calls yet", "Calls you make and receive show here."),
            }
        };
        let state = w::empty_state(icon, title, desc, None);
        state.set_margin_top(48);
        state.set_margin_start(16);
        state.set_margin_end(16);
        self.empty.append(&state);
    }

    /// Messages that match the header search, under the chats that do.
    fn search_messages(&self, query: String) {
        let revision = self.search_revision.get().wrapping_add(1);
        self.search_revision.set(revision);
        while let Some(child) = self.found.first_child() {
            self.found.remove(&child);
        }
        self.found_heading.set_visible(false);
        self.calls.set_visible(query.is_empty() && self.place() == Place::Calls);
        self.refresh_empty();
        if query.chars().count() < 2 {
            return;
        }
        let Some(service) = crate::window::service() else { return };
        let generation = service.generation.clone();
        let found = self.found.downgrade();
        let heading = self.found_heading.downgrade();
        let current = self.search_revision.clone();
        let empty = self.empty.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            if current.get() != revision {
                return;
            }
            let rx = service.request(
                "searchMessages",
                serde_json::json!({"chat_list":null,"query":query,"offset":"","limit":20,"filter":null,"chat_type_filter":null,"min_date":0,"max_date":0}),
            );
            glib::spawn_future_local(async move {
                let Ok(Ok(data)) = rx.recv().await else { return };
                if current.get() != revision || !crate::window::service_is_current(&generation) {
                    return;
                }
                let (Some(found), Some(heading)) = (found.upgrade(), heading.upgrade()) else { return };
                let messages: Vec<MessageSummary> =
                    data["messages"].as_array().into_iter().flatten().filter_map(parse_message).collect();
                for m in &messages {
                    found.append(&found_row(m));
                }
                heading.set_visible(!messages.is_empty());
                if !messages.is_empty()
                    && let Some(empty) = empty.upgrade()
                {
                    while let Some(child) = empty.first_child() {
                        empty.remove(&child);
                    }
                }
            });
        });
    }

    fn load_calls(&self) {
        let revision = self.calls_revision.get().wrapping_add(1);
        self.calls_revision.set(revision);
        while let Some(child) = self.calls.first_child() {
            self.calls.remove(&child);
        }
        self.refresh_empty();
        let Some(service) = crate::window::service() else { return };
        let generation = service.generation.clone();
        let rx = service.request("searchCallMessages", serde_json::json!({"offset":"","limit":50,"only_missed":false}));
        let calls = self.calls.downgrade();
        let empty = self.empty.downgrade();
        let current = self.calls_revision.clone();
        glib::spawn_future_local(async move {
            let result = rx.recv().await;
            if current.get() != revision || !crate::window::service_is_current(&generation) {
                return;
            }
            let (Some(calls), Some(empty)) = (calls.upgrade(), empty.upgrade()) else { return };
            let messages: Vec<MessageSummary> = match result {
                Ok(Ok(data)) => data["messages"].as_array().into_iter().flatten().filter_map(parse_message).collect(),
                Ok(Err(e)) => {
                    crate::window::show_status(&e);
                    Vec::new()
                }
                Err(_) => Vec::new(),
            };
            for m in &messages {
                calls.append(&call_row(m));
            }
            if !messages.is_empty() {
                while let Some(child) = empty.first_child() {
                    empty.remove(&child);
                }
            }
        });
    }
}

fn matches_query(chat: &ChatSummary, query: &str) -> bool {
    chat.title.to_lowercase().contains(query)
        || chat.last_message.as_ref().is_some_and(|m| match &m.content {
            MessageContent::Text(t) => t.text.to_lowercase().contains(query),
            _ => false,
        })
}

/// The chat you keep for yourself.
pub fn is_saved_messages(chat: &ChatSummary) -> bool {
    crate::window::my_id().is_some_and(|me| chat.chat_type == ChatType::Private && chat.data["type"]["user_id"].as_i64() == Some(me))
}

/// A chat's name as Pulse shows it: Saved Messages for the chat with yourself.
pub fn display_title(chat: &ChatSummary) -> String {
    if is_saved_messages(chat) { "Saved Messages".into() } else { chat.title.clone() }
}

fn snippet(chat: &ChatSummary) -> String {
    let Some(m) = chat.last_message.as_ref() else { return String::new() };
    let body = match &m.content {
        MessageContent::Text(t) => t.text.clone(),
        MessageContent::Photo { caption, .. } if !caption.text.is_empty() => format!("📷 {}", caption.text),
        MessageContent::Photo { .. } => "📷 Photo".into(),
        MessageContent::Video { .. } => "📹 Video".into(),
        MessageContent::Document { file_name, .. } => format!("📎 {file_name}"),
        MessageContent::VoiceNote { duration_seconds, .. } => {
            format!("voice message · {}:{:02}", duration_seconds / 60, duration_seconds % 60)
        }
        MessageContent::Sticker { emoji, .. } => format!("{emoji} Sticker"),
        MessageContent::Call { is_video, .. } => if *is_video { "📹 Video call" } else { "📞 Call" }.into(),
        _ => crate::chat_view::preview(m),
    };
    let body = body.lines().next().unwrap_or("").trim().to_string();
    let group = matches!(chat.chat_type, ChatType::BasicGroup | ChatType::Supergroup);
    match m.sender_name.split_whitespace().next() {
        Some(first) if group && !m.is_outgoing => format!("{first}: {body}"),
        _ => body,
    }
}

fn row_text(top: &gtk::Box, bottom: &gtk::Box) -> gtk::Box {
    let text = w::vbox(3);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(top);
    text.append(bottom);
    text
}

fn create_chat_item(chat: &ChatSummary, is_active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("chat-row");
    if is_active {
        button.add_css_class("active");
    }
    let content = w::hbox(11);
    content.append(&build_avatar_widget(chat, 40));

    let top = w::hbox(5);
    let name = display_title(chat);
    let title = w::label(&if chat.is_secret { format!("🔒 {name}") } else { name.clone() }, "chat-title");
    if chat.is_secret {
        title.add_css_class("secret-chat");
    }
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&title);
    if chat.is_muted {
        let mute = gtk::Image::from_icon_name("notifications-disabled-symbolic");
        mute.add_css_class("row-mark");
        mute.set_tooltip_text(Some("Muted"));
        top.append(&mute);
    }
    if let Some(m) = chat.last_message.as_ref().filter(|m| m.is_outgoing) {
        let read = chat.data["last_read_outbox_message_id"].as_i64().unwrap_or(0) >= m.id;
        let tick = w::label(if read { "✓✓" } else { "✓" }, "row-tick");
        tick.set_tooltip_text(Some(if read { "Read" } else { "Sent" }));
        top.append(&tick);
    }
    let time = chat.last_message.as_ref().map(|m| format_time(m.date)).unwrap_or_default();
    top.append(&w::label(&time, "chat-time"));

    let bottom = w::hbox(5);
    let snippet_lbl = w::label(&snippet(chat), "chat-snippet");
    snippet_lbl.set_hexpand(true);
    snippet_lbl.set_single_line_mode(true);
    snippet_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    bottom.append(&snippet_lbl);
    let unread = chat.unread_count > 0 || chat.is_marked_unread();
    if chat.is_pinned && !unread {
        let pin = gtk::Image::from_icon_name("view-pin-symbolic");
        pin.add_css_class("row-mark");
        pin.set_tooltip_text(Some("Pinned"));
        bottom.append(&pin);
    }
    if unread {
        let badge = w::label(&if chat.unread_count > 0 { chat.unread_count.to_string() } else { "●".into() }, "unread-badge");
        if chat.is_muted {
            badge.add_css_class("muted");
        }
        badge.set_xalign(0.5);
        badge.set_valign(gtk::Align::Center);
        bottom.append(&badge);
    }
    let text = row_text(&top, &bottom);
    let tags = crate::folders::tags(chat);
    if !tags.is_empty() {
        let label = w::label(&tags.join(" · "), "folder-tags");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&label);
    }
    content.append(&text);
    button.set_child(Some(&content));
    let tooltip = if unread { format!("{name} ({} unread)", chat.unread_count) } else { name };
    button.set_tooltip_text(Some(&tooltip));
    button
}

/// A message the header search found: its chat, when, and the text.
fn found_row(m: &MessageSummary) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("chat-row");
    let content = w::hbox(11);
    let chat = crate::window::chat(m.chat_id);
    let title = chat.as_ref().map(display_title).unwrap_or_else(|| m.sender_name.clone());
    content.append(&match &chat {
        Some(c) => build_avatar_widget(c, 40),
        None => avatar(&title, None, 40),
    });
    let top = w::hbox(5);
    let name = w::label(&title, "chat-title");
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&name);
    top.append(&w::label(&format_time(m.date), "chat-time"));
    let bottom = w::hbox(5);
    let text = crate::chat_view::preview(m);
    let line = w::label(text.lines().next().unwrap_or(""), "chat-snippet");
    line.set_hexpand(true);
    line.set_ellipsize(gtk::pango::EllipsizeMode::End);
    bottom.append(&line);
    content.append(&row_text(&top, &bottom));
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(&format!("Open this message in {title}")));
    let message = m.clone();
    button.connect_clicked(move |_| crate::window::jump_message(&message));
    button
}

/// A call: who, which way, how long, and when.
fn call_row(m: &MessageSummary) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("chat-row");
    let content = w::hbox(11);
    let chat = crate::window::chat(m.chat_id);
    let title = chat.as_ref().map(display_title).unwrap_or_else(|| m.sender_name.clone());
    content.append(&match &chat {
        Some(c) => build_avatar_widget(c, 40),
        None => avatar(&title, None, 40),
    });
    let top = w::hbox(5);
    let name = w::label(&title, "chat-title");
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&name);
    top.append(&w::label(&format_time(m.date), "chat-time"));
    let bottom = w::hbox(5);
    let (duration, video, reason) = match &m.content {
        MessageContent::Call { duration_seconds, is_video, discard_reason } => (*duration_seconds, *is_video, discard_reason.clone()),
        _ => (0, false, String::new()),
    };
    let missed = !m.is_outgoing && reason.contains("Missed");
    let (icon, what) = if missed {
        ("call-missed-symbolic", "Missed".to_string())
    } else if reason.contains("Declined") {
        ("call-missed-symbolic", if m.is_outgoing { "Declined" } else { "You declined" }.to_string())
    } else if m.is_outgoing {
        ("call-outgoing-symbolic", "Outgoing".to_string())
    } else {
        ("call-incoming-symbolic", "Incoming".to_string())
    };
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("row-mark");
    if missed {
        image.add_css_class("missed");
    }
    bottom.append(&image);
    let mut line = format!("{}{what}", if video { "Video · " } else { "" });
    if duration > 0 {
        line.push_str(&format!(" · {}:{:02}", duration / 60, duration % 60));
    }
    let label = w::label(&line, "chat-snippet");
    if missed {
        label.add_css_class("missed");
    }
    label.set_hexpand(true);
    bottom.append(&label);
    content.append(&row_text(&top, &bottom));
    button.set_child(Some(&content));
    let id = m.chat_id;
    button.set_tooltip_text(Some(&format!("Open your chat with {title}")));
    button.connect_clicked(move |_| crate::window::open_chat(id));
    button
}

fn format_time(timestamp: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(timestamp, 0).single() else { return String::new() };
    let now = Local::now();
    let days = (now.date_naive() - dt.date_naive()).num_days();
    if days == 0 {
        dt.format("%H:%M").to_string()
    } else if (1..7).contains(&days) {
        dt.format("%a").to_string()
    } else {
        dt.format("%b %-d").to_string()
    }
}

/// A chat's round picture: its photo, the bookmark for Saved Messages, or its
/// initials on a colour picked from its name.
pub fn build_avatar_widget(chat: &ChatSummary, size: i32) -> gtk::Widget {
    if is_saved_messages(chat) {
        let frame = avatar_frame(size);
        frame.add_css_class("saved-messages-avatar");
        let icon = gtk::Image::from_icon_name("user-bookmarks-symbolic");
        icon.set_pixel_size((size / 2).max(12));
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        frame.set_child(Some(&icon));
        return frame.upcast();
    }
    avatar(&chat.title, chat.avatar_path.as_deref(), size)
}

/// A round picture of `size` px from `path`, or `name`'s initials.
pub fn avatar(name: &str, path: Option<&str>, size: i32) -> gtk::Widget {
    if let Some(path) = path.filter(|p| std::path::Path::new(p).exists())
        && let Ok(texture) = gdk::Texture::from_file(&gio::File::for_path(path))
    {
        let frame = avatar_frame(size);
        let base = w::hbox(0);
        base.set_size_request(size, size);
        frame.set_child(Some(&base));
        let pic = gtk::Picture::for_paintable(&texture);
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_can_shrink(true);
        pic.set_size_request(size, size);
        frame.add_overlay(&pic);
        return frame.upcast();
    }
    let initials = name.split_whitespace().take(2).filter_map(|s| s.chars().find(|c| c.is_alphanumeric())).collect::<String>().to_uppercase();
    let frame = avatar_frame(size);
    frame.add_css_class("tinted");
    nyx_ui::sidebar::set_section(&frame, &avatar_swatch(name));
    let lbl = w::label(if initials.is_empty() { "T" } else { &initials }, "chat-avatar-initials");
    lbl.add_css_class(match size {
        0..=30 => "small",
        31..=48 => "medium",
        _ => "large",
    });
    lbl.set_xalign(0.5);
    lbl.set_justify(gtk::Justification::Center);
    lbl.set_halign(gtk::Align::Center);
    lbl.set_valign(gtk::Align::Center);
    lbl.set_size_request(size, size);
    frame.set_child(Some(&lbl));
    frame.upcast()
}

fn avatar_frame(size: i32) -> gtk::Overlay {
    let frame = gtk::Overlay::new();
    frame.set_size_request(size, size);
    frame.set_hexpand(false);
    frame.set_vexpand(false);
    frame.set_halign(gtk::Align::Center);
    frame.set_valign(gtk::Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.add_css_class("chat-avatar-mini");
    frame
}

/// The same swatch for the same name, every time.
pub fn avatar_swatch(name: &str) -> String {
    const SWATCHES: [&str; 8] = ["blue", "violet", "green", "pink", "coral", "teal", "red", "amber"];
    let hash = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    format!("swatch-{}", SWATCHES[hash as usize % SWATCHES.len()])
}

fn bind_row_actions(button: &gtk::Button, id: ChatId) {
    let click = gtk::GestureClick::new();
    click.set_button(3);
    let weak = button.downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        if let Some(button) = weak.upgrade() {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            button.grab_focus();
            crate::actions::show_chat_menu(button.upcast_ref(), id, Some((x, y)));
        }
    });
    button.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    let weak = button.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gdk::Key::Delete
            && !modifiers.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::SHIFT_MASK,
            )
        {
            crate::window::request_delete_chat(id);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Menu || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK)) {
            if let Some(button) = weak.upgrade() {
                crate::actions::show_chat_menu(button.upcast_ref(), id, None);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    button.add_controller(keys);
}

#[cfg(test)]
pub fn check_chat_removal(chat: &ChatSummary) {
    let list = ChatList::new(&gtk::SearchEntry::new());
    let mut chat = chat.clone();
    chat.order = 1;
    list.update_chats(&[chat.clone()]);
    list.select_chat(chat.id);
    assert_eq!(list.inner.borrow().active_chat_id, Some(chat.id));
    list.remove_chat(chat.id);
    assert!(list.inner.borrow().active_chat_id.is_none());
    assert!(list.get_all_chats().is_empty());
    assert!(list.empty.first_child().is_some(), "An empty list says why it's empty");
    chat.data["positions"] = serde_json::json!([{"list":{"@type":"chatListMain"},"order":"0","is_pinned":false},{"list":{"@type":"chatListArchive"},"order":"500","is_pinned":true}]);
    list.upsert_chat(&chat);
    assert!(list.get_chat(chat.id).is_some(), "Archive must remain in the account cache");
    assert!(list.inner.borrow().buttons.is_empty(), "Archived chat is absent from Main");
    list.inner.borrow_mut().place = Place::Chats(ChatListId::Archive, ChatFilter::All);
    list.rebuild_list();
    assert_eq!(list.inner.borrow().buttons.len(), 1);
    assert!(list.rows.first_child().is_some());
    assert!(list.empty.first_child().is_none());
    chat.data["positions"] = serde_json::json!([{"list":{"@type":"chatListMain"},"order":600,"is_pinned":false}]);
    chat.data["is_marked_as_unread"] = serde_json::json!(true);
    list.upsert_chat(&chat);
    assert!(list.inner.borrow().buttons.is_empty(), "Unarchive removes the row from Archive");
    list.inner.borrow_mut().place = Place::Chats(ChatListId::Main, ChatFilter::Unread);
    list.rebuild_list();
    assert_eq!(list.inner.borrow().buttons.len(), 1, "Manual unread is visible even with zero unread messages");
    list.inner.borrow_mut().place = Place::Chats(ChatListId::Main, ChatFilter::Channels);
    list.rebuild_list();
    assert!(list.inner.borrow().buttons.is_empty(), "A private chat is not a channel");
}
