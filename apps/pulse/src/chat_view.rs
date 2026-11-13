//! Conversation UI backed by stable Telegram message identities.
use gtk::{gdk, gio, glib, prelude::*};
use nyx_telegram_core::{client::parse_message, models::*};
use nyx_ui::widgets as w;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

pub struct ChatView {
    pub(crate) detached: Cell<bool>,
    self_weak: RefCell<std::rc::Weak<Self>>,
    pub root: gtk::Box,
    title_label: gtk::Label,
    status_label: gtk::Label,
    default_status: RefCell<String>,
    avatar_box: gtk::Box,
    pub message_list: gtk::Box,
    scroll_window: gtk::ScrolledWindow,
    text_view: gtk::TextView,
    selection_bar: gtk::Box,
    selection_label: gtk::Label,
    selected: RefCell<std::collections::BTreeSet<i64>>,
    quote: RefCell<Option<Value>>,
    reply_id: Cell<Option<i64>>,
    restoring: Cell<bool>,
    format_pending: Cell<bool>,
    reply_banner: gtk::Box,
    reply_label: gtk::Label,
    current_reply: RefCell<Option<MessageSummary>>,
    messages: RefCell<BTreeMap<MessageId, MessageSummary>>,
    rows: RefCell<BTreeMap<MessageId, gtk::Box>>,
    retired_ids: RefCell<std::collections::HashSet<MessageId>>,
    pub current_chat_id: RefCell<Option<ChatId>>,
    current_chat: RefCell<Option<ChatSummary>>,
    topic: RefCell<Value>,
    history_revision: Cell<u64>,
    history_jump: Cell<Option<i64>>,
    context_bar: gtk::Box,
    context_label: gtk::Label,
    pending_scroll: Cell<bool>,
    sending: Cell<bool>,
    send_generation: Cell<u64>,
    composer: gtk::Box,
    send_button: gtk::Button,
    placeholder: gtk::Label,
    call_button: gtk::Button,
    call_label: gtk::Label,
    video_button: gtk::Button,
    info_button: gtk::Button,
    pinned_button: gtk::Button,
    pinned_text: gtk::Label,
    pinned_id: Cell<Option<i64>>,
    pages: gtk::Stack,
    members: Cell<(i64, i64)>,
    details_revision: Cell<u64>,
    callable: Cell<bool>,
    info_open: Cell<bool>,
    day_marks_pending: Cell<bool>,
    loading: Cell<bool>,
    has_more: Cell<bool>,
    last_typing: Cell<Option<std::time::Instant>>,
    jump: gtk::Button,
    older: gtk::Button,
    on_typing: RefCell<Option<Box<dyn Fn(ChatId)>>>,
    on_start_call: RefCell<Option<Box<dyn Fn(ChatId, bool)>>>,
    on_details: RefCell<Option<Box<dyn Fn()>>>,
    on_media: RefCell<Option<Box<dyn Fn(&MessageSummary)>>>,
}
impl ChatView {
    pub fn new() -> Rc<Self> {
        let root = w::vbox(0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("chat-view-column");
        let header = w::hbox(10);
        header.add_css_class("chat-header");
        let avatar_box = w::hbox(0);
        header.append(&avatar_box);
        let titles = w::vbox(0);
        titles.set_hexpand(true);
        let title_label = w::label("Select a conversation", "chat-header-title");
        title_label.set_xalign(0.0);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.set_max_width_chars(1);
        title_label.connect_notify_local(Some("label"),|label,_|label.set_tooltip_text(Some(&label.text())));
        let status_label = w::label("", "chat-header-status");
        status_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status_label.set_xalign(0.0);
        status_label.set_max_width_chars(1);
        status_label.connect_notify_local(Some("label"),|label,_|label.set_tooltip_text(Some(&label.text())));
        titles.append(&title_label);
        titles.append(&status_label);
        header.append(&titles);
        let pinned = gtk::Button::new();
        pinned.add_css_class("pinned-chip");
        let chip = w::hbox(7);
        let pin_icon = gtk::Image::from_icon_name("view-pin-symbolic");
        chip.append(&pin_icon);
        chip.append(&w::label("Pinned", "pinned-label"));
        let pinned_text = w::label("", "pinned-text");
        pinned_text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        pinned_text.set_max_width_chars(26);
        chip.append(&pinned_text);
        pinned.set_child(Some(&chip));
        pinned.set_visible(false);
        pinned.set_valign(gtk::Align::Center);
        let search = w::icon_button("edit-find-symbolic", "Search in this chat");
        let call = gtk::Button::new();
        call.add_css_class("call-button");
        let call_face = w::hbox(6);
        call_face.append(&gtk::Image::from_icon_name("call-start-symbolic"));
        let call_label = gtk::Label::new(Some("Call"));
        call_face.append(&call_label);
        call.set_child(Some(&call_face));
        call.set_valign(gtk::Align::Center);
        call.set_visible(false);
        let video = w::icon_button("camera-video-symbolic", "Video call");
        video.set_visible(false);
        let details = w::icon_button("dialog-information-symbolic", "Info: members, shared media and settings");
        details.add_css_class("info-toggle");
        let menu = w::icon_button("view-more-horizontal-symbolic", "More");
        for b in [&pinned, &search, &call, &video, &details, &menu] {
            header.append(b);
        }
        root.append(&header);
        let context_bar = w::hbox(8);
        context_bar.add_css_class("message-selection-bar");
        let back = gtk::Button::with_label("← All messages");
        let context_label = w::label("", "dim");
        context_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        context_label.set_hexpand(true);context_label.set_max_width_chars(1);
        context_bar.append(&back); context_bar.append(&context_label);
        context_bar.set_visible(false); root.append(&context_bar);
        let selection_bar = w::hbox(6);
        selection_bar.add_css_class("message-selection-bar");
        selection_bar.set_visible(false);
        let selection_label = w::label("Select messages", "dim");
        selection_label.set_hexpand(true);
        let selection_forward = w::bar_button("mail-forward-symbolic","Forward selected messages");
        let selection_copy = w::bar_button("edit-copy-symbolic","Copy selected messages");
        let selection_delete = w::bar_button("user-trash-symbolic","Delete selected messages");
        let selection_report = w::bar_button("dialog-warning-symbolic","Report selected messages");
        let selection_cancel = w::bar_button("window-close-symbolic", "Clear message selection");
        selection_bar.append(&selection_label);
        for button in [&selection_forward, &selection_copy, &selection_delete, &selection_report, &selection_cancel] {
            selection_bar.append(button);
        }
        root.append(&selection_bar);
        let older = gtk::Button::with_label("Load older messages");
        older.add_css_class("flat");
        older.set_visible(false);
        root.append(&older);
        let message_list = w::vbox(6);
        message_list.add_css_class("message-history-list");
        let scroll_window = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .child(&message_list)
            .build();
        root.append(&scroll_window);
        let jump = gtk::Button::with_label("↓ Latest messages");
        jump.add_css_class("flat");
        jump.set_visible(false);
        root.append(&jump);
        let reply_banner = w::hbox(6);
        reply_banner.add_css_class("reply-banner");
        reply_banner.set_visible(false);
        let reply_label = w::label("", "reply-snippet");
        reply_label.set_hexpand(true);reply_label.set_max_width_chars(1);
        reply_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        reply_banner.append(&reply_label);
        let cancel = w::bar_button("window-close-symbolic", "Cancel reply");
        reply_banner.append(&cancel);
        root.append(&reply_banner);
        let composer = w::hbox(8);
        composer.add_css_class("message-composer");
        composer.set_sensitive(false);
        let field = w::hbox(4);
        field.add_css_class("composer-field");
        field.set_hexpand(true);
        let attach = w::icon_button("mail-attachment-symbolic", "Attach a file or photo, or send a poll, contact or location");
        attach.set_valign(gtk::Align::End);
        field.append(&attach);
        let text_view = gtk::TextView::new();
        text_view.add_css_class("composer-input");
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.set_accepts_tab(false);
        text_view.set_hexpand(true);
        let input = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .min_content_height(22)
            .max_content_height(160)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .child(&text_view)
            .build();
        let typing = gtk::Overlay::new();
        typing.set_hexpand(true);
        typing.set_child(Some(&input));
        let placeholder = w::label("Message", "composer-placeholder");
        placeholder.set_can_target(false);
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_ellipsize(gtk::pango::EllipsizeMode::End);
        typing.add_overlay(&placeholder);
        field.append(&typing);
        let emoji = w::icon_button("face-smile-symbolic", "Emoji, stickers and GIFs");
        emoji.set_valign(gtk::Align::End);
        field.append(&emoji);
        composer.append(&field);
        let send = gtk::Button::from_icon_name("audio-input-microphone-symbolic");
        send.add_css_class("composer-action");
        send.set_valign(gtk::Align::End);
        send.set_tooltip_text(Some("Record a voice message"));
        send.update_property(&[gtk::accessible::Property::Label("Record a voice message")]);
        composer.append(&send);
        root.append(&composer);
        let column = root;
        let pages = gtk::Stack::new();
        pages.add_named(&column, Some("chat"));
        let empty = w::empty_state("pulse-chats-symbolic", "No chat open", "Choose a chat from the list, or start one with New.", None);
        pages.add_named(&empty, Some("empty"));
        pages.set_visible_child_name("empty");
        let root = w::vbox(0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("chat-view-column");
        root.append(&pages);
        let v = Rc::new(Self {
            detached: Cell::new(false),
            self_weak: Default::default(),
            root,
            title_label,
            status_label,
            default_status: Default::default(),
            avatar_box,
            message_list,
            scroll_window,
            text_view,
            current_reply: RefCell::new(None),
            selection_bar,
            selection_label,
            selected: Default::default(),
            quote: Default::default(),
            reply_id: Cell::new(None),
            restoring: Cell::new(false),
            format_pending: Cell::new(false),
            reply_banner,
            reply_label,
            messages: Default::default(),
            rows: Default::default(),
            retired_ids: Default::default(),
            current_chat_id: Default::default(),
            current_chat: Default::default(),
            topic: RefCell::new(Value::Null), history_revision: Cell::new(0), history_jump: Cell::new(None), context_bar, context_label,
            pending_scroll: Cell::new(false),
            sending: Cell::new(false),
            send_generation: Cell::new(0),
            composer,
            send_button: send.clone(),
            placeholder,
            call_button: call.clone(),
            call_label,
            video_button: video.clone(),
            info_button: details.clone(),
            pinned_button: pinned.clone(),
            pinned_text,
            pinned_id: Cell::new(None),
            pages,
            members: Cell::new((0, 0)),
            details_revision: Cell::new(0),
            callable: Cell::new(false),
            info_open: Cell::new(false),
            day_marks_pending: Cell::new(false),
            loading: Cell::new(false),
            has_more: Cell::new(true),
            last_typing: Cell::new(None),
            jump,
            older,
            on_typing: Default::default(),
            on_start_call: Default::default(),
            on_details: Default::default(),
            on_media: Default::default(),
        });
        *v.self_weak.borrow_mut()=Rc::downgrade(&v);
        let weak = Rc::downgrade(&v);
        let width = Cell::new(0);
        v.root.add_tick_callback(move |root, _| {
            let current = root.width();
            if current > 0
                && current != width.replace(current)
                && let Some(v) = weak.upgrade()
            {
                v.fit_header();
            }
            glib::ControlFlow::Continue
        });

        macro_rules! clicked {
            ($b:expr,$f:expr) => {{
                let weak = Rc::downgrade(&v);
                $b.connect_clicked(move |_| {
                    if let Some(v) = weak.upgrade() {
                        ($f)(v);
                    }
                });
            }};
        }
        clicked!(send, |v: Rc<Self>| {
            if v.has_text() {
                v.send_current_text();
            } else {
                crate::recording::open(&v.root, *v.current_chat_id.borrow());
            }
        });
        clicked!(pinned, |v: Rc<Self>| {
            if let Some(id) = v.pinned_id.get() {
                v.jump_to(id);
            }
        });
        clicked!(cancel, |v: Rc<Self>| v.clear_reply());
        clicked!(v.jump, |v: Rc<Self>| v.scroll_to_bottom());
        let weak = Rc::downgrade(&v);
        back.connect_clicked(move |_| { if let Some(v)=weak.upgrade() { let chat=*v.current_chat_id.borrow(); if let Some(chat)=chat{crate::window::open_chat(chat);} } });
        clicked!(v.older, |v: Rc<Self>| v.load_older());
        let weak = Rc::downgrade(&v);
        attach.connect_clicked(move |button| {
            let Some(v) = weak.upgrade() else { return };
            let Some(chat) = *v.current_chat_id.borrow() else { return };
            let (p, body) = crate::actions::popover(button.upcast_ref(), None);
            let weak = Rc::downgrade(&v);
            crate::actions::menu_item(&p, &body, "Photo or file…", move || { if let Some(v) = weak.upgrade() { v.choose_attachment(); } });
            let video_view=Rc::downgrade(&v);
            crate::actions::menu_item(&p,&body,"Video message…",move ||{if let Some(v)=video_view.upgrade(){crate::recording::open_video(&v.root,Some(chat));}});
            crate::actions::menu_item(&p,&body,"Checklist…",move||crate::message_extras::compose_checklist(chat));
            crate::actions::menu_item(&p,&body,"Inline bot search…",move||crate::bots::inline(chat));
            crate::actions::menu_item(&p, &body, "Poll or quiz…", move || crate::rich_messages::compose_poll(chat));
            crate::actions::menu_item(&p, &body, "Contact…", move || crate::rich_messages::compose_contact(chat));
            crate::actions::menu_item(&p, &body, "Location…", move || crate::rich_messages::compose_location(chat));
            let (weak,anchor)=(Rc::downgrade(&v),button.downgrade());
            crate::actions::menu_item(&p,&body,"Format message…",move||{if let(Some(v),Some(anchor))=(weak.upgrade(),anchor.upgrade()){crate::compose::menu(anchor.upcast_ref(),&v.text_view);}});
            let weak=Rc::downgrade(&v);crate::actions::menu_item(&p,&body,"Mention someone…",move||{if let Some(v)=weak.upgrade(){crate::message_tools::mention(&v);}});
            let sender = v.current_chat.borrow().as_ref().filter(|c| matches!(c.chat_type, ChatType::Supergroup | ChatType::Channel)).map(|c| c.data["pulse_sender_name"].as_str().filter(|s| !s.is_empty()).unwrap_or("Telegram default").to_string());
            if let Some(sender) = sender {
                crate::actions::menu_item(&p, &body, &format!("Send as: {sender}…"), move || crate::identities::choose(chat));
            }
            if v.has_text() {
                let (weak, anchor) = (Rc::downgrade(&v), v.send_button.downgrade());
                crate::actions::menu_item(&p, &body, "Send silently or schedule…", move || {
                    if let (Some(v), Some(anchor)) = (weak.upgrade(), anchor.upgrade()) {
                        v.send_options_menu(anchor.upcast_ref());
                    }
                });
            }
            p.popup();
        });
        clicked!(details, |v: Rc<Self>| {
            if let Some(cb) = v.on_details.borrow().as_ref() {
                cb();
            }
        });
        clicked!(call, |v: Rc<Self>| v.start_call(false));
        clicked!(video, |v: Rc<Self>| v.start_call(true));
        let weak = Rc::downgrade(&v);
        menu.connect_clicked(move |button| {
            if let Some(v) = weak.upgrade() {
                if let Some(chat) = *v.current_chat_id.borrow() {
                    crate::actions::show_chat_menu(button.upcast_ref(), chat, None);
                }
            }
        });
        clicked!(selection_forward, |v: Rc<Self>| crate::actions::forward(v.selected_messages(), false));
        clicked!(selection_copy, |v: Rc<Self>| crate::actions::copy_messages(v.selected_messages()));
        clicked!(selection_delete, |v: Rc<Self>| crate::actions::delete_messages(v.selected_messages(), Some(v.clone())));
        clicked!(selection_report, |v: Rc<Self>| crate::actions::report_messages(v.selected_messages()));
        clicked!(selection_cancel, |v: Rc<Self>| v.clear_selection());
        let selection_keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&v);
        selection_keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = weak.upgrade() else { return glib::Propagation::Proceed };
            if v.selection_bar.is_visible() {
                if key == gdk::Key::Escape {
                    v.clear_selection();
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Delete {
                    crate::actions::delete_messages(v.selected_messages(), Some(v.clone()));
                    return glib::Propagation::Stop;
                }
                if (key == gdk::Key::c || key == gdk::Key::C) && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                    crate::actions::copy_messages(v.selected_messages());
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        v.root.add_controller(selection_keys);
        clicked!(search, |v: Rc<Self>| v.search_messages());
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gdk::BUTTON_SECONDARY);
        let weak = Rc::downgrade(&v);
        let parent = send.downgrade();
        gesture.connect_pressed(move |gesture, _, _, _| {
            if let (Some(v), Some(button)) = (weak.upgrade(), parent.upgrade())
                && v.has_text()
            {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                v.send_options_menu(button.upcast_ref());
            }
        });
        send.add_controller(gesture);
        let weak = Rc::downgrade(&v);
        v.text_view.buffer().connect_apply_tag(move |_, _, _, _| {
            if let Some(v) = weak.upgrade() {
                v.formatting_changed();
            }
        });
        let weak = Rc::downgrade(&v);
        v.text_view.buffer().connect_remove_tag(move |_, _, _, _| {
            if let Some(v) = weak.upgrade() {
                v.formatting_changed();
            }
        });
        let picker = crate::picker::PickerPopover::new(&emoji);
        let tv = v.text_view.clone();
        picker.connect_emoji(move |e| tv.buffer().insert_at_cursor(e));
        let weak = Rc::downgrade(&v);
        picker.connect_sticker(move |id,_|{if let Some(v)=weak.upgrade(){if let Ok(id)=id.parse::<i32>(){v.send_content(json!({"@type":"inputMessageSticker","sticker":{"@type":"inputFileId","id":id},"width":0,"height":0,"emoji":""}));}}});
        let weak = Rc::downgrade(&v);
        picker.connect_gif(move |_,token|{if let Some(v)=weak.upgrade(){if let Ok(value)=serde_json::from_str::<Value>(token){if let (Some(service),Some(chat))=(crate::window::service(),*v.current_chat_id.borrow()){let rx=service.request("sendInlineQueryResultMessage",json!({"chat_id":chat,"topic_id":v.topic_id(),"reply_to":v.reply_target(),"query_id":value["query_id"],"result_id":value["result_id"],"hide_via_bot":true}));let weak=Rc::downgrade(&v);let account=service.account;glib::spawn_future_local(async move{if let Ok(result)=rx.recv().await{if crate::prefs::get().active_account_id!=account{return}if let Some(v)=weak.upgrade(){match result{Ok(data)=>if let Some(message)=parse_message(&data){v.append_message(&message);},Err(e)=>v.set_error(&e)}}}});}}else if let Ok(id)=token.parse::<i32>(){v.send_content(json!({"@type":"inputMessageAnimation","animation":{"@type":"inputFileId","id":id},"caption":{"@type":"formattedText","text":"","entities":[]}}));}}});
        emoji.connect_clicked(move |_| picker.popup());
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&v);
        keys.connect_key_pressed(move |_, key, _, mods| {
            if let Some(v) = weak.upgrade() {
                if crate::compose::key(&v.text_view, key, mods) {
                    v.save_draft();
                    return glib::Propagation::Stop;
                }
            }
            if key == gdk::Key::v && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(v) = weak.upgrade() {
                    if v.paste_attachment() {
                        return glib::Propagation::Stop;
                    }
                }
            }

            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) && !mods.contains(gdk::ModifierType::SHIFT_MASK) {
                if let Some(v) = weak.upgrade() {
                    v.send_current_text();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        v.text_view.add_controller(keys);
        let weak = Rc::downgrade(&v);
        v.text_view.buffer().connect_changed(move |_| {
            if let Some(v) = weak.upgrade() {
                v.update_send_button();
                v.save_draft();
                if let Some(id) = *v.current_chat_id.borrow() {
                    let now = std::time::Instant::now();
                    if v.last_typing.get().map(|t| now.duration_since(t).as_secs() > 4).unwrap_or(true) {
                        v.last_typing.set(Some(now));
                        if let Some(cb) = v.on_typing.borrow().as_ref() {
                            cb(id);
                        }
                    }
                }
            }
        });
        let weak = Rc::downgrade(&v);
        v.scroll_window.vadjustment().connect_value_changed(move |a| {
            if let Some(v) = weak.upgrade() {
                v.jump.set_visible(a.value() + a.page_size() < a.upper() - 80.0);
            }
        });
        let weak = Rc::downgrade(&v);
        v.scroll_window.connect_edge_reached(move |_, edge| {
            if edge == gtk::PositionType::Top
                && let Some(v) = weak.upgrade()
            {
                v.load_older();
            }
        });
        v.reset_account();
        v
    }
    fn draft_key(&self) -> String {
        crate::topics::draft_key(&crate::prefs::get().active_account_id, self.current_chat_id.borrow().unwrap_or(0), &self.topic.borrow())
    }
    pub fn topic_id(&self) -> Value { self.topic.borrow().clone() }
    pub fn load_history(&self, from:i64) {
        if let Some(chat)=*self.current_chat_id.borrow() {
            crate::window::with_service(|s| {
                self.loading.set(true);
                if self.detached.get(){
                    let topic=self.topic_id();let revision=self.history_revision.get();let weak=self.self_weak.borrow().clone();let scope=crate::actions::Scope::current();
                    let rx=s.history_request(chat,topic.clone(),from);
                    glib::spawn_future_local(async move{if let Ok(result)=rx.recv().await{if scope.is_some_and(|s|s.valid()){if let Some(v)=weak.upgrade(){v.topic_history(chat,&topic,revision,from,result);}}}});
                }else{s.load_topic_history(chat,self.topic_id(),self.history_revision.get(),from);}
            });
        }
    }
    pub fn topic_history(self:&Rc<Self>,chat:i64,topic:&Value,revision:u64,from:i64,result:Result<Vec<MessageSummary>,String>) {
        if *self.current_chat_id.borrow()!=Some(chat)||self.topic_id()!=*topic||self.history_revision.get()!=revision{return}
        match result {
            Ok(messages)=>{
                self.merge_history(&messages,from!=0);
                if let Some(id)=self.history_jump.get().filter(|id|*id==from||self.rows.borrow().contains_key(id)){
                    self.history_jump.set(None);
                    if self.rows.borrow().contains_key(&id){let weak=Rc::downgrade(self);glib::idle_add_local_once(move ||{if let Some(v)=weak.upgrade(){if v.history_revision.get()==revision{v.jump_to(id);}}});}
                    else{self.set_error("This message is not available in the current conversation.");}
                }
            },
            Err(e)=>{self.loading.set(false);self.older.set_visible(true);self.set_error(&e);}
        }
    }
    fn save_draft(&self) {
        if self.restoring.get() {
            return;
        }
        let Some(chat) = *self.current_chat_id.borrow() else { return };
        if self.current_chat.borrow().as_ref().is_some_and(|c| crate::prefs::unsupported_draft(&c.data["draft_message"]))
            && !crate::prefs::get().drafts.get(&self.draft_key()).is_some_and(|d| d.pending)
        {
            return;
        }
        let formatted = crate::compose::formatted(&self.text_view.buffer());
        let mut remote = crate::prefs::get().drafts.get(&self.draft_key()).map(|d| d.remote.clone()).unwrap_or(Value::Null);
        if self.current_reply.borrow().is_some() && remote.is_object() {
            remote["reply_to"] = Value::Null;
        }
        let draft = crate::prefs::Draft {
            text: formatted["text"].as_str().unwrap_or("").into(),
            formatted,
            remote,
            reply: self.reply_id.get(),
            quote: self.quote.borrow().clone().unwrap_or(Value::Null),
            updated_at: chrono::Utc::now().timestamp(),
            pending: true,
        };
        let key = self.draft_key();
        if draft.empty() && !crate::prefs::get().drafts.contains_key(&key) {
            return;
        }
        if crate::prefs::get().drafts.get(&key).is_some_and(|old| old.same_content(&draft)) {
            return;
        }
        crate::prefs::update(|p| {
            p.drafts.insert(key, draft.clone());
        });
        crate::window::with_service(|s| s.set_topic_draft(chat, self.topic_id(), draft.to_td()));
    }
    fn restore_draft(&self, draft: &crate::prefs::Draft) {
        self.restoring.set(true);
        crate::compose::load(&self.text_view.buffer(), &draft.formatted_text());
        self.current_reply.borrow_mut().take();
        self.reply_id.set(draft.reply);
        *self.quote.borrow_mut() = (!draft.quote.is_null()).then(|| draft.quote.clone());
        self.reply_banner.set_visible(draft.reply.is_some());
        if let Some(id) = draft.reply {
            self.reply_label.set_text(&if let Some(text) = draft.quote["text"]["text"].as_str() {
                format!("Quote: {text}")
            } else {
                format!("Reply to message {id}")
            });
        }
        self.restoring.set(false);
    }
    pub fn set_chat(self: &Rc<Self>, chat: &ChatSummary) { self.set_context(chat, Value::Null, ""); }
    pub fn set_context(self: &Rc<Self>, chat: &ChatSummary, topic: Value, title: &str) {
        self.history_revision.set(self.history_revision.get().wrapping_add(1));
        self.history_jump.set(None); self.pending_scroll.set(false);
        self.send_generation.set(self.send_generation.get() + 1);
        self.sending.set(false);
        self.save_draft();
        *self.topic.borrow_mut() = topic;
        self.context_bar.set_visible(!self.topic.borrow().is_null());
        self.context_label.set_text(title);
        self.restoring.set(true);
        *self.current_chat_id.borrow_mut() = None;
        self.text_view.buffer().set_text("");
        self.clear_reply();
        *self.current_chat_id.borrow_mut() = Some(chat.id);
        *self.current_chat.borrow_mut() = Some(chat.clone());
        self.pages.set_visible_child_name("chat");
        self.title_label.set_text(&crate::chat_list::display_title(chat));
        self.placeholder.set_text(&format!("Message {}", crate::chat_list::display_title(chat)));
        self.update_avatar(chat);
        self.members.set((0, 0));
        self.pinned_id.set(None);
        self.pinned_button.set_visible(false);
        self.apply_chat_status(chat);
        self.load_details(chat);
        self.clear_messages();
        self.has_more.set(true);
        self.loading.set(false);
        let key = self.draft_key();
        let local = crate::prefs::get().drafts.get(&key).cloned();
        let draft = if local.as_ref().is_some_and(|d| d.pending || d.formatted.is_null()) {
            let mut d = local.unwrap();
            d.pending = true;
            d
        } else if let Some(remote) = chat.data.get("draft_message") {
            crate::prefs::Draft::from_td(remote)
        } else {
            local.unwrap_or_default()
        };
        self.restore_draft(&draft);
        crate::prefs::update(|p| {
            p.last_chat_id = Some(chat.id);
            if draft.empty() && !draft.pending {
                p.drafts.remove(&key);
            } else {
                p.drafts.insert(key, draft.clone());
            }
        });
        if draft.pending {
            crate::window::with_service(|s| s.set_topic_draft(chat.id, self.topic_id(), draft.to_td()));
        }
    }
    pub fn update_avatar(&self, chat: &ChatSummary) {
        while let Some(c) = self.avatar_box.first_child() {
            self.avatar_box.remove(&c);
        }
        self.avatar_box.append(&crate::chat_list::build_avatar_widget(chat, 36));
    }
    pub fn clear_messages(&self) {
        self.clear_selection();
        self.messages.borrow_mut().clear();
        self.rows.borrow_mut().clear();
        self.retired_ids.borrow_mut().clear();
        while let Some(c) = self.message_list.first_child() {
            self.message_list.remove(&c);
        }
    }
    pub fn merge_history(self: &Rc<Self>, messages: &[MessageSummary], older: bool) {
        let adj = self.scroll_window.vadjustment();
        let old_height = adj.upper();
        let old_value = adj.value();
        let mut added = 0;
        for m in messages {
            if !self.messages.borrow().contains_key(&m.id) {
                self.append_message(m);
                added += 1;
            }
        }
        self.loading.set(false);
        self.has_more.set(!messages.is_empty());
        self.older.set_visible(false);
        // Older messages load as you scroll up; until there's enough to scroll, keep loading.
        if added > 0 {
            let weak = Rc::downgrade(self);
            let revision = self.history_revision.get();
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                if let Some(v) = weak.upgrade()
                    && v.history_revision.get() == revision
                {
                    let a = v.scroll_window.vadjustment();
                    if a.upper() <= a.page_size() + 1.0 {
                        v.load_older();
                    }
                }
            });
        }
        let status =
            if self.messages.borrow().is_empty() { "No messages yet".to_owned() } else { self.default_status.borrow().clone() };
        self.status_label.set_text(&status);
        if older {
            glib::idle_add_local_once(move || adj.set_value(old_value + (adj.upper() - old_height).max(0.0)));
        } else if self.history_jump.get().is_none() {
            self.scroll_to_bottom();
        }
    }
    pub fn append_message(self: &Rc<Self>, m: &MessageSummary) {
        if !crate::topics::matches_message(&self.topic.borrow(), &m.data) { return; }
        if m.data["scheduling_state"].is_object() {
            return;
        }
        if self.retired_ids.borrow().contains(&m.id) {
            return;
        }
        if self.current_chat_id.borrow().as_ref() != Some(&m.chat_id) {
            return;
        }
        let bottom = self.at_bottom();
        let is_new = !self.messages.borrow().contains_key(&m.id);
        self.messages.borrow_mut().insert(m.id, m.clone());
        if album_id(m) != 0 {
            self.rebuild_album(album_id(m));
            if bottom && is_new {
                self.scroll_to_bottom();
            }
            return;
        }
        let row = self.render(m);
        if let Some(old) = self.rows.borrow_mut().remove(&m.id) {
            self.message_list.remove(&old);
        }
        let previous = self
            .messages
            .borrow()
            .values()
            .filter(|v| v.id != m.id && (v.date, v.id) < (m.date, m.id))
            .max_by_key(|m| (m.date, m.id))
            .map(|m| m.id);
        let prev = previous.and_then(|id| self.rows.borrow().get(&id).cloned());
        self.message_list.insert_child_after(&row, prev.as_ref());
        self.rows.borrow_mut().insert(m.id, row);
        if bottom && is_new {
            self.scroll_to_bottom();
        }
    }
    pub fn replace_message(self: &Rc<Self>, old: MessageId, m: &MessageSummary) {
        if old != m.id {
            self.retired_ids.borrow_mut().insert(old);
            self.delete_messages(&[old]);
        }
        self.append_message(m);
    }
    pub fn delete_messages(self: &Rc<Self>, ids: &[MessageId]) {
        let mut albums = std::collections::BTreeSet::new();
        for id in ids {
            self.selected.borrow_mut().remove(id);
            self.retired_ids.borrow_mut().insert(*id);
            if let Some(m) = self.messages.borrow_mut().remove(id) {
                if album_id(&m) != 0 {
                    albums.insert(album_id(&m));
                }
            }
            if let Some(row) = self.rows.borrow_mut().remove(id) {
                if row.parent().as_ref() == Some(self.message_list.upcast_ref()) {
                    self.message_list.remove(&row);
                }
            }
        }
        for album in albums {
            self.rebuild_album(album);
        }
        self.queue_day_marks();
        if self.reply_id.get().is_some_and(|id| ids.contains(&id)) {
            self.clear_reply();
        }
        if self.selection_bar.is_visible() {
            if self.selected.borrow().is_empty() {
                self.clear_selection();
            } else {
                self.selection_label.set_text(&format!("{} selected", self.selected.borrow().len()));
            }
        }
    }
    fn rebuild_album(self: &Rc<Self>, album: i64) {
        let mut members = self.messages.borrow().values().filter(|m| album_id(m) == album).cloned().collect::<Vec<_>>();
        members.sort_by_key(|m| (m.date, m.id));
        let Some(first) = members.first() else { return };
        for m in &members {
            if let Some(old) = self.rows.borrow_mut().remove(&m.id) {
                if old.parent().as_ref() == Some(self.message_list.upcast_ref()) {
                    self.message_list.remove(&old);
                }
            }
        }
        let previous = self
            .messages
            .borrow()
            .values()
            .filter(|m| album_id(m) != album && (m.date, m.id) < (first.date, first.id))
            .max_by_key(|m| (m.date, m.id))
            .map(|m| m.id);
        let prev = previous.and_then(|id| self.rows.borrow().get(&id).cloned());
        let row = w::vbox(4);
        row.set_widget_name(&format!("album-{}", first.id));
        row.set_halign(if first.is_outgoing { gtk::Align::End } else { gtk::Align::Start });
        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_min_children_per_line(1);
        grid.set_max_children_per_line(2);
        grid.set_row_spacing(4);
        grid.set_column_spacing(4);
        for m in &members {
            grid.insert(&self.render(m), -1);
        }
        row.append(&grid);
        self.message_list.insert_child_after(&row, prev.as_ref());
        for m in members {
            self.rows.borrow_mut().insert(m.id, row.clone());
        }
    }
    pub fn visible_ids(&self) -> Vec<MessageId> {
        let adj = self.scroll_window.vadjustment();
        self.rows
            .borrow()
            .iter()
            .filter(|(_, r)| {
                let y = r
                    .compute_point(&self.message_list, &gtk::graphene::Point::new(0.0, 0.0))
                    .map(|p| p.y() as f64)
                    .unwrap_or(0.0);
                y + r.height() as f64 >= adj.value() && y <= adj.value() + adj.page_size()
            })
            .map(|(id, _)| *id)
            .collect()
    }
    fn render(self: &Rc<Self>, m: &MessageSummary) -> gtk::Box {
        let row = w::hbox(0);
        row.add_css_class("message-row");
        row.set_widget_name(&format!("message-{}", m.id));
        row.set_halign(if m.is_outgoing { gtk::Align::End } else { gtk::Align::Start });
        let bubble = w::vbox(5);
        bubble.add_css_class("message-bubble");
        bubble.add_css_class(if m.is_outgoing { "outgoing" } else { "incoming" });
        let group = self.current_chat.borrow().as_ref().is_some_and(|c| matches!(c.chat_type, ChatType::BasicGroup | ChatType::Supergroup));
        if group && !m.is_outgoing {
            let face = crate::chat_list::avatar(&m.sender_name, None, 32);
            face.set_valign(gtk::Align::End);
            face.add_css_class("message-sender-avatar");
            row.append(&face);
        }
        if !m.is_outgoing && !m.sender_name.is_empty() {
            let header=w::hbox(6);
            let l = w::label(&m.sender_name, "message-sender-name");
            l.set_xalign(0.0);l.set_ellipsize(gtk::pango::EllipsizeMode::End);l.set_max_width_chars(32);l.set_hexpand(true);
            if group {
                nyx_ui::sidebar::set_section(&l, &crate::chat_list::avatar_swatch(&m.sender_name));
            } else {
                l.add_css_class("section-accent");
            }
            header.append(&l);
            if let Some(tag)=m.data["sender_tag"].as_str().filter(|tag|!tag.is_empty()){
                let label=w::label(tag,"dim");label.set_max_width_chars(16);label.set_ellipsize(gtk::pango::EllipsizeMode::End);header.append(&label);
            }
            bubble.append(&header);
        }
        for key in ["voice_note","video_note"]{let result=&m.data["content"][key]["speech_recognition_result"];let text=match result["@type"].as_str(){Some("speechRecognitionResultText")=>result["text"].as_str(),Some("speechRecognitionResultPending")=>result["partial_text"].as_str(),Some("speechRecognitionResultError")=>result["error"]["message"].as_str(),_=>None};if let Some(text)=text.filter(|t|!t.is_empty()){let label=w::label(text,"message-transcription");label.set_wrap(true);label.set_selectable(true);label.set_max_width_chars(50);bubble.append(&label);}}
        if let Some(id) = m.reply_to_message_id {
            let reply = self
                .messages
                .borrow()
                .get(&id)
                .map(|r| format!("{}: {}", r.sender_name, preview(r)))
                .unwrap_or_else(|| "View replied message".into());
            let b = gtk::Button::with_label(&reply.chars().take(80).collect::<String>());
            b.add_css_class("flat");
            if let Some(label) = b.child().and_downcast::<gtk::Label>() {
                label.set_max_width_chars(28);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            }
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.jump_to(id);
                }
            });
            bubble.append(&b);
        }
        let original = m.files.iter().find(|f| f.role == "original");
        let thumbnail = m.files.iter().find(|f| f.role == "thumbnail");
        let path = original.and_then(|f| f.path.as_ref()).or_else(|| thumbnail.and_then(|f| f.path.as_ref()));
        let is_visual =
            matches!(m.content, MessageContent::Photo { .. } | MessageContent::Video { .. } | MessageContent::Sticker { .. });
        if is_visual {
            let mut rendered = false;
            if let Some(path) = path {
                if let Ok(tex) = gdk::Texture::from_file(&gio::File::for_path(path)) {
                    let pic = gtk::Picture::for_paintable(&tex);
                    pic.set_content_fit(gtk::ContentFit::Contain);
                    pic.set_can_shrink(true);
                    let width = if matches!(m.content, MessageContent::Sticker { .. }) {
                        160
                    } else if album_id(m) != 0 {
                        180
                    } else {
                        280
                    };
                    let height = ((width as f64 * tex.height() as f64 / tex.width().max(1) as f64) as i32).clamp(32, 300);
                    pic.set_size_request(width, height);
                    pic.add_css_class("message-picture");
                    pic.set_cursor_from_name(Some("pointer"));
                    pic.set_tooltip_text(Some("Open"));
                    let click = gtk::GestureClick::new();
                    let (weak, msg) = (Rc::downgrade(self), m.clone());
                    click.connect_released(move |_, _, _, _| {
                        if let Some(v) = weak.upgrade() {
                            v.open_media(&msg);
                        }
                    });
                    pic.add_controller(click);
                    bubble.append(&pic);
                    rendered = true;
                }
            }
            if !rendered && m.data["content"]["@type"] == "messageVideo" {
                if let Some(path) = thumbnail.and_then(|f| f.path.as_ref()) {
                    if let Ok(texture) = gdk::Texture::from_file(&gio::File::for_path(path)) {
                        let picture = gtk::Picture::for_paintable(&texture);
                        picture.set_content_fit(gtk::ContentFit::Contain);
                        picture.set_can_shrink(true);
                        picture.set_size_request(280, 180);
                        bubble.append(&picture);
                        rendered = true;
                    }
                }
            }
            if !rendered {
                if let Some(path) = path {
                    if path.ends_with(".tgs") {
                        let pic = gtk::Picture::new();
                        pic.set_size_request(140, 140);
                        rendered = crate::animation::play(&pic, path);
                        if rendered {
                            bubble.append(&pic);
                        }
                    } else if matches!(m.content, MessageContent::Sticker { .. })
                        || m.data["content"]["@type"] == "messageAnimation"
                    {
                        let pic = gtk::Picture::new();
                        crate::clip::play(&pic, path, 180, 160);
                        bubble.append(&pic);
                        rendered = true;
                    }
                }
            }
            let still = rendered && !matches!(m.content, MessageContent::Video { .. }) && m.data["content"]["@type"] != "messageAnimation";
            if !still {
                let b = gtk::Button::with_label(if matches!(m.content, MessageContent::Photo { .. }) {
                    "Load photo"
                } else if matches!(m.content, MessageContent::Sticker { .. }) {
                    "Load sticker"
                } else {
                    "▶ Play"
                });
                b.add_css_class("message-file-action");
                b.set_halign(gtk::Align::Start);
                let weak = Rc::downgrade(self);
                let msg = m.clone();
                b.connect_clicked(move |_| {
                    if let Some(v) = weak.upgrade() {
                        v.open_media(&msg);
                    }
                });
                bubble.append(&b);
            }
        }
        match &m.content {
            MessageContent::Text(t) => {
                self.append_rich(&bubble, t);
                if let Some(p) = m.data["content"].get("link_preview") {
                    let title = p["title"].as_str().unwrap_or("");
                    let url = p["url"].as_str().unwrap_or("");
                    if !title.is_empty() {
                        let link = gtk::LinkButton::with_label(url, title);
                        if let Some(label) = link.child().and_downcast::<gtk::Label>() {
                            label.set_max_width_chars(36);
                            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        }
                        bubble.append(&link);
                    }
                }
            }
            MessageContent::Photo { caption, .. } | MessageContent::Video { caption, .. } => self.append_rich(&bubble, caption),
            MessageContent::Sticker { emoji, .. } => {
                if path.is_none() {
                    bubble.append(&w::label(emoji, "message-text"));
                }
            }
            MessageContent::Document { file_name, mime_type, file_size, .. } => {
                bubble.append(&self.document_card(m, file_name, mime_type, *file_size));
            }
            MessageContent::VoiceNote { duration_seconds, .. } => {
                bubble.append(&self.voice_note(m, *duration_seconds));
            }
            MessageContent::Call { duration_seconds, is_video, discard_reason } => bubble.append(&w::label(
                &format!(
                    "{} call · {}s · {}",
                    if *is_video { "Video" } else { "Voice" },
                    duration_seconds,
                    discard_reason.trim_start_matches("callDiscardReason")
                ),
                "message-text",
            )),
            MessageContent::Unsupported(_) if crate::message_extras::render(&bubble,m) => {},
            MessageContent::Unsupported(_) if crate::rich_messages::render(&bubble, m) => {},
            MessageContent::Unsupported(_) => {
                let content = &m.data["content"];
                let text = match content["@type"].as_str().unwrap_or("") {
                    "messagePoll" => format!(
                        "Poll: {}",
                        content["poll"]["question"]["text"].as_str().or(content["poll"]["question"].as_str()).unwrap_or("Poll")
                    ),
                    "messageLocation" => {
                        format!("Location: {}, {}", content["location"]["latitude"], content["location"]["longitude"])
                    }
                    "messageContact" => format!(
                        "Contact: {} · {}",
                        content["contact"]["first_name"].as_str().unwrap_or(""),
                        content["contact"]["phone_number"].as_str().unwrap_or("")
                    ),
                    "messageAudio" => format!("Audio: {}", content["audio"]["file_name"].as_str().unwrap_or("Audio")),
                    "messageVideoNote" => "Video message".into(),
                    _ => "This message type is not supported yet.".into(),
                };
                bubble.append(&w::label(&text, "message-text"));
                if !m.files.is_empty() {
                    let b = gtk::Button::with_label("Download / Play");
                    let weak = Rc::downgrade(self);
                    let msg = m.clone();
                    b.connect_clicked(move |_| {
                        if let Some(v) = weak.upgrade() {
                            v.open_media(&msg);
                        }
                    });
                    bubble.append(&b);
                }
            }
        }
        if let Some(count)=m.data["interaction_info"]["reply_info"]["reply_count"].as_i64() {
            let b=gtk::Button::with_label(&format!("{} · {count}",if m.data["is_channel_post"]==true{"Comments"}else{"Replies"}));
            let message=m.clone(); b.connect_clicked(move |_|crate::topics::thread(message.chat_id,message.id)); bubble.append(&b);
        }
        let carded = matches!(m.content, MessageContent::Document { .. } | MessageContent::VoiceNote { .. }) || (is_visual && path.is_some());
        for f in m.files.iter().filter(|_| !carded) {
            if f.path.is_none() {
                let p = gtk::ProgressBar::new();
                p.set_fraction(if f.size > 0 { (f.downloaded as f64 / f.size as f64).clamp(0.0, 1.0) } else { 0.0 });
                bubble.append(&p);
                let b = gtk::Button::with_label(if f.active { "Cancel download" } else { "Download" });
                b.add_css_class("message-file-action");
                b.set_halign(gtk::Align::Start);
                let (id, active) = (f.id, f.active);
                b.connect_clicked(move |_| {
                    crate::window::with_service(|s| {
                        if active {
                            s.request("cancelDownloadFile", json!({"file_id":id,"only_if_pending":false}));
                        } else {
                            s.download(id);
                        }
                    })
                });
                bubble.append(&b);
            }
        }
        crate::bots::render(&bubble,m);
        for file in m.files.iter().filter(|f| f.role == "link-preview") {
            if let Some(path) = &file.path {
                if let Ok(texture) = gdk::Texture::from_file(&gio::File::for_path(path)) {
                    let picture = gtk::Picture::for_paintable(&texture);
                    picture.set_content_fit(gtk::ContentFit::Contain);
                    picture.set_can_shrink(true);
                    picture.set_size_request(260, 160);
                    bubble.append(&picture);
                }
            }
        }
        let local = chrono::DateTime::from_timestamp(m.date, 0).map(|d| d.with_timezone(&chrono::Local));
        let time = local.map(|d| d.format("%H:%M").to_string()).unwrap_or_default();
        let read = m.is_outgoing
            && self.current_chat.borrow().as_ref().and_then(|c| c.data["last_read_outbox_message_id"].as_i64()).unwrap_or(0) >= m.id;
        let (status, said) = if let Some(state) = &m.sending_state {
            if state.contains("Failed") { ("Failed — retry", "Failed") } else { ("Sending…", "Sending") }
        } else if m.is_outgoing {
            if read { ("✓✓", "Read") } else { ("✓", "Sent") }
        } else {
            ("", "")
        };
        let edited = if m.edit_date > 0 { "edited " } else { "" };
        let footer = w::label(format!("{edited}{time} {status}").trim_end(), "message-time");
        if read {
            footer.add_css_class("read");
        }
        let full = local.map(|d| d.format("%A %-d %B %Y, %H:%M").to_string()).unwrap_or_default();
        footer.set_tooltip_text(Some(&[full.as_str(), said].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")));
        footer.set_xalign(1.0);
        bubble.append(&footer);
        if m.sending_state.as_ref().is_some_and(|s| s.contains("Failed")) {
            let b = gtk::Button::with_label("Retry message");
            let (chat, id) = (m.chat_id, m.id);
            b.connect_clicked(move |_| {
                crate::window::with_service(|s| {
                    s.request("resendMessages", json!({"chat_id":chat,"message_ids":[id]}));
                })
            });
            bubble.append(&b);
        }
        let reactions = w::hbox(4);
        for reaction in m.data["interaction_info"]["reactions"]["reactions"].as_array().into_iter().flatten() {
            let b = gtk::Button::with_label(&format!(
                "{} {}",
                crate::actions::reaction_label(&reaction["type"]),
                reaction["total_count"].as_i64().unwrap_or(0)
            ));
            b.add_css_class("flat");
            if reaction["is_chosen"] == true {
                b.add_css_class("suggested-action");
            }
            let message = m.clone();
            let reaction_type = reaction["type"].clone();
            b.connect_clicked(move |_| crate::actions::toggle_reaction(message.clone(), reaction_type.clone()));
            reactions.append(&b);
        }
        bubble.append(&reactions);
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gdk::BUTTON_SECONDARY);
        let weak = Rc::downgrade(self);
        let parent = bubble.downgrade();
        let id = m.id;
        gesture.connect_pressed(move |gesture, _, x, y| {
            if let (Some(v), Some(parent)) = (weak.upgrade(), parent.upgrade()) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                crate::actions::show_message_menu(&v, parent.upcast_ref(), id, Some((x, y)));
            }
        });
        bubble.add_controller(gesture);
        let select = gtk::GestureClick::new();
        select.set_button(gdk::BUTTON_PRIMARY);
        let weak = Rc::downgrade(self);
        let id = m.id;
        select.connect_released(move |gesture, n, _, _| {
            if let Some(v) = weak.upgrade() {
                if v.selection_bar.is_visible() {
                    v.toggle_selection(id);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                } else if n == 2 && !gesture.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                    // Double-clicking selectable text keeps text-selection semantics.
                } else if gesture.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                    v.toggle_selection(id);
                }
            }
        });
        bubble.add_controller(select);
        row.set_focusable(true);
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        let parent = bubble.downgrade();
        let id = m.id;
        keys.connect_key_pressed(move |_, key, _, mods| {
            if let (Some(v), Some(parent)) = (weak.upgrade(), parent.upgrade()) {
                if key == gdk::Key::Menu || (key == gdk::Key::F10 && mods.contains(gdk::ModifierType::SHIFT_MASK)) {
                    crate::actions::show_message_menu(&v, parent.upcast_ref(), id, None);
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::space {
                    v.toggle_selection(id);
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Delete {
                    if let Some(message) = v.message(id) {
                        crate::actions::delete_messages(vec![message], Some(v.clone()));
                    }
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        row.add_controller(keys);
        if self.selected.borrow().contains(&m.id) {
            row.add_css_class("selected-message");
        }
        row.append(&bubble);
        self.queue_day_marks();
        row
    }

    fn queue_day_marks(&self) {
        if self.day_marks_pending.replace(true) {
            return;
        }
        let weak = self.self_weak.borrow().clone();
        glib::idle_add_local_once(move || {
            if let Some(v) = weak.upgrade() {
                v.day_marks_pending.set(false);
                v.refresh_day_marks();
            }
        });
    }

    /// A "Today", "Yesterday" or date pill before the first message of each day.
    fn refresh_day_marks(&self) {
        let messages = self.messages.borrow();
        let mut child = self.message_list.first_child();
        let mut last_day = None;
        let mut previous: Option<gtk::Widget> = None;
        while let Some(c) = child {
            child = c.next_sibling();
            if c.has_css_class("day-mark") {
                self.message_list.remove(&c);
                continue;
            }
            let name = c.widget_name();
            let id = name.strip_prefix("message-").or_else(|| name.strip_prefix("album-")).and_then(|s| s.parse::<i64>().ok());
            let day = id
                .and_then(|id| messages.get(&id))
                .and_then(|m| chrono::DateTime::from_timestamp(m.date, 0))
                .map(|d| d.with_timezone(&chrono::Local).date_naive());
            if let Some(day) = day
                && last_day != Some(day)
            {
                let mark = w::label(&day_title(day), "date-separator");
                mark.add_css_class("day-mark");
                mark.set_halign(gtk::Align::Center);
                self.message_list.insert_child_after(&mark, previous.as_ref());
                last_day = Some(day);
            }
            previous = Some(c);
        }
    }
    fn append_rich(&self, b: &gtk::Box, t: &FormattedText) {
        if t.text.is_empty() {
            return;
        }
        let content = crate::rich_text::widget(t);
        if t.entities.iter().any(|e| matches!(e.kind, TextEntityKind::Spoiler)) {
            let reveal = gtk::Expander::builder().label("Reveal spoiler").child(&content).build();
            b.append(&reveal);
        } else {
            b.append(&content);
        }
    }
    fn open_media(&self, m: &MessageSummary) {
        if let Some(cb) = self.on_media.borrow().as_ref() {
            cb(m);
        }
    }
    pub fn set_reply_to(&self, m: &MessageSummary) {
        self.quote.borrow_mut().take();
        self.reply_id.set(Some(m.id));
        *self.current_reply.borrow_mut() = Some(m.clone());
        self.reply_label.set_text(&format!("Replying to {}: {}", m.sender_name, preview(m)));
        self.reply_banner.set_visible(true);
        self.text_view.grab_focus();
        self.save_draft();
    }
    pub fn clear_reply(&self) {
        self.quote.borrow_mut().take();
        self.reply_id.set(None);
        *self.current_reply.borrow_mut() = None;
        self.reply_banner.set_visible(false);
        self.save_draft();
    }
    pub fn set_typing(&self, on: bool) {
        let status = if on { "typing…".to_owned() } else { self.default_status.borrow().clone() };
        self.status_label.set_text(&status);
    }
    pub fn set_error(&self, text: &str) {
        self.loading.set(false);
        self.status_label.set_text(text);
    }
    fn at_bottom(&self) -> bool {
        let a = self.scroll_window.vadjustment();
        a.value() + a.page_size() >= a.upper() - 80.0
    }
    pub fn scroll_to_bottom(self: &Rc<Self>) {
        if self.history_jump.get().is_some(){return}
        if self.pending_scroll.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        let revision=self.history_revision.get();
        glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
            if let Some(v) = weak.upgrade() {
                if v.history_revision.get()!=revision{return}
                let a = v.scroll_window.vadjustment();
                a.set_value((a.upper() - a.page_size()).max(0.0));
                v.pending_scroll.set(false);
                v.jump.set_visible(false);
            }
        });
    }
    pub fn jump_to(&self, id: i64) {
        if let Some(row) = self.rows.borrow().get(&id) {
            if let Some(p) = row.compute_point(&self.message_list, &gtk::graphene::Point::new(0.0, 0.0)) {
                self.scroll_window.vadjustment().set_value(p.y() as f64);
            }
        } else {
            self.history_jump.set(Some(id));self.load_history(id);
        }
    }

    fn load_older(&self) {
        if self.loading.get() || !self.has_more.get() {
            return;
        }
        if let Some(chat) = *self.current_chat_id.borrow() {
            let from = self.messages.borrow().values().min_by_key(|m| (m.date, m.id)).map(|m| m.id).unwrap_or(0);
            let _ = chat;
            self.load_history(from);
        }
    }
    fn send_current_text(self: &Rc<Self>) {
        self.send_text_options(Value::Null);
    }
    pub fn send_text_options(self: &Rc<Self>, mut options: Value) {
        let text = crate::compose::formatted(&self.text_view.buffer());
        if text["text"].as_str().unwrap_or("").trim().is_empty() {
            return;
        }
        let mut content = json!({"@type":"inputMessageText","text":text,"clear_draft":true});
        if let Some(draft) = crate::prefs::get().drafts.get(&self.draft_key()) {
            let preview = &draft.remote["content"]["link_preview_options"];
            if preview.is_object() {
                content["link_preview_options"] = preview.clone();
            }
            let effect = &draft.remote["effect_id"];
            if !effect.is_null() && options["effect_id"].is_null() {
                if !options.is_object() {
                    options = json!({"@type":"messageSendOptions"});
                }
                options["effect_id"] = effect.clone();
            }
        }
        self.send_content_options(content, options);
    }
    pub fn send_content(self: &Rc<Self>, content: Value) {
        self.send_content_options(content, Value::Null);
    }
    pub fn send_content_options(self: &Rc<Self>, content: Value, options: Value) {
        let Some(chat) = *self.current_chat_id.borrow() else { return };
        if self.sending.replace(true) {
            self.set_error("Wait for the current send to finish before sending another message");
            return;
        }
        let reply = self.reply_id.get();
        let Some(s) = crate::window::service() else {
            self.sending.set(false);
            return;
        };
        let is_text = content["@type"] == "inputMessageText";
        let sent_formatted = crate::compose::formatted(&self.text_view.buffer());
        let sent_reply = reply;
        let sent_quote = self.quote.borrow().clone().unwrap_or(Value::Null);
        let session = s.generation.clone();
        let topic = self.topic_id();
        let scheduled = options["scheduling_state"].is_object();
        let mut req = json!({"chat_id":chat,"topic_id":topic,"input_message_content":content,"options":options});
        if is_text {
            s.cancel_topic_draft(chat, &topic);
        }
        if let Some(id) = reply {
            req["reply_to"] = json!({"@type":"inputMessageReplyToMessage","message_id":id});
            if let Some(quote) = self.quote.borrow().as_ref() {
                req["reply_to"]["quote"] = quote.clone();
            }
        }
        let account = s.account.clone();
        let generation = self.send_generation.get() + 1;
        self.send_generation.set(generation);
        let keyboard=self.current_chat.borrow().as_ref().and_then(|c|c.data["reply_markup_message_id"].as_i64()).unwrap_or(0);
        let rx = s.request("sendMessage", req);
        let weak = Rc::downgrade(self);
        let key = self.draft_key();
        let old_text = self
            .text_view
            .buffer()
            .text(&self.text_view.buffer().start_iter(), &self.text_view.buffer().end_iter(), false)
            .to_string();
        glib::spawn_future_local(async move {
            let res = rx.recv().await;
            let Some(v) = weak.upgrade() else { return };
            if v.send_generation.get() == generation {
                v.sending.set(false);
            }
            let active = crate::prefs::get().active_account_id == account
                && crate::window::service_is_current(&session)
                && *v.current_chat_id.borrow() == Some(chat) && v.topic_id() == topic;
            match res {
                Ok(Ok(data)) => {
                    if is_text {
                        crate::prefs::update(|p| {
                            if p.drafts.get(&key).is_some_and(|d| {
                                d.formatted_text() == sent_formatted && d.reply == sent_reply && d.quote == sent_quote
                            }) {
                                p.drafts.remove(&key);
                            }
                        });
                    }
                    if active {
                        if keyboard!=0{crate::bots::consume_keyboard(chat,keyboard);}
                        if let Some(m) = parse_message(&data) {
                            v.append_message(&m);
                        }
                        let b = v.text_view.buffer();
                        if is_text
                            && b.text(&b.start_iter(), &b.end_iter(), false) == old_text
                            && crate::compose::formatted(&b) == sent_formatted
                            && v.reply_id.get() == sent_reply
                            && v.quote.borrow().as_ref().unwrap_or(&Value::Null) == &sent_quote
                        {
                            v.restoring.set(true);
                            b.set_text("");
                            v.clear_reply();
                            v.restoring.set(false);
                        } else if is_text {
                            v.save_draft();
                        }
                        if scheduled {
                            crate::window::show_status("Message scheduled");
                            crate::message_tools::changed(chat);
                        } else {
                            v.scroll_to_bottom();
                        }
                    }
                }
                Ok(Err(e)) if active => {
                    v.set_error(&e);
                    v.retry_draft();
                }
                Err(_) if active => {
                    v.set_error("Service stopped; draft preserved");
                    v.retry_draft();
                }
                _ => {}
            }
        });
    }
    fn paste_attachment(self: &Rc<Self>) -> bool {
        let clipboard = self.text_view.clipboard();
        let formats = clipboard.formats();
        let files = formats.contains_type(gdk::FileList::static_type());
        let image = formats.contains_type(gdk::Texture::static_type());
        if !files && !image {
            return false;
        }
        let weak = Rc::downgrade(self);
        let chat = *self.current_chat_id.borrow();
        let account = crate::prefs::get().active_account_id;
        let topic = self.topic_id();
        glib::spawn_future_local(async move {
            if files {
                if let Ok(value) = clipboard.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await {
                    if let Ok(files) = value.get::<gdk::FileList>() {
                        if let Some(v) = weak.upgrade() {
                            if *v.current_chat_id.borrow() == chat && v.topic_id()==topic && crate::prefs::get().active_account_id == account {
                                for f in files.files() {
                                    if let Some(path) = f.path() {
                                        v.attach(path);
                                    }
                                }
                            }
                        }
                    }
                }
            } else if let Ok(Some(texture)) = clipboard.read_texture_future().await {
                let path = nyx_core::paths::cache_dir().join(format!("paste-{}.png", uuid::Uuid::new_v4()));
                if texture.save_to_png(&path).is_ok() {
                    if let Some(v) = weak.upgrade() {
                        if *v.current_chat_id.borrow() == chat && v.topic_id()==topic && crate::prefs::get().active_account_id == account {
                            v.attach(path);
                        }
                    }
                }
            }
        });
        true
    }
    fn choose_attachment(self: &Rc<Self>) {
        let d = gtk::FileDialog::builder().title("Attach files or photos").build();
        let Some(win) = self.root.root().and_downcast::<gtk::Window>() else { return };
        let weak = Rc::downgrade(self);
        let chat=*self.current_chat_id.borrow();let topic=self.topic_id();let session=crate::window::service().map(|s|s.generation);
        d.open_multiple(Some(&win), gio::Cancellable::NONE, move |res| {
            if let (Ok(files), Some(v)) = (res, weak.upgrade()) {
                if *v.current_chat_id.borrow()!=chat||v.topic_id()!=topic||!session.as_ref().is_some_and(|s|crate::window::service_is_current(s)){return}
                for i in 0..files.n_items() {
                    if let Some(f) = files.item(i).and_downcast::<gio::File>() {
                        if let Some(path) = f.path() {
                            v.attach(path);
                        }
                    }
                }
            }
        });
    }
    pub fn attach(self: &Rc<Self>, path: std::path::PathBuf) {
        let Some(win) = self.root.root().and_downcast::<gtk::Window>() else { return };
        let d = gtk::Window::builder().title("Send attachment").transient_for(&win).modal(true).default_width(420).build();
        d.add_css_class("pulse-window");
        let box_ = w::vbox(12);
        box_.set_margin_start(16);
        box_.set_margin_end(16);
        box_.set_margin_top(16);
        box_.set_margin_bottom(16);
        let name = w::label(&path.file_name().unwrap_or_default().to_string_lossy(), "");
        name.set_max_width_chars(40);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        box_.append(&name);
        let is_photo = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| ["jpg", "jpeg", "png", "webp"].contains(&s.to_ascii_lowercase().as_str()));
        if is_photo {
            let p = gtk::Picture::for_filename(&path);
            p.set_content_fit(gtk::ContentFit::Contain);
            p.set_size_request(280, 180);
            box_.append(&p);
        }
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let is_video = ["mp4", "webm", "mov", "mkv"].contains(&extension.as_str());
        let is_animation = extension == "gif";
        let caption = gtk::TextView::new();
        caption.set_wrap_mode(gtk::WrapMode::WordChar);
        caption.set_size_request(-1, 60);
        let caption_scroll = gtk::ScrolledWindow::builder()
            .min_content_height(60)
            .max_content_height(160)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .child(&caption)
            .build();
        box_.append(&caption_scroll);
        let format = gtk::Button::with_label("Format caption");
        let editor = caption.clone();
        format.connect_clicked(move |button| crate::compose::menu(button.upcast_ref(), &editor));
        box_.append(&format);
        let buttons = w::hbox(8);
        let cancel = gtk::Button::with_label("Cancel");
        let send = gtk::Button::with_label("Send");
        buttons.append(&cancel);
        buttons.append(&send);
        box_.append(&buttons);
        d.set_child(Some(&box_));
        let dd = d.clone();
        cancel.connect_clicked(move |_| dd.close());
        let weak = Rc::downgrade(self);
        let chat = *self.current_chat_id.borrow();
        let dd = d.clone();
        let session = crate::window::service().map(|s| s.generation);
        let options = gtk::Button::with_label("Send options");
        buttons.append(&options);
        let options_path = path.clone();
        let options_caption = caption.clone();
        let options_view = weak.clone();
        let options_dialog = d.downgrade();
        let topic = self.topic_id();
        let options_topic = topic.clone();
        let options_session = session.clone();
        options.connect_clicked(move |button| {
            let Some(v) = options_view.upgrade() else { return };
            let Some(chat_id) = chat else { return };
            if *v.current_chat_id.borrow() != chat || v.topic_id() != options_topic
                || !options_session.as_ref().is_some_and(|s| crate::window::service_is_current(s))
            {
                return;
            }
            let content = attachment_content(
                &options_path,
                is_photo,
                is_video,
                is_animation,
                crate::compose::formatted(&options_caption.buffer()),
            );
            let weak = Rc::downgrade(&v);
            let dialog = options_dialog.clone();
            let session = options_session.clone();
            let topic = options_topic.clone();
            crate::message_tools::send_menu(button.upcast_ref(), chat_id, move |send_options| {
                if let Some(v) = weak.upgrade() {
                    if *v.current_chat_id.borrow() == Some(chat_id) && v.topic_id() == topic
                        && session.as_ref().is_some_and(|s| crate::window::service_is_current(s))
                    {
                        v.send_content_options(content.clone(), send_options);
                        if let Some(d) = dialog.upgrade() {
                            d.close();
                        }
                    }
                }
            });
        });
        send.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if *v.current_chat_id.borrow() != chat || v.topic_id() != topic || !session.as_ref().is_some_and(|s| crate::window::service_is_current(s))
                {
                    dd.close();
                    return;
                }
                v.send_content(attachment_content(
                    &path,
                    is_photo,
                    is_video,
                    is_animation,
                    crate::compose::formatted(&caption.buffer()),
                ));
            }
            dd.close();
        });
        crate::dialog_dismissal::escape(&d);
        d.present();
    }
    pub fn search_messages(self: &Rc<Self>) {
        if let Some(chat) = *self.current_chat_id.borrow() {
            crate::message_tools::search(Some(chat));
        }
    }
    pub fn connect_typing<F: Fn(ChatId) + 'static>(&self, f: F) {
        *self.on_typing.borrow_mut() = Some(Box::new(f));
    }
    pub fn connect_start_call<F: Fn(ChatId, bool) + 'static>(&self, f: F) {
        *self.on_start_call.borrow_mut() = Some(Box::new(f));
    }
    pub fn connect_toggle_details<F: Fn() + 'static>(&self, f: F) {
        *self.on_details.borrow_mut() = Some(Box::new(f));
    }
    pub fn connect_open_media<F: Fn(&MessageSummary) + 'static>(&self, f: F) {
        *self.on_media.borrow_mut() = Some(Box::new(f));
    }
}
pub fn preview(m: &MessageSummary) -> String {
    match &m.content {
        MessageContent::Text(t) => t.text.clone(),
        MessageContent::Photo { caption, .. } => {
            if caption.text.is_empty() {
                "Photo".into()
            } else {
                caption.text.clone()
            }
        }
        MessageContent::Video { caption, .. } => {
            if caption.text.is_empty() {
                "Video".into()
            } else {
                caption.text.clone()
            }
        }
        MessageContent::Document { file_name, .. } => file_name.clone(),
        MessageContent::Sticker { emoji, .. } => format!("{emoji} Sticker"),
        MessageContent::VoiceNote { .. } => "Voice message".into(),
        MessageContent::Call { .. } => "Call".into(),
        _ => "Message".into(),
    }
}

