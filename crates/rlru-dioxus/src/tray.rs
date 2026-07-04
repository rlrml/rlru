use dioxus::prelude::*;

#[cfg(all(feature = "desktop", target_os = "linux"))]
use crate::desktop::{
    cleanup_desktop_instance_socket, restore_desktop_window_handle, APP_ICON_PNG, APP_ID,
};
use crate::model::*;

#[cfg(all(feature = "desktop", target_os = "linux"))]
#[derive(Clone, Debug)]
enum TrayCommand {
    ShowWindow,
    ToggleWindow,
    SyncNow,
    RefreshHistory,
    Retry(ReplayUploadRequest),
    Quit,
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
struct TrayState {
    sender: std::sync::mpsc::Sender<TrayThreadMessage>,
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
impl Drop for TrayState {
    fn drop(&mut self) {
        let _ = self.sender.send(TrayThreadMessage::Shutdown);
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
enum TrayThreadMessage {
    Update(Box<TrayUpdate>),
    Shutdown,
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
struct TrayUpdate {
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
#[component]
pub(crate) fn DesktopTrayBridge(
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    use dioxus::desktop::WindowCloseBehaviour;
    use futures_util::StreamExt;

    let mut tray_state = use_signal(|| None);
    let mut window_hidden_to_tray = use_signal(|| false);
    let command_handler = use_coroutine(
        move |mut receiver: UnboundedReceiver<TrayCommand>| async move {
            while let Some(command) = receiver.next().await {
                match command {
                    TrayCommand::ShowWindow => show_window(window_hidden_to_tray),
                    TrayCommand::ToggleWindow => toggle_window_visibility(window_hidden_to_tray),
                    TrayCommand::SyncNow => {
                        show_window(window_hidden_to_tray);
                        onsync.call(());
                    }
                    TrayCommand::RefreshHistory => onrefreshhistory.call(()),
                    TrayCommand::Retry(request) => onretry.call(request),
                    TrayCommand::Quit => {
                        quit_application(tray_state);
                        return;
                    }
                }
            }
        },
    );

    use_hook(move || {
        let state = create_tray_state(command_handler.tx());
        let behaviour = if state.is_some() {
            WindowCloseBehaviour::WindowHides
        } else {
            WindowCloseBehaviour::WindowCloses
        };
        dioxus::desktop::window().set_close_behavior(behaviour);
        tray_state.set(state);
    });

    dioxus::desktop::use_wry_event_handler(move |event, _| {
        if matches!(
            event,
            dioxus::desktop::tao::event::Event::WindowEvent {
                event: dioxus::desktop::tao::event::WindowEvent::CloseRequested,
                ..
            }
        ) {
            window_hidden_to_tray.set(true);
        }
    });

    use_effect(use_reactive!(
        |summary, history, sync_run, failed_uploads| {
            if let Some(tray_state) = tray_state.read().as_ref() {
                update_tray_state(
                    tray_state,
                    summary.clone(),
                    history.clone(),
                    sync_run.clone(),
                    failed_uploads.clone(),
                );
            }
        }
    ));

    rsx! {}
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn create_tray_state(sender: UnboundedSender<TrayCommand>) -> Option<TrayState> {
    match create_tray_state_inner(sender) {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!("Failed to initialize rlru tray icon: {error}");
            None
        }
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn create_tray_state_inner(sender: UnboundedSender<TrayCommand>) -> Result<TrayState, String> {
    use ksni::blocking::TrayMethods;

    let tray = RlruTrayItem::new(sender);
    let (thread_sender, thread_receiver) = std::sync::mpsc::channel();
    let (handle_sender, handle_receiver) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = tray
            .spawn()
            .map_err(|error| format!("failed to build rlru tray icon: {error}"));
        match result {
            Ok(handle) => {
                let _ = handle_sender.send(Ok(()));
                run_tray_thread(handle, thread_receiver);
            }
            Err(error) => {
                let _ = handle_sender.send(Err(error));
            }
        }
    });

    handle_receiver
        .recv()
        .map_err(|error| format!("failed to start rlru tray thread: {error}"))??;

    Ok(TrayState {
        sender: thread_sender,
    })
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn run_tray_thread(
    handle: ksni::blocking::Handle<RlruTrayItem>,
    receiver: std::sync::mpsc::Receiver<TrayThreadMessage>,
) {
    for message in receiver {
        match message {
            TrayThreadMessage::Update(update) => {
                let _ = handle.update(move |tray| {
                    tray.summary = Some(update.summary);
                    tray.history = update.history;
                    tray.sync_run = update.sync_run;
                    tray.failed_uploads = update.failed_uploads;
                });
            }
            TrayThreadMessage::Shutdown => {
                handle.shutdown().wait();
                return;
            }
        }
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn update_tray_state(
    state: &TrayState,
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
) {
    let _ = state
        .sender
        .send(TrayThreadMessage::Update(Box::new(TrayUpdate {
            summary,
            history,
            sync_run,
            failed_uploads,
        })));
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn disabled_item<T>(label: impl Into<String>) -> ksni::menu::MenuItem<T> {
    ksni::menu::StandardItem {
        label: label.into(),
        enabled: false,
        ..Default::default()
    }
    .into()
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn action_item(
    label: &str,
    command: TrayCommand,
    sender: &UnboundedSender<TrayCommand>,
    enabled: bool,
) -> ksni::menu::MenuItem<RlruTrayItem> {
    let sender = sender.clone();
    ksni::menu::StandardItem {
        label: label.to_string(),
        enabled,
        activate: Box::new(move |_| {
            let _ = sender.unbounded_send(command.clone());
        }),
        ..Default::default()
    }
    .into()
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn submenu(
    label: impl Into<String>,
    items: Vec<ksni::menu::MenuItem<RlruTrayItem>>,
) -> ksni::menu::MenuItem<RlruTrayItem> {
    ksni::menu::SubMenu {
        label: label.into(),
        submenu: items,
        ..Default::default()
    }
    .into()
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn history_menu_items(
    history: Option<&Result<Vec<HistoryRow>, String>>,
    failed_uploads: &[ReplayUploadRequest],
) -> Vec<ksni::menu::MenuItem<RlruTrayItem>> {
    match history {
        None => vec![disabled_item("Loading current history")],
        Some(Err(error)) => vec![disabled_item(format!("History unavailable: {error}"))],
        Some(Ok(rows)) if rows.is_empty() => vec![disabled_item("No current history entries")],
        Some(Ok(rows)) => rows
            .iter()
            .take(8)
            .map(|row| {
                let mut row_items = vec![
                    disabled_item(format!("Account: {}", row.account)),
                    disabled_item(format!("When: {}", row.timestamp)),
                ];
                row_items.extend(row.upload_destinations.iter().map(|destination| {
                    let state = if let Some(failure) =
                        failed_upload(failed_uploads, &destination.target_name, &row.match_id)
                    {
                        failure.reason.as_deref().unwrap_or("Failed")
                    } else {
                        destination.state.as_str()
                    };
                    disabled_item(format!("{}: {}", destination.target_name, state))
                }));

                submenu(
                    format!(
                        "{} - {} - {}",
                        short_match_id(&row.match_id),
                        row.map_name,
                        row.score
                    ),
                    row_items,
                )
            })
            .collect(),
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn png_to_argb32(png_data: &[u8]) -> ksni::Icon {
    let image = image::load_from_memory_with_format(png_data, image::ImageFormat::Png)
        .expect("embedded PNG is valid")
        .into_rgba8();
    let data = image
        .pixels()
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect();
    ksni::Icon {
        width: image.width() as i32,
        height: image.height() as i32,
        data,
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn load_icon_set() -> Vec<ksni::Icon> {
    vec![png_to_argb32(APP_ICON_PNG)]
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
struct RlruTrayItem {
    summary: Option<AppSummary>,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    sender: UnboundedSender<TrayCommand>,
    icons: Vec<ksni::Icon>,
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
impl RlruTrayItem {
    fn new(sender: UnboundedSender<TrayCommand>) -> Self {
        Self {
            summary: None,
            history: None,
            sync_run: SyncRunState::default(),
            failed_uploads: Vec::new(),
            sender,
            icons: load_icon_set(),
        }
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
impl ksni::Tray for RlruTrayItem {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "rlru-dioxus".to_string()
    }

    fn title(&self) -> String {
        "rlru".to_string()
    }

    fn icon_name(&self) -> String {
        APP_ID.to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = self
            .summary
            .as_ref()
            .map(|summary| tray_tooltip(summary, &self.sync_run, self.failed_uploads.len()))
            .unwrap_or_else(|| "rlru\nTray data loading".to_string());

        ksni::ToolTip {
            title: "rlru".to_string(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.unbounded_send(TrayCommand::ShowWindow);
    }

    fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
        let mut items = Vec::new();
        items.push(disabled_item("rlru"));
        items.push(ksni::menu::MenuItem::Separator);

        if let Some(summary) = self.summary.as_ref() {
            items.push(disabled_item(tray_sync_label(&self.sync_run)));
            items.push(disabled_item(format!(
                "Auto upload: {}, {}",
                auto_upload_label(summary),
                summary.interval
            )));
        } else {
            items.push(disabled_item("Tray data loading"));
        }

        items.push(ksni::menu::MenuItem::Separator);
        items.push(submenu(
            "History",
            history_menu_items(self.history.as_ref(), &self.failed_uploads),
        ));

        if !self.failed_uploads.is_empty() {
            let failures = self
                .failed_uploads
                .iter()
                .cloned()
                .map(|request| {
                    action_item(
                        &format_failed_upload_retry_label(&request),
                        TrayCommand::Retry(request),
                        &self.sender,
                        true,
                    )
                })
                .collect();
            items.push(submenu(
                format!("Failed Uploads ({})", self.failed_uploads.len()),
                failures,
            ));
        }

        items.extend([
            ksni::menu::MenuItem::Separator,
            action_item(
                "Sync Now",
                TrayCommand::SyncNow,
                &self.sender,
                !self.sync_run.running,
            ),
            action_item(
                "Refresh History",
                TrayCommand::RefreshHistory,
                &self.sender,
                true,
            ),
            action_item("Open App", TrayCommand::ShowWindow, &self.sender, true),
            action_item(
                "Show/Hide Window",
                TrayCommand::ToggleWindow,
                &self.sender,
                true,
            ),
            ksni::menu::MenuItem::Separator,
            action_item("Quit", TrayCommand::Quit, &self.sender, true),
        ]);

        items
    }
}

#[cfg(all(test, feature = "desktop", target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn tray_left_click_activates_window_instead_of_menu() {
        const {
            assert!(!<RlruTrayItem as ksni::Tray>::MENU_ON_ACTIVATE);
        }
    }
}

#[cfg(not(all(feature = "desktop", target_os = "linux")))]
#[component]
pub(crate) fn DesktopTrayBridge(
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    let _ = (
        summary,
        history,
        sync_run,
        failed_uploads,
        onsync,
        onrefreshhistory,
        onretry,
    );
    rsx! {}
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn show_window(mut window_hidden_to_tray: Signal<bool>) {
    let win = dioxus::desktop::window();
    restore_desktop_window_handle(win.window.clone());
    window_hidden_to_tray.set(false);
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn toggle_window_visibility(mut window_hidden_to_tray: Signal<bool>) {
    let win = dioxus::desktop::window();
    if !window_hidden_to_tray() && win.window.is_visible() {
        win.set_visible(false);
        window_hidden_to_tray.set(true);
    } else {
        restore_desktop_window_handle(win.window.clone());
        window_hidden_to_tray.set(false);
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn quit_application(mut tray_state: Signal<Option<TrayState>>) {
    use dioxus::desktop::WindowCloseBehaviour;

    if let Some(tray_state) = tray_state.write().take() {
        let _ = tray_state.sender.send(TrayThreadMessage::Shutdown);
    }

    cleanup_desktop_instance_socket();

    let win = dioxus::desktop::window();
    win.set_close_behavior(WindowCloseBehaviour::WindowCloses);
    win.close();

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}
