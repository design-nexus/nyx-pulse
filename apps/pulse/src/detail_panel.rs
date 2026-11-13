//! Info beside the conversation: who or what the chat is, its shared media and
//! its members, fetched from Telegram.
use base64::Engine as _;
use gtk::{gdk, gio, glib, prelude::*};
use nyx_telegram_core::{client::parse_message, models::*};
use nyx_ui::widgets as w;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub struct DetailPanel {
    pub root: gtk::Box,
    body: gtk::Box,
    close: gtk::Button,
    revision: Cell<u64>,
    chat: RefCell<Option<ChatSummary>>,
    on_secret: RefCell<Option<Box<dyn Fn(i64)>>>,
    on_media: RefCell<Option<Box<dyn Fn(&MessageSummary)>>>,
}
impl DetailPanel {
    pub fn new() -> Rc<Self> {
        let root = w::vbox(0);
        root.add_css_class("detail-panel");
        root.set_size_request(300, -1);
        root.set_visible(false);
        let close = w::icon_button("window-close-symbolic", "Close info (Esc)");
        close.set_halign(gtk::Align::End);
        close.set_margin_top(6);
        close.set_margin_end(6);
        close.set_visible(false);
        root.append(&close);
        let body = w::vbox(0);
        body.add_css_class("detail-body");
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .child(&body)
            .build();
        root.append(&scroll);
        let p = Rc::new(Self {
            root,
            body,
            close,
            revision: Cell::new(0),
            chat: Default::default(),
            on_secret: Default::default(),
            on_media: Default::default(),
        });
        let weak = Rc::downgrade(&p);
        p.close.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.root.set_visible(false);
            }
        });
        p
    }
    /// Over the conversation (a narrow window) it needs its own Close.
    pub fn set_overlaid(&self, overlaid: bool) {
        self.close.set_visible(overlaid);
        if overlaid {
            self.root.add_css_class("overlaid");
        } else {
            self.root.remove_css_class("overlaid");
        }
    }
    pub fn set_chat(self: &Rc<Self>, chat: &ChatSummary) {
        self.refresh(chat);
    }
    pub fn clear(&self) {
        self.revision.set(self.revision.get() + 1);
        self.chat.borrow_mut().take();
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        self.root.set_visible(false);
    }
    fn valid(&self, revision: u64, generation: &str) -> bool {
        self.revision.get() == revision && crate::window::service_is_current(generation)
    }
    pub fn refresh(self: &Rc<Self>, chat: &ChatSummary) {
        let unchanged = self.chat.borrow().as_ref().is_some_and(|old| {
            old.id == chat.id
                && old.title == chat.title
                && old.avatar_path == chat.avatar_path
                && old.is_muted == chat.is_muted
                && old.is_secret == chat.is_secret
                && old.data["type"] == chat.data["type"]
        });
        *self.chat.borrow_mut() = Some(chat.clone());
        if unchanged {
            return;
        }
        let revision = self.revision.get() + 1;
        self.revision.set(revision);
        while let Some(c) = self.body.first_child() {
            self.body.remove(&c);
        }
        let id = chat.id;
        let user = chat.data["type"]["user_id"].as_i64();
        let saved = crate::chat_list::is_saved_messages(chat);
        let group = matches!(chat.chat_type, ChatType::BasicGroup | ChatType::Supergroup);

        let head = w::vbox(4);
        head.add_css_class("detail-head");
        let avatar = crate::chat_list::build_avatar_widget(chat, 72);
        avatar.set_halign(gtk::Align::Center);
        head.append(&avatar);
        let title = w::label(&crate::chat_list::display_title(chat), "detail-title");
        title.set_xalign(0.5);
        title.set_justify(gtk::Justification::Center);
        title.set_wrap(true);
        title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        title.set_max_width_chars(26);
        head.append(&title);
        let kind = if chat.is_secret {
            "Secret chat"
        } else if saved {
            "Your saved messages"
        } else {
            match chat.chat_type {
                ChatType::Private => "Telegram contact",
                ChatType::Channel => "Channel",
                _ => "Group",
            }
        };
        let subtitle = w::label(kind, "detail-subtitle");
        subtitle.set_xalign(0.5);
        head.append(&subtitle);
        let quick = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        quick.set_homogeneous(true);
        quick.add_css_class("quick-actions");
        let muted = chat.is_muted;
        quick.append(&quick_action(
            if muted { "notifications-symbolic" } else { "notifications-disabled-symbolic" },
            if muted { "Unmute" } else { "Mute" },
            move |_| {
                crate::actions::run(id, "details-notifications", move |scope| async move {
                    let chat = scope.call("getChat", json!({"chat_id":id})).await?;
                    let mut settings = chat["notification_settings"].clone();
                    settings["use_default_mute_for"] = json!(false);
                    settings["mute_for"] = json!(if muted { 0 } else { i32::MAX });
                    scope.call("setChatNotificationSettings", json!({"chat_id":id,"notification_settings":settings})).await?;
                    Ok(())
                });
            },
        ));
        quick.append(&quick_action("edit-find-symbolic", "Search", move |_| crate::window::search_conversation(id)));
        let private_call = chat.chat_type == ChatType::Private && !chat.is_secret && !saved;
        if private_call {
            quick.append(&quick_action("call-start-symbolic", "Call", move |_| {
                if !crate::window::start_call_from_assistant(id, false) {
                    crate::window::show_status("Calling is only available for a Telegram contact");
                }
            }));
        } else if chat.data["video_chat"]["group_call_id"].as_i64().unwrap_or(0) != 0 {
            quick.append(&quick_action("call-start-symbolic", "Voice chat", move |_| crate::group_calls::open(id)));
        }
        quick.append(&quick_action("view-more-horizontal-symbolic", "More", move |b| {
            crate::actions::show_chat_menu(b.upcast_ref(), id, None)
        }));
        head.append(&quick);
        self.body.append(&head);
        self.body.append(&nyx_ui::frame::pane_line(gtk::Orientation::Horizontal));

        let info = w::vbox(14);
        info.add_css_class("detail-info");
        self.body.append(&info);
        let about = w::vbox(0);
        about.add_css_class("group-list");
        about.set_visible(false);
        info.append(&about);

        let media_head = section_head("Shared media", "See all", move || crate::message_tools::photos(id));
        let media_grid = gtk::Grid::new();
        media_grid.add_css_class("media-grid");
        media_grid.set_column_spacing(4);
        media_grid.set_row_spacing(4);
        media_grid.set_column_homogeneous(true);
        info.append(&media_head.0);
        info.append(&media_grid);

        let members = if group || chat.chat_type == ChatType::Channel {
            let head = section_head("Members", "Manage", move || crate::groups::members(id));
            let list = w::vbox(0);
            list.add_css_class("group-list");
            list.set_visible(false);
            info.append(&head.0);
            info.append(&list);
            Some((head, list))
        } else {
            None
        };

        let more = w::vbox(0);
        more.add_css_class("group-list");
        if chat.data["pulse_is_forum"] == true {
            more.append(&link_row("Topics", move || crate::topics::browse(id)));
        }
        more.append(&link_row("Pinned messages", move || crate::message_tools::pinned(id)));
        if let Some(user) = user.filter(|_| !chat.is_secret && !saved) {
            let weak = Rc::downgrade(self);
            more.append(&link_row("Start a secret chat", move || {
                if let Some(p) = weak.upgrade()
                    && let Some(cb) = p.on_secret.borrow().as_ref()
                {
                    cb(user);
                }
            }));
        }
        if matches!(chat.chat_type, ChatType::BasicGroup | ChatType::Supergroup | ChatType::Channel) {
            more.append(&link_row(
                if chat.chat_type == ChatType::Channel { "Manage channel" } else { "Manage group" },
                move || crate::groups::manage(id),
            ));
        }
        info.append(&more);

        let Some(service) = crate::window::service() else { return };
        let generation = service.generation.clone();
        let (method, args) = if let Some(user) = user {
            ("getUserFullInfo", json!({"user_id":user}))
        } else if let Some(group) = chat.data["type"]["supergroup_id"].as_i64() {
            ("getSupergroupFullInfo", json!({"supergroup_id":group}))
        } else if let Some(group) = chat.data["type"]["basic_group_id"].as_i64() {
            ("getBasicGroupFullInfo", json!({"basic_group_id":group}))
        } else {
            ("", Value::Null)
        };
        if !method.is_empty() {
            let rx = service.request(method, args);
            let weak = Rc::downgrade(self);
            let (generation, subtitle, about) = (generation.clone(), subtitle.clone(), about.clone());
            let members = members.clone();
            let chat_kind = chat.chat_type;
            let basic = chat.data["type"]["basic_group_id"].as_i64().is_some();
            glib::spawn_future_local(async move {
                let Ok(Ok(data)) = rx.recv().await else { return };
                let Some(panel) = weak.upgrade() else { return };
                if !panel.valid(revision, &generation) {
                    return;
                }
                let bio = data["bio"]["text"].as_str().or(data["description"].as_str()).unwrap_or("");
                if !bio.is_empty() {
                    about.append(&about_row(if user.is_some() { "Bio" } else { "Description" }, bio));
                    about.set_visible(true);
                }
                if let Some(link) = data["invite_link"]["invite_link"].as_str() {
                    about.append(&invite_row(link));
                    about.set_visible(true);
                }
                let count = data["member_count"].as_i64().or_else(|| data["members"].as_array().map(|m| m.len() as i64));
                if let Some(count) = count.filter(|c| *c > 0) {
                    subtitle.set_text(&match chat_kind {
                        ChatType::Channel => format!("Channel · {count} subscribers"),
                        _ => format!("Group · {count} members"),
                    });
                    if let Some(((_, count_label), _)) = &members {
                        count_label.set_text(&count.to_string());
                    }
                }
                if basic && let Some((_, list)) = &members {
                    let first: Vec<Value> = data["members"].as_array().into_iter().flatten().take(3).cloned().collect();
                    panel.show_members(revision, &generation, list, first);
                }
            });
        }
        if let Some(user) = user.filter(|_| !saved) {
            let rx = service.request("getUser", json!({"user_id":user}));
            let weak = Rc::downgrade(self);
            let (generation, subtitle, about) = (generation.clone(), subtitle.clone(), about.clone());
            glib::spawn_future_local(async move {
                let Ok(Ok(data)) = rx.recv().await else { return };
                let Some(panel) = weak.upgrade() else { return };
                if !panel.valid(revision, &generation) {
                    return;
                }
                subtitle.set_text(&user_status(&data["status"]));
                if let Some(name) = data["usernames"]["active_usernames"].as_array().and_then(|u| u.first()).and_then(Value::as_str) {
                    about.prepend(&about_pair("Username", &format!("@{name}")));
                    about.set_visible(true);
                }
            });
        }
        if let (Some(group), Some((_, list))) = (chat.data["type"]["supergroup_id"].as_i64(), &members) {
            let rx = service.request(
                "getSupergroupMembers",
                json!({"supergroup_id":group,"filter":null,"offset":0,"limit":3}),
            );
            let weak = Rc::downgrade(self);
            let (generation, list) = (generation.clone(), list.clone());
            glib::spawn_future_local(async move {
                let Ok(Ok(data)) = rx.recv().await else { return };
                let Some(panel) = weak.upgrade() else { return };
                if panel.valid(revision, &generation) {
                    let first: Vec<Value> = data["members"].as_array().into_iter().flatten().take(3).cloned().collect();
                    panel.show_members(revision, &generation, &list, first);
                }
            });
        }
        self.load_media(id, revision, &generation, &media_grid, &media_head.1);
    }

    /// The first members, each with their role or when they were last seen.
    fn show_members(self: &Rc<Self>, revision: u64, generation: &str, list: &gtk::Box, members: Vec<Value>) {
        let Some(service) = crate::window::service() else { return };
        for member in members {
            let Some(user) = member["member_id"]["user_id"].as_i64() else { continue };
            let rx = service.request("getUser", json!({"user_id":user}));
            let weak = Rc::downgrade(self);
            let (generation, list) = (generation.to_string(), list.clone());
            glib::spawn_future_local(async move {
                let Ok(Ok(data)) = rx.recv().await else { return };
                let Some(panel) = weak.upgrade() else { return };
                if !panel.valid(revision, &generation) {
                    return;
                }
                let name = format!("{} {}", data["first_name"].as_str().unwrap_or(""), data["last_name"].as_str().unwrap_or(""));
                let name = name.trim().to_string();
                let role = match member["status"]["@type"].as_str() {
                    Some("chatMemberStatusCreator") => "owner".to_string(),
                    Some("chatMemberStatusAdministrator") => "admin".to_string(),
                    _ => user_status(&data["status"]),
                };
                let row = w::hbox(10);
                row.add_css_class("detail-row");
                let photo = data["profile_photo"]["small"]["local"]["path"].as_str().filter(|p| !p.is_empty());
                row.append(&crate::chat_list::avatar(&name, photo, 26));
                let label = w::label(&name, "detail-key");
                label.set_hexpand(true);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                row.append(&label);
                row.append(&w::label(&role, "detail-value"));
                list.append(&row);
                list.set_visible(true);
            });
        }
    }

    /// Six recent photos and videos, and how many there are.
    fn load_media(self: &Rc<Self>, id: i64, revision: u64, generation: &str, grid: &gtk::Grid, count: &gtk::Label) {
        let Some(s) = crate::window::service() else { return };
        let rx = s.request(
            "searchChatMessages",
            json!({"chat_id":id,"query":"","from_message_id":0,"offset":0,"limit":6,"filter":{"@type":"searchMessagesFilterPhotoAndVideo"}}),
        );
        let weak = Rc::downgrade(self);
        let (generation, grid, count) = (generation.to_string(), grid.clone(), count.clone());
        glib::spawn_future_local(async move {
            let result = rx.recv().await;
            let Some(p) = weak.upgrade() else { return };
            if !p.valid(revision, &generation) {
                return;
            }
            let Ok(Ok(data)) = result else { return };
            if let Some(total) = data["total_count"].as_i64() {
                count.set_text(&total.to_string());
            }
            let messages: Vec<MessageSummary> = data["messages"].as_array().into_iter().flatten().filter_map(parse_message).collect();
            if messages.is_empty() {
                grid.attach(&w::label("No photos or videos yet", "dim"), 0, 0, 3, 1);
            }
            for (i, m) in messages.into_iter().enumerate() {
                let tile = gtk::Button::new();
                tile.add_css_class("media-tile");
                tile.set_overflow(gtk::Overflow::Hidden);
                tile.set_size_request(-1, 68);
                if let Some(texture) = thumbnail(&m) {
                    let picture = gtk::Picture::for_paintable(&texture);
                    picture.set_content_fit(gtk::ContentFit::Cover);
                    picture.set_can_shrink(true);
                    tile.set_child(Some(&picture));
                } else {
                    tile.set_child(Some(&gtk::Image::from_icon_name("image-x-generic-symbolic")));
                }
                tile.set_tooltip_text(Some(&crate::chat_view::preview(&m)));
                let weak = Rc::downgrade(&p);
                let generation = generation.clone();
                tile.connect_clicked(move |_| {
                    if let Some(p) = weak.upgrade()
                        && p.valid(revision, &generation)
                        && let Some(cb) = p.on_media.borrow().as_ref()
                    {
                        cb(&m);
                    }
                });
                grid.attach(&tile, (i % 3) as i32, (i / 3) as i32, 1, 1);
            }
        });
    }
    pub fn toggle_visible(self: &Rc<Self>) {
        self.root.set_visible(!self.root.is_visible());
        if self.root.is_visible() {
            let chat = self.chat.borrow().clone();
            if let Some(c) = chat {
                self.refresh(&c);
            }
        }
    }
    pub fn connect_start_secret_chat<F: Fn(i64) + 'static>(&self, f: F) {
        *self.on_secret.borrow_mut() = Some(Box::new(f));
    }
    pub fn connect_media<F: Fn(&MessageSummary) + 'static>(&self, f: F) {
        *self.on_media.borrow_mut() = Some(Box::new(f));
    }
}