impl ChatView {
    pub fn update_chat(self: &Rc<Self>, chat: &ChatSummary) {
        let mut effective = chat.clone();
        if !self.topic.borrow().is_null() {
            effective.data["draft_message"] = self.current_chat.borrow().as_ref().map(|c|c.data["draft_message"].clone()).unwrap_or(Value::Null);
        }
        let chat = &effective;
        let old = self.current_chat.borrow().as_ref().and_then(|c| c.data["last_read_outbox_message_id"].as_i64());
        let draft_changed = self
            .current_chat
            .borrow()
            .as_ref()
            .is_some_and(|old| old.data.get("draft_message") != chat.data.get("draft_message"));
        if draft_changed {
            if let Some(remote) = chat.data.get("draft_message") {
                let draft = crate::prefs::Draft::from_td(remote);
                let key = self.draft_key();
                if !self.format_pending.get() && !crate::prefs::get().drafts.get(&key).is_some_and(|d| d.pending) {
                    self.restore_draft(&draft);
                    crate::prefs::update(|p| {
                        if draft.empty() {
                            p.drafts.remove(&key);
                        } else {
                            p.drafts.insert(key, draft);
                        }
                    });
                }
            }
        }
        *self.current_chat.borrow_mut() = Some(chat.clone());
        self.title_label.set_text(&crate::chat_list::display_title(chat));
        self.update_avatar(chat);
        self.apply_chat_status(chat);
        if old != chat.data["last_read_outbox_message_id"].as_i64() {
            let messages = self.messages.borrow().values().filter(|m| m.is_outgoing).cloned().collect::<Vec<_>>();
            for m in messages {
                self.append_message(&m);
            }
        }
    }
    pub fn media_messages(&self) -> Vec<MessageSummary> {
        let mut messages = self.messages.borrow().values().filter(|m| !m.files.is_empty()).cloned().collect::<Vec<_>>();
        messages.sort_by_key(|m| (m.date, m.id));
        messages
    }
}

