// SPDX-License-Identifier: GPL-3.0

use crate::config::Config;
use crate::fl;
use crate::trash::{TrashBackend, TrashItem};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{futures, window::Id, Alignment, Length, Limits, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, Column, Row};
use futures::SinkExt;
use notify::{RecursiveMode, Watcher};
use std::time::Duration;

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    config: Config,
    trash_items: Vec<TrashItem>,
    total_size: u64,
    confirm_empty: bool,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: cosmic::Core::default(),
            popup: None,
            config: Config::default(),
            trash_items: Vec::new(),
            total_size: 0,
            confirm_empty: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    RefreshTrash,
    EmptyTrashRequested,
    ConfirmEmptyTrash,
    CancelEmptyTrash,
    OpenTrashFileManager,
    RestoreItem(String),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.github.abde.cosmic-applet-trash";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let trash_items = TrashBackend::list_items();
        let total_size = TrashBackend::calculate_total_size(&trash_items);

        let app = AppModel {
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            trash_items,
            total_size,
            confirm_empty: false,
            ..Default::default()
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let icon_name = if self.trash_items.is_empty() {
            "user-trash-symbolic"
        } else {
            "user-trash-full-symbolic"
        };

        self.core
            .applet
            .icon_button(icon_name)
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let item_count = self.trash_items.len();
        let formatted_size = TrashBackend::format_size(self.total_size);

        // Header section
        let header_title = if item_count == 0 {
            "Trash is Empty".to_string()
        } else if item_count == 1 {
            format!("1 Item ({})", formatted_size)
        } else {
            format!("{} Items ({})", item_count, formatted_size)
        };

        let header = Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(widget::icon::from_name(if item_count == 0 {
                "user-trash-symbolic"
            } else {
                "user-trash-full-symbolic"
            }).size(24))
            .push(widget::text::title3(header_title));

        // Action Buttons Row
        let mut action_row = Row::new().spacing(8);

        if item_count > 0 {
            action_row = action_row.push(
                widget::button::destructive(fl!("empty-trash"))
                    .on_press(Message::EmptyTrashRequested)
            );
        }

        action_row = action_row.push(
            widget::button::standard(fl!("open-trash"))
                .on_press(Message::OpenTrashFileManager)
        );

        let mut content = Column::new()
            .spacing(12)
            .padding(12)
            .push(header)
            .push(action_row);

        // Confirmation section for emptying trash
        if self.confirm_empty {
            let confirm_box = Column::new()
                .spacing(8)
                .padding(8)
                .push(widget::text::body("Permanently delete all items in trash?"))
                .push(
                    Row::new()
                        .spacing(8)
                        .push(
                            widget::button::destructive("Delete All")
                                .on_press(Message::ConfirmEmptyTrash)
                        )
                        .push(
                            widget::button::standard("Cancel")
                                .on_press(Message::CancelEmptyTrash)
                        )
                );
            content = content.push(confirm_box);
        }

        // Trash Items List
        if !self.trash_items.is_empty() {
            let mut list_col = Column::new().spacing(8);

            for item in &self.trash_items {
                let item_id = item.id.clone();
                let orig_path_str = item.original_path.to_string_lossy().to_string();

                let item_info = Column::new()
                    .spacing(2)
                    .push(widget::text::body(item.name.clone()))
                    .push(widget::text::caption(orig_path_str));

                let restore_btn = widget::button::standard(fl!("restore"))
                    .on_press(Message::RestoreItem(item_id));

                let item_row = Row::new()
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(widget::container(item_info).width(Length::Fill))
                    .push(restore_btn);

                list_col = list_col.push(item_row);
            }

            let scrollable_list = widget::scrollable(list_col)
                .height(Length::Fixed(250.0));

            content = content
                .push(widget::divider::horizontal::default())
                .push(scrollable_list);
        }

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        #[allow(dead_code)]
        struct TrashSubscription;

        Subscription::batch(vec![
            Subscription::run(|| {
                cosmic::iced::stream::channel(10, move |mut channel: futures::channel::mpsc::Sender<_>| async move {
                    let trash_dir = TrashBackend::get_trash_dir();
                    let _ = std::fs::create_dir_all(trash_dir.join("files"));
                    let _ = std::fs::create_dir_all(trash_dir.join("info"));

                    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
                    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                        if res.is_ok() {
                            let _ = tx.blocking_send(());
                        }
                    }).ok();

                    if let Some(w) = watcher.as_mut() {
                        let _ = w.watch(&trash_dir, RecursiveMode::Recursive);
                    }

                    let mut interval = tokio::time::interval(Duration::from_secs(2));

                    loop {
                        tokio::select! {
                            _ = rx.recv() => {
                                let _ = channel.send(Message::RefreshTrash).await;
                            }
                            _ = interval.tick() => {
                                let _ = channel.send(Message::RefreshTrash).await;
                            }
                        }
                    }
                })
            }),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::RefreshTrash => {
                self.trash_items = TrashBackend::list_items();
                self.total_size = TrashBackend::calculate_total_size(&self.trash_items);
            }
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::EmptyTrashRequested => {
                self.confirm_empty = true;
            }
            Message::CancelEmptyTrash => {
                self.confirm_empty = false;
            }
            Message::ConfirmEmptyTrash => {
                let _ = TrashBackend::empty_trash();
                self.confirm_empty = false;
                self.trash_items = TrashBackend::list_items();
                self.total_size = TrashBackend::calculate_total_size(&self.trash_items);
            }
            Message::OpenTrashFileManager => {
                TrashBackend::open_trash_in_file_manager();
            }
            Message::RestoreItem(item_id) => {
                if let Some(item) = self.trash_items.iter().find(|i| i.id == item_id) {
                    let _ = TrashBackend::restore_item(item);
                }
                self.trash_items = TrashBackend::list_items();
                self.total_size = TrashBackend::calculate_total_size(&self.trash_items);
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(400.0)
                        .min_width(320.0)
                        .min_height(180.0)
                        .max_height(600.0);
                    get_popup(popup_settings)
                }
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.confirm_empty = false;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