fn quick_action(icon: &str, label: &str, run: impl Fn(&gtk::Button) + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("quick-action");
    let face = w::vbox(4);
    face.append(&gtk::Image::from_icon_name(icon));
    let l = w::label(label, "");
    l.set_xalign(0.5);
    face.append(&l);
    b.set_child(Some(&face));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b.connect_clicked(run);
    b
}

/// "Shared media 128  See all": the heading, a count filled in later, and a link.
fn section_head(title: &str, action: &str, run: impl Fn() + 'static) -> (gtk::Box, gtk::Label) {
    let row = w::hbox(6);
    row.add_css_class("detail-section");
    row.append(&w::label(title, "detail-section-title"));
    let count = w::label("", "detail-count");
    count.set_hexpand(true);
    row.append(&count);
    let link = gtk::Button::with_label(action);
    link.add_css_class("detail-link");
    link.connect_clicked(move |_| run());
    row.append(&link);
    (row, count)
}

fn about_row(key: &str, text: &str) -> gtk::Box {
    let row = w::vbox(3);
    row.add_css_class("detail-row");
    row.append(&w::label(key, "detail-key"));
    let body = w::label(text, "detail-about");
    body.set_wrap(true);
    body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.set_max_width_chars(30);
    body.set_selectable(true);
    row.append(&body);
    row
}