impl ChatView {
    pub fn reset_account(&self) {
        self.save_draft();
        *self.current_chat_id.borrow_mut() = None;
        *self.topic.borrow_mut() = Value::Null;
        self.history_revision.set(self.history_revision.get().wrapping_add(1));
        self.context_bar.set_visible(false);
        self.text_view.buffer().set_text("");
        self.clear_reply();
        self.clear_messages();
        self.send_generation.set(self.send_generation.get() + 1);
        self.sending.set(false);
        *self.current_chat.borrow_mut() = None;
        self.title_label.set_text("Select a conversation");
        self.status_label.set_text("");
        self.default_status.borrow_mut().clear();
        while let Some(child) = self.avatar_box.first_child() {
            self.avatar_box.remove(&child);
        }
        self.composer.set_sensitive(false);
        self.call_button.set_visible(false);
        self.video_button.set_visible(false);
        self.pinned_button.set_visible(false);
        self.pinned_id.set(None);
        self.members.set((0, 0));
        self.details_revision.set(self.details_revision.get().wrapping_add(1));
        self.pages.set_visible_child_name("empty");
        self.loading.set(false);
        self.older.set_visible(false);
        self.jump.set_visible(false);
    }
}

impl ChatView {
    /// A file: an accent square, its name and size, and one action — Download,
    /// Cancel while it downloads, Open once it's here.
    fn document_card(self: &Rc<Self>, m: &MessageSummary, name: &str, mime: &str, size: i64) -> gtk::Box {
        let card = w::hbox(10);
        card.add_css_class("message-document-card");
        let icon = gtk::Image::from_icon_name("x-office-document-symbolic");
        icon.add_css_class("message-document-icon");
        icon.set_valign(gtk::Align::Center);
        card.append(&icon);
        let text = w::vbox(1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let title = w::label(name, "message-media-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        title.set_max_width_chars(28);
        title.set_tooltip_text(Some(name));
        text.append(&title);
        let kind = name.rsplit_once('.').map(|(_, ext)| ext.to_uppercase()).filter(|e| e.len() <= 5).unwrap_or_else(|| {
            mime.rsplit('/').next().unwrap_or("File").to_uppercase()
        });
        let file = m.files.iter().find(|f| f.role == "original").or(m.files.first());
        let sub = match file.filter(|f| f.active && f.size > 0) {
            Some(f) => format!("{} of {} · {kind}", human_size(f.downloaded), human_size(f.size)),
            None => format!("{} · {kind}", human_size(size)),
        };
        text.append(&w::label(&sub, "message-media-sub"));
        card.append(&text);
        let here = file.and_then(|f| f.path.as_ref()).is_some();
        let active = file.is_some_and(|f| f.active);
        let (label, icon) = if here {
            ("Open", "document-open-symbolic")
        } else if active {
            ("Cancel", "process-stop-symbolic")
        } else {
            ("Download", "folder-download-symbolic")
        };
        let action = w::labeled_button(icon, label);
        action.add_css_class("message-file-action");
        action.set_valign(gtk::Align::Center);
        let weak = Rc::downgrade(self);
        let msg = m.clone();
        let file_id = file.map(|f| f.id);
        action.connect_clicked(move |_| match (active, file_id) {
            (true, Some(id)) => {
                crate::window::with_service(|s| {
                    s.request("cancelDownloadFile", json!({"file_id":id,"only_if_pending":false}));
                });
            }
            _ => {
                if let Some(v) = weak.upgrade() {
                    v.open_media(&msg);
                }
            }
        });
        card.append(&action);
        card
    }

    /// A voice message: play, its waveform, and how long it is.
    fn voice_note(self: &Rc<Self>, m: &MessageSummary, seconds: i32) -> gtk::Box {
        let row = w::hbox(10);
        row.add_css_class("message-voice");
        let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
        play.add_css_class("message-voice-play");
        play.set_tooltip_text(Some("Play"));
        play.update_property(&[gtk::accessible::Property::Label("Play voice message")]);
        let weak = Rc::downgrade(self);
        let msg = m.clone();
        play.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.open_media(&msg);
            }
        });
        row.append(&play);
        let wave = w::hbox(2);
        wave.add_css_class("message-waveform");
        wave.set_valign(gtk::Align::Center);
        let played = m.data["content"]["is_listened"] == true;
        for level in waveform(m.data["content"]["voice_note"]["waveform"].as_str().unwrap_or(""), 28) {
            let bar = w::hbox(0);
            bar.add_css_class("waveform-bar");
            if played {
                bar.add_css_class("played");
            }
            bar.set_size_request(3, 3 + (level * 15.0) as i32);
            bar.set_valign(gtk::Align::Center);
            wave.append(&bar);
        }
        row.append(&wave);
        let length = w::label(&format!("{}:{:02}", seconds / 60, seconds % 60), "message-voice-duration");
        length.set_valign(gtk::Align::Center);
        length.set_margin_end(36);
        row.append(&length);
        row
    }
}

/// Telegram's waveform (5-bit levels, packed) as `bars` levels from 0 to 1.
fn waveform(encoded: &str, bars: usize) -> Vec<f64> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap_or_default();
    let count = bytes.len() * 8 / 5;
    if count == 0 {
        return vec![0.2; bars];
    }
    let level = |i: usize| {
        let bit = i * 5;
        let word = bytes[bit / 8] as u16 | (*bytes.get(bit / 8 + 1).unwrap_or(&0) as u16) << 8;
        ((word >> (bit % 8)) & 31) as f64 / 31.0
    };
    (0..bars).map(|b| level(b * count / bars)).collect()
}

fn human_size(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", b / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{b:.0} B")
    }
}

fn day_title(day: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    match (today - day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => day.format("%A").to_string(),
        _ if day.format("%Y").to_string() == today.format("%Y").to_string() => day.format("%-d %B").to_string(),
        _ => day.format("%-d %B %Y").to_string(),
    }
}

fn album_id(m: &MessageSummary) -> i64 {
    m.data["media_album_id"].as_i64().or_else(|| m.data["media_album_id"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

#[cfg(test)]
mod ui_regression {
    use super::*;
    #[test]
    fn tiled_layout_media_replacement_albums_and_drafts() {
        let protocol=nyx_telegram_calls::engine::Engine::protocol().expect("Load native call library before checking GTK isolation");
        assert!(protocol["library_versions"].as_array().is_some_and(|v|!v.is_empty()));
        gtk::init().expect("Run this check with a dedicated Broadway display");
        crate::actions::check_forward_selection();
        nyx_core::app::init(nyx_core::app::App { slug: "nyx-pulse-ui-test", name: "Pulse UI test" });
        crate::theme::install();
        let view = ChatView::new();
        let chat = ChatSummary {
            id: 7,
            title: "Test conversation".into(),
            chat_type: ChatType::Private,
            unread_count: 0,
            last_message: None,
            is_pinned: false,
            is_muted: false,
            avatar_path: None,
            order: 1,
            is_secret: false,
            data: json!({"permissions":{"can_send_basic_messages":true}}),
        };
        crate::message_extras::check_widgets();
        crate::compose::check_roundtrip();
        crate::animation::check_timer_lifetime();
        crate::detail_panel::check_layout_and_refresh(&chat);
        crate::chat_list::check_chat_removal(&chat);
        crate::places::check_places(&chat);
        view.set_chat(&chat);
        view.text_view.buffer().set_text("Preserve this draft");
        let mut second = chat.clone();
        second.id = 8;
        view.set_chat(&second);
        view.text_view.buffer().set_text("Other draft");
        view.set_chat(&chat);
        let buffer = view.text_view.buffer();
        assert_eq!(buffer.text(&buffer.start_iter(), &buffer.end_iter(), false), "Preserve this draft");
        let mut message=parse_message(&json!({"id":1,"chat_id":7,"date":1,"content":{"@type":"messagePhoto","photo":{"sizes":[{"width":80,"height":120,"photo":{"id":11,"size":100,"local":{"is_downloading_completed":false}}}]},"caption":{"text":"Caption with a long unbroken value: abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz","entities":[]}}})).unwrap();
        view.append_message(&message);
        view.append_message(&message);
        assert_eq!(view.rows.borrow().len(), 1);
        let bytes = glib::Bytes::from_owned(vec![180u8; 80 * 120 * 4]);
        let texture = gdk::MemoryTexture::new(80, 120, gdk::MemoryFormat::R8g8b8a8, &bytes, 80 * 4);
        let path = std::env::temp_dir().join(format!("pulse-ui-media-{}.png", uuid::Uuid::new_v4()));
        texture.save_to_png(&path).unwrap();
        message.files[0].path = Some(path.to_string_lossy().into());
        view.replace_message(1, &message);
        assert_eq!(view.rows.borrow().len(), 1);
        message.data["media_album_id"] = json!("91");
        view.append_message(&message);
        let mut other = message.clone();
        other.id = 2;
        view.append_message(&other);
        assert_eq!(view.rows.borrow().len(), 2);
        assert_eq!(view.rows.borrow().get(&1), view.rows.borrow().get(&2));
        view.delete_messages(&[1]);
        assert!(view.rows.borrow().get(&2).unwrap().parent().is_some());
        let mut final_message = other.clone();
        final_message.id = 3;
        final_message.data["media_album_id"] = json!(0);
        view.replace_message(2, &final_message);
        view.append_message(&other);
        assert!(!view.rows.borrow().contains_key(&2));
        let long_document=parse_message(&json!({"id":4,"chat_id":7,"date":2,"content":{"@type":"messageDocument","document":{"file_name":"a".repeat(300),"document":{"id":44,"size":5,"local":{"is_downloading_completed":false}}}}})).unwrap();
        view.append_message(&long_document);
        view.toggle_selection(3);
        assert_eq!(view.selected_messages().len(), 1);
        assert!(!view.composer.is_visible());
        view.clear_selection();
        assert!(view.composer.is_visible());
        let quote_message = parse_message(&json!({"id":5,"chat_id":7,"date":3,"content":{"@type":"messageText","text":{"text":"😀 quoted text","entities":[]}}})).unwrap();
        view.set_quote_to(&quote_message, "quoted");
        assert_eq!(view.quote.borrow().as_ref().unwrap()["position"], 3);
        view.set_chat(&second);
        view.set_chat(&chat);
        assert_eq!(view.reply_id.get(), Some(5));
        assert_eq!(view.quote.borrow().as_ref().unwrap()["position"], 3);
        view.clear_reply();
        let key = view.draft_key();
        crate::prefs::update(|p| {
            if let Some(draft) = p.drafts.get_mut(&key) {
                draft.pending = false;
            }
        });
        let mut remote = chat.clone();
        remote.data["draft_message"] = json!({"@type":"draftMessage","date":100,"content":{"@type":"draftMessageContentText","text":{"@type":"formattedText","text":"Remote draft","entities":[{"@type":"textEntity","offset":0,"length":6,"type":{"@type":"textEntityTypeBold"}}]}}});
        view.update_chat(&remote);
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"], "Remote draft");
        view.text_view.buffer().set_text("New local draft");
        remote.data["draft_message"]["content"]["text"]["text"] = json!("Stale remote draft");
        view.update_chat(&remote);
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"], "New local draft");
        let forum1=json!({"@type":"messageTopicForum","forum_topic_id":1});
        let forum2=json!({"@type":"messageTopicForum","forum_topic_id":2});
        let mut topic_chat=chat.clone();topic_chat.data["draft_message"]=Value::Null;
        view.set_context(&topic_chat,forum1.clone(),"First topic");
        assert!(view.context_bar.is_visible());
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"],"");
        view.text_view.buffer().set_text("First topic draft");
        let first_key=view.draft_key();
        let first_revision=view.history_revision.get();
        let topic_message=parse_message(&json!({"id":19,"chat_id":7,"date":5,"topic_id":forum1,"content":{"@type":"messageText","text":{"text":"In topic one","entities":[]}}})).unwrap();
        view.append_message(&topic_message);assert!(view.rows.borrow().contains_key(&19));
        let mut wrong_topic=topic_message.clone();wrong_topic.id=20;wrong_topic.data["topic_id"]=forum2.clone();
        view.append_message(&wrong_topic);assert!(!view.rows.borrow().contains_key(&20));
        view.set_context(&topic_chat,forum2.clone(),"Second topic");
        view.text_view.buffer().set_text("Second topic draft");
        assert_ne!(first_key,view.draft_key());
        view.topic_history(7,&forum1,first_revision,0,Ok(vec![topic_message]));
        assert!(view.rows.borrow().is_empty());
        view.remote_topic_draft(7,&forum1,&remote.data["draft_message"]);
        view.update_chat(&remote);
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"],"Second topic draft");
        view.set_context(&topic_chat,forum1,"First topic");
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"],"First topic draft");
        view.set_chat(&chat);
        assert!(view.topic_id().is_null());assert!(!view.context_bar.is_visible());
        assert_eq!(crate::compose::formatted(&view.text_view.buffer())["text"],"New local draft");
        view.append_message(&message);view.append_message(&long_document);
        let outgoing=parse_message(&json!({"id":90,"chat_id":7,"date":10,"is_outgoing":true,"content":{"@type":"messageText","text":{"text":"A reply with a link and a long unbroken value abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz","entities":[]}}})).unwrap();view.append_message(&outgoing);
        let mut long_chat=chat.clone();long_chat.title="A very long conversation title that must fit beside the info pane".into();view.update_chat(&long_chat);
        let panel=crate::detail_panel::DetailPanel::new();panel.set_chat(&long_chat);panel.root.set_visible(true);
        let layout=w::hbox(0);layout.append(&view.root);layout.append(&panel.root);
        let window = gtk::Window::builder().default_width(800).default_height(640).child(&layout).build();window.add_css_class("pulse-window");window.set_decorated(false);
        window.present();
        let loop_ = glib::MainLoop::new(None, false);
        let done = loop_.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || done.quit());
        loop_.run();
        let (minimum, _, _, _) = view.root.measure(gtk::Orientation::Horizontal, -1);
        assert!(minimum <= 748, "Conversation minimum width {minimum} exceeds a tiled window");
        let (combined,_,_,_)=layout.measure(gtk::Orientation::Horizontal,-1);assert!(combined<=800,"Chat and info pane minimum width {combined} exceeds a tiled window");
        assert!(layout.is_mapped());assert!(layout.width()>0);
        layout.queue_allocate();layout.allocate(layout.width(),layout.height(),-1,None);
        assert!(view.text_view.width()>=128,"The composer needs usable typing space beside the info pane");
        let snapshot = gtk::Snapshot::new();
        layout.snapshot_child(&view.root,&snapshot);layout.snapshot_child(&panel.root,&snapshot);
        let node=snapshot.to_node().expect("The chat and info pane must render for visual inspection");
        let renderer=gtk::gsk::CairoRenderer::new();renderer.realize_for_display(&gtk::prelude::WidgetExt::display(&window)).unwrap();renderer.render_texture(&node,None).save_to_png("/tmp/pulse-ui-audit.png").unwrap();renderer.unrealize();
        view.reset_account();
        assert!(view.current_chat.borrow().is_none());
        assert!(view.current_chat_id.borrow().is_none());
        assert!(!view.composer.is_sensitive());
        assert_eq!(view.status_label.text(), "");
        window.close();
        let _ = std::fs::remove_file(path);
    }
}

impl ChatView {
    fn status_text(&self, chat: &ChatSummary) -> String {
        if chat.is_secret {
            return match chat.data["pulse_secret_state"]["@type"].as_str() {
                Some("secretChatStateReady") => "End-to-end encrypted secret chat",
                Some("secretChatStateClosed") => "Secret chat closed",
                _ => "Secret chat pending",
            }
            .into();
        }
        if crate::chat_list::is_saved_messages(chat) {
            return "Messages you keep for yourself".into();
        }
        let (members, online) = self.members.get();
        match chat.chat_type {
            ChatType::Private if chat.data["pulse_user_online"] == true => "online".into(),
            ChatType::Private => "Telegram contact".into(),
            ChatType::Channel if members > 0 => format!("{members} subscribers"),
            ChatType::Channel => "Channel".into(),
            _ if members > 0 && online > 1 => format!("{members} members · {online} online"),
            _ if members > 0 => format!("{members} members"),
            _ => "Group".into(),
        }
    }
    fn apply_chat_status(&self, chat: &ChatSummary) {
        let status = self.status_text(chat);
        *self.default_status.borrow_mut() = status.clone();
        self.status_label.set_text(&status);
        let can_send = chat.data["pulse_can_send"]
            .as_bool()
            .or_else(|| chat.data["permissions"]["can_send_basic_messages"].as_bool())
            .unwrap_or(chat.chat_type != ChatType::Channel);
        let unsupported = crate::prefs::unsupported_draft(&chat.data["draft_message"])
            && !crate::prefs::get().drafts.get(&self.draft_key()).is_some_and(|d| d.pending);
        self.composer.set_sensitive(
            can_send && !unsupported && (!chat.is_secret || chat.data["pulse_secret_state"]["@type"] == "secretChatStateReady"),
        );
        if unsupported {
            self.status_label.set_text("This saved draft cannot be edited here. Discard it to write a new message.");
            *self.default_status.borrow_mut() =
                "This saved draft cannot be edited here. Discard it to write a new message.".into();
        }
        self.apply_call_availability(chat);
    }

    /// Calls only where they work: a person (not a bot, not yourself), or a group
    /// or channel with a voice chat going or that you may start one in.
    fn apply_call_availability(&self, chat: &ChatSummary) {
        let private = chat.chat_type == ChatType::Private && !chat.is_secret;
        let live = chat.data["video_chat"]["group_call_id"].as_i64().unwrap_or(0) != 0;
        self.call_button.set_visible(self.callable.get() || (!private && live));
        self.video_button.set_visible(private && self.callable.get());
        let label = if chat.chat_type == ChatType::Private { "Call" } else { "Voice chat" };
        self.call_label.set_text(label);
        self.call_button.set_tooltip_text(Some(if private { "Voice call" } else if live { "Join the voice chat" } else { "Start a voice chat" }));
        self.call_button.update_property(&[gtk::accessible::Property::Label(label)]);
    }

    fn start_call(&self, video: bool) {
        let Some(chat) = self.current_chat.borrow().clone() else { return };
        if chat.chat_type == ChatType::Private {
            if let Some(cb) = self.on_start_call.borrow().as_ref() {
                cb(chat.id, video);
            }
        } else {
            crate::group_calls::open(chat.id);
        }
    }

    /// What the header shows beyond the chat itself: the member count, the pinned
    /// message, and whether calls work here.
    fn load_details(self: &Rc<Self>, chat: &ChatSummary) {
        let revision = self.details_revision.get().wrapping_add(1);
        self.details_revision.set(revision);
        self.callable.set(false);
        let Some(service) = crate::window::service() else { return };
        let id = chat.id;
        let still = {
            let weak = Rc::downgrade(self);
            move || weak.upgrade().filter(|v| v.details_revision.get() == revision && *v.current_chat_id.borrow() == Some(id))
        };
        self.refresh_pinned();
        let group = chat.data["type"]["supergroup_id"].as_i64().map(|g| ("getSupergroupFullInfo", json!({"supergroup_id":g})));
        let basic = chat.data["type"]["basic_group_id"].as_i64().map(|g| ("getBasicGroupFullInfo", json!({"basic_group_id":g})));
        if let Some((method, args)) = group.or(basic) {
            let rx = service.request(method, args);
            let still = still.clone();
            glib::spawn_future_local(async move {
                let Ok(Ok(info)) = rx.recv().await else { return };
                let Some(v) = still() else { return };
                let count = info["member_count"].as_i64().or_else(|| info["members"].as_array().map(|m| m.len() as i64)).unwrap_or(0);
                v.members.set((count, v.members.get().1));
                v.refresh_status();
            });
        }
        let me = crate::window::my_id();
        let user = chat.data["type"]["user_id"].as_i64();
        let rx = match (user, me) {
            (Some(user), me) if !chat.is_secret && Some(user) != me => Some(service.request("getUser", json!({"user_id":user}))),
            (None, Some(me)) => {
                Some(service.request("getChatMember", json!({"chat_id":id,"member_id":{"@type":"messageSenderUser","user_id":me}})))
            }
            _ => None,
        };
        if let Some(rx) = rx {
            glib::spawn_future_local(async move {
                let Ok(Ok(found)) = rx.recv().await else { return };
                let Some(v) = still() else { return };
                let callable = match found["@type"].as_str() {
                    Some("user") => !matches!(found["type"]["@type"].as_str(), Some("userTypeBot" | "userTypeDeleted")),
                    _ => {
                        let status = &found["status"];
                        status["@type"] == "chatMemberStatusCreator"
                            || (status["@type"] == "chatMemberStatusAdministrator" && status["rights"]["can_manage_video_chats"] == true)
                    }
                };
                v.callable.set(callable);
                let chat = v.current_chat.borrow().clone();
                if let Some(chat) = chat {
                    v.apply_call_availability(&chat);
                }
            });
        }
    }

    pub fn refresh_pinned(self: &Rc<Self>) {
        let Some(id) = *self.current_chat_id.borrow() else { return };
        let Some(service) = crate::window::service() else { return };
        let revision = self.details_revision.get();
        let rx = service.request("getChatPinnedMessage", json!({"chat_id":id}));
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = rx.recv().await;
            let Some(v) = weak.upgrade() else { return };
            if v.details_revision.get() != revision || *v.current_chat_id.borrow() != Some(id) {
                return;
            }
            let message = match result {
                Ok(Ok(data)) => parse_message(&data),
                _ => None,
            };
            v.pinned_id.set(message.as_ref().map(|m| m.id));
            if let Some(m) = message {
                let text = preview(&m);
                v.pinned_text.set_text(text.lines().next().unwrap_or(""));
                v.pinned_button.set_tooltip_text(Some(&format!("Pinned: {text}")));
            }
            v.fit_header();
        });
    }

    pub fn set_online_count(&self, chat: i64, online: i64) {
        if *self.current_chat_id.borrow() == Some(chat) {
            self.members.set((self.members.get().0, online));
            self.refresh_status();
        }
    }

    fn refresh_status(&self) {
        let Some(chat) = self.current_chat.borrow().clone() else { return };
        let typing = self.status_label.text() == "typing…";
        let status = self.status_text(&chat);
        *self.default_status.borrow_mut() = status.clone();
        if !typing {
            self.status_label.set_text(&status);
        }
    }

    /// Info beside the chat: the Info button shows it's on, and the pinned message
    /// gives the header room.
    pub fn set_info_open(&self, open: bool) {
        self.info_open.set(open);
        if open {
            self.info_button.add_css_class("active");
        } else {
            self.info_button.remove_css_class("active");
        }
        self.fit_header();
    }

    /// In a narrow conversation the pinned message hides and Call keeps only its icon.
    fn fit_header(&self) {
        let width = self.root.width();
        let narrow = width > 0 && width < 620;
        self.pinned_button.set_visible(self.pinned_id.get().is_some() && !narrow && !self.info_open.get());
        self.call_label.set_visible(!narrow);
    }

    fn has_text(&self) -> bool {
        let buffer = self.text_view.buffer();
        !buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).trim().is_empty()
    }

    /// The round button records a voice message until there's text, then sends it.
    fn update_send_button(&self) {
        let (icon, tip) = if self.has_text() {
            ("document-send-symbolic", "Send (Enter). Right-click to send silently or schedule")
        } else {
            ("audio-input-microphone-symbolic", "Record a voice message")
        };
        self.placeholder.set_visible(!self.has_text());
        self.send_button.set_icon_name(icon);
        self.send_button.set_tooltip_text(Some(tip));
        self.send_button.update_property(&[gtk::accessible::Property::Label(if self.has_text() { "Send" } else { "Record a voice message" })]);
        if self.has_text() {
            self.send_button.add_css_class("sending");
        } else {
            self.send_button.remove_css_class("sending");
        }
    }
}