fn about_pair(key: &str, value: &str) -> gtk::Box {
    let row = w::hbox(10);
    row.add_css_class("detail-row");
    let k = w::label(key, "detail-key");
    k.set_hexpand(true);
    row.append(&k);
    let v = w::label(value, "detail-value");
    v.set_selectable(true);
    row.append(&v);
    row
}

/// The invite link; clicking copies it.
fn invite_row(link: &str) -> gtk::Box {
    let row = w::hbox(10);
    row.add_css_class("detail-row");
    let k = w::label("Invite link", "detail-key");
    k.set_hexpand(true);
    row.append(&k);
    let shown = link.trim_start_matches("https://");
    let b = gtk::Button::with_label(shown);
    b.add_css_class("detail-link");
    b.set_tooltip_text(Some("Copy the invite link"));
    if let Some(label) = b.child().and_downcast::<gtk::Label>() {
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_max_width_chars(20);
    }
    let link = link.to_string();
    b.connect_clicked(move |b| {
        b.clipboard().set_text(&link);
        crate::window::show_status("Invite link copied");
    });
    row.append(&b);
    row
}

fn link_row(title: &str, run: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("detail-action");
    let face = w::hbox(8);
    let l = w::label(title, "");
    l.set_hexpand(true);
    face.append(&l);
    face.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    b.set_child(Some(&face));
    b.connect_clicked(move |_| run());
    b
}