impl ChatView {
    pub fn reply_target(&self) -> Value {
        self.reply_id.get().map(|id| json!({"@type":"inputMessageReplyToMessage","message_id":id,"quote":self.quote.borrow().clone().unwrap_or(Value::Null)})).unwrap_or(Value::Null)
    }
    pub fn message(&self, id: i64) -> Option<MessageSummary> {
        self.messages.borrow().get(&id).cloned()
    }
    pub fn selected_messages(&self) -> Vec<MessageSummary> {
        self.selected.borrow().iter().filter_map(|id| self.message(*id)).collect()
    }
    pub fn selection_active(&self) -> bool {
        self.selection_bar.is_visible()
    }
    pub fn start_selection(&self) {
        self.selection_bar.set_visible(true);
        self.composer.set_visible(false);
        self.selection_label.set_text("Select messages");
    }
    pub fn toggle_selection(&self, id: i64) {
        self.start_selection();
        {
            let mut selected = self.selected.borrow_mut();
            if !selected.remove(&id) {
                selected.insert(id);
            }
        }
        self.selection_label.set_text(&format!("{} selected", self.selected.borrow().len()));
        self.refresh_selection_styles();
    }
    pub fn clear_selection(&self) {
        self.selected.borrow_mut().clear();
        self.selection_bar.set_visible(false);
        self.composer.set_visible(true);
        self.refresh_selection_styles();
    }
    fn refresh_selection_styles(&self) {
        fn walk(widget: &gtk::Widget, selected: &std::collections::BTreeSet<i64>) {
            if let Some(id) = widget.widget_name().strip_prefix("message-").and_then(|s| s.parse::<i64>().ok()) {
                if selected.contains(&id) {
                    widget.add_css_class("selected-message");
                } else {
                    widget.remove_css_class("selected-message");
                }
            }
            let mut child = widget.first_child();
            while let Some(c) = child {
                walk(&c, selected);
                child = c.next_sibling();
            }
        }
        walk(self.message_list.upcast_ref(), &self.selected.borrow());
    }
    pub fn forward_group(&self, id: i64) -> Vec<MessageSummary> {
        let Some(message) = self.message(id) else { return Vec::new() };
        let album = album_id(&message);
        if album == 0 {
            vec![message]
        } else {
            self.messages.borrow().values().filter(|m| album_id(m) == album).cloned().collect()
        }
    }
    pub fn set_quote_to(&self, message: &MessageSummary, quote: &str) {
        let formatted = crate::actions::message_text(message);
        let text = formatted["text"].as_str().unwrap_or("");
        let Some(position) = text.find(quote) else {
            crate::window::show_status("Select a plain-text portion of the message to quote");
            return;
        };
        self.set_reply_to(message);
        *self.quote.borrow_mut() = Some(
            json!({"@type":"inputTextQuote","text":{"@type":"formattedText","text":quote,"entities":[]},"position":text[..position].encode_utf16().count()}),
        );
        self.reply_label.set_text(&format!("Quote: {quote}"));
        self.save_draft();
    }
}