fn user_status(status: &Value) -> String {
    match status["@type"].as_str() {
        Some("userStatusOnline") => "online".into(),
        Some("userStatusRecently") => "last seen recently".into(),
        Some("userStatusLastWeek") => "last seen this week".into(),
        Some("userStatusLastMonth") => "last seen this month".into(),
        Some("userStatusOffline") => {
            let then = status["was_online"].as_i64().unwrap_or(0);
            let minutes = (chrono::Utc::now().timestamp() - then).max(0) / 60;
            match minutes {
                0 => "last seen just now".into(),
                1..=59 => format!("last seen {minutes} min ago"),
                60..=1439 => format!("last seen {} h ago", minutes / 60),
                _ => chrono::DateTime::from_timestamp(then, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("last seen %b %-d").to_string())
                    .unwrap_or_else(|| "last seen a while ago".into()),
            }
        }
        _ => "Telegram contact".into(),
    }
}

/// A picture for a photo or video: the downloaded file, or Telegram's tiny preview.
fn thumbnail(m: &MessageSummary) -> Option<gdk::Texture> {
    let path = m
        .files
        .iter()
        .filter(|f| f.role == "thumbnail" || f.role == "original")
        .find_map(|f| f.path.clone().filter(|p| std::path::Path::new(p).exists()));
    if let Some(path) = path
        && let Ok(texture) = gdk::Texture::from_file(&gio::File::for_path(&path))
    {
        return Some(texture);
    }
    let content = &m.data["content"];
    let mini = content["photo"]["minithumbnail"]["data"].as_str().or(content["video"]["minithumbnail"]["data"].as_str())?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(mini).ok()?;
    gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()
}

#[cfg(test)]
pub fn check_layout_and_refresh(chat: &ChatSummary) {
    let panel = DetailPanel::new();
    panel.set_chat(chat);
    let head = panel.body.first_child().unwrap();
    assert!(head.has_css_class("detail-head"));
    assert!(!panel.close.get_visible(), "Docked beside the chat, the Info button closes it");
    panel.set_overlaid(true);
    assert!(panel.close.get_visible(), "Over the chat, it has its own Close");
    let marker = w::label("Loaded media", "dim");
    panel.body.append(&marker);
    let mut update = chat.clone();
    update.unread_count += 1;
    panel.refresh(&update);
    assert!(marker.parent().is_some(), "Unread updates must preserve loaded details");
    update.id += 1;
    panel.set_chat(&update);
    assert!(marker.parent().is_none(), "Changing chats must clear prior details");
    panel.clear();
    assert!(panel.body.first_child().is_none());
}