impl ChatView {
    fn formatting_changed(self: &Rc<Self>) {
        if self.restoring.get() || self.format_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(v) = weak.upgrade() {
                v.format_pending.set(false);
                v.save_draft();
            }
        });
    }
    fn send_options_menu(self: &Rc<Self>, parent: &gtk::Widget) {
        let Some(chat) = *self.current_chat_id.borrow() else { return };
        let weak = Rc::downgrade(self);
        let session = crate::window::service().map(|s| s.generation);
        let topic = self.topic_id();
        crate::message_tools::send_menu(parent, chat, move |options| {
            if let Some(v) = weak.upgrade() {
                if *v.current_chat_id.borrow() == Some(chat) && v.topic_id() == topic
                    && session.as_ref().is_some_and(|s| crate::window::service_is_current(s))
                {
                    v.send_text_options(options);
                }
            }
        });
    }
    pub fn insert_mention(&self, name: &str, user: i64) {
        crate::compose::insert_mention(&self.text_view.buffer(), name, user);
        self.save_draft();
        self.text_view.grab_focus();
    }
}

fn attachment_content(path: &std::path::Path, photo: bool, video: bool, animation: bool, caption: Value) -> Value {
    let file = json!({"@type":"inputFileLocal","path":path.to_string_lossy()});
    if photo {
        json!({"@type":"inputMessagePhoto","photo":file,"caption":caption})
    } else if video {
        json!({"@type":"inputMessageVideo","video":file,"caption":caption,"supports_streaming":true})
    } else if animation {
        json!({"@type":"inputMessageAnimation","animation":file,"caption":caption})
    } else {
        json!({"@type":"inputMessageDocument","document":file,"caption":caption})
    }
}
impl ChatView {
    fn retry_draft(&self) {
        if let Some(chat) = *self.current_chat_id.borrow() {
            if let Some(draft) = crate::prefs::get().drafts.get(&self.draft_key()) {
                crate::window::with_service(|s| s.set_topic_draft(chat, self.topic_id(), draft.to_td()));
            }
        }
    }
}

impl ChatView {
    pub fn discard_draft(&self) {
        self.restoring.set(true);
        self.text_view.buffer().set_text("");
        self.clear_reply();
        self.restoring.set(false);
        let key = self.draft_key();
        crate::prefs::update(|p| {
            p.drafts.remove(&key);
        });
        let chat = {
            let mut chat = self.current_chat.borrow_mut();
            if let Some(chat) = chat.as_mut() {
                chat.data["draft_message"] = Value::Null;
            }
            chat.clone()
        };
        if let Some(chat) = chat {
            self.apply_chat_status(&chat);
        }
    }
}

impl ChatView {
    pub fn remote_topic_draft(&self, chat:i64, topic:&Value, value:&Value) {
        if *self.current_chat_id.borrow()!=Some(chat)||self.topic_id()!=*topic{return}
        if let Some(current)=self.current_chat.borrow_mut().as_mut(){current.data["draft_message"]=value.clone();}
        let key=self.draft_key();
        if !self.format_pending.get()&&!crate::prefs::get().drafts.get(&key).is_some_and(|d|d.pending){
            let draft=crate::prefs::Draft::from_td(value);self.restore_draft(&draft);
            crate::prefs::update(|p|{if draft.empty(){p.drafts.remove(&key);}else{p.drafts.insert(key,draft);}});
        }
        if let Some(chat)=self.current_chat.borrow().as_ref(){self.apply_chat_status(chat);}
    }
}

impl ChatView {
    pub fn topic_name(&self,chat:i64,id:i64,name:&str){
        if *self.current_chat_id.borrow()==Some(chat)&&self.topic_id()["forum_topic_id"].as_i64()==Some(id){self.context_label.set_text(name);}
    }
}

impl ChatView { pub fn context_jump(&self,jump:Option<i64>){self.history_jump.set(jump);} }

impl ChatView {pub fn bot_force_reply(&self,message:&MessageSummary){if message.is_outgoing||*self.current_chat_id.borrow()!=Some(message.chat_id)||!crate::topics::matches_message(&self.topic_id(),&message.data)||message.data["reply_markup"]["@type"]!="replyMarkupForceReply"||self.reply_id.get().is_some(){return}let b=self.text_view.buffer();if b.text(&b.start_iter(),&b.end_iter(),false).is_empty(){self.set_reply_to(message);}}}
