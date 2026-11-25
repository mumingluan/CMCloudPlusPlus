use crate::{transfer_activity::TransferOverview, AppState};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_dialog::DialogExt;

pub(crate) fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn report(app: &tauri::AppHandle, error: String) {
    show(app);
    app.dialog()
        .message(error)
        .title("传输操作未完成")
        .show(|_| {});
}

pub fn setup(app: &tauri::AppHandle) -> tauri::Result<()> {
    let handle = app;
    let open = MenuItem::with_id(handle, "open", "打开 CMCloud++", true, None::<&str>)?;
    let speed = MenuItem::with_id(
        handle,
        "speed",
        "上传 0 B/s · 下载 0 B/s",
        false,
        None::<&str>,
    )?;
    let progress = MenuItem::with_id(handle, "progress", "任务 0/0（0.0%）", false, None::<&str>)?;
    let size = MenuItem::with_id(
        handle,
        "size",
        "大小 0 B / 0 B（0.0%）",
        false,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(handle)?;
    let start = MenuItem::with_id(handle, "start", "开始全部传输", false, None::<&str>)?;
    let pause = MenuItem::with_id(handle, "pause", "暂停全部传输", false, None::<&str>)?;
    let exit = MenuItem::with_id(handle, "exit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        handle,
        &[
            &open,
            &separator,
            &speed,
            &progress,
            &size,
            &PredefinedMenuItem::separator(handle)?,
            &start,
            &pause,
            &PredefinedMenuItem::separator(handle)?,
            &exit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("CMCloud++")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show(app),
            "start" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let _session = state.session.lock().await;
                    if let Err(error) = state.transfers.resume_all().await {
                        report(&app, error);
                    }
                });
            }
            "pause" => {
                if let Err(error) = app.state::<AppState>().transfers.pause_all() {
                    report(app, error);
                }
            }
            "exit" => {
                // Pause and persist queue state before stopping the native runtime.
                match app.state::<AppState>().transfers.pause_all() {
                    Ok(()) => app.exit(0),
                    Err(error) => report(app, error),
                }
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous: Option<TraySnapshot> = None;
        loop {
            if let Ok(value) = app.state::<AppState>().transfers.overview().await {
                let next = TraySnapshot::from(&value);
                if previous
                    .as_ref()
                    .is_none_or(|last| last.can_start != next.can_start)
                {
                    let _ = start.set_enabled(next.can_start);
                }
                if previous
                    .as_ref()
                    .is_none_or(|last| last.can_pause != next.can_pause)
                {
                    let _ = pause.set_enabled(next.can_pause);
                }
                if previous
                    .as_ref()
                    .is_none_or(|last| last.speed != next.speed)
                {
                    let _ = speed.set_text(&next.speed);
                }
                if previous
                    .as_ref()
                    .is_none_or(|last| last.progress != next.progress)
                {
                    let _ = progress.set_text(&next.progress);
                }
                if previous.as_ref().is_none_or(|last| last.size != next.size) {
                    let _ = size.set_text(&next.size);
                }
                if previous
                    .as_ref()
                    .is_none_or(|last| last.tooltip != next.tooltip)
                {
                    let _ = tray.set_tooltip(Some(&next.tooltip));
                }
                previous = Some(next);
            } else if previous.take().is_some() {
                let _ = start.set_enabled(false);
                let _ = pause.set_enabled(false);
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct TraySnapshot {
    can_start: bool,
    can_pause: bool,
    speed: String,
    progress: String,
    size: String,
    tooltip: String,
}
impl From<&TransferOverview> for TraySnapshot {
    fn from(value: &TransferOverview) -> Self {
        let speed = value.speed_text();
        let progress = value.progress_text();
        let size = value.size_text();
        let text = format!(
            "CMCloud++\n{}\n任务 {}/{}（{:.1}%）\n{}",
            speed,
            value.completed_jobs,
            value.total_jobs,
            value.job_percent(),
            size
        );
        let mut units = 0;
        let tooltip = text
            .chars()
            .take_while(|ch| {
                units += ch.len_utf16();
                units <= 127
            })
            .collect();
        Self {
            can_start: value.paused_jobs > 0 || value.failed_jobs > 0,
            can_pause: value.active_jobs > 0 || value.queued_jobs > 0,
            speed,
            progress,
            size,
            tooltip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_display_values_reuse_the_same_snapshot() {
        let first = TransferOverview {
            upload_speed: 1024.01,
            ..Default::default()
        };
        let second = TransferOverview {
            upload_speed: 1024.02,
            ..Default::default()
        };
        assert_eq!(TraySnapshot::from(&first), TraySnapshot::from(&second));
        let paused = TransferOverview {
            paused_jobs: 1,
            ..Default::default()
        };
        assert_ne!(
            TraySnapshot::from(&paused),
            TraySnapshot::from(&TransferOverview::default())
        );
    }
    #[test]
    fn tooltip_respects_windows_utf16_limit_for_large_queues() {
        let value = TransferOverview {
            total_jobs: u64::MAX,
            completed_jobs: u64::MAX,
            total_bytes: u64::MAX,
            completed_bytes: u64::MAX,
            upload_speed: u64::MAX as f64,
            download_speed: u64::MAX as f64,
            ..Default::default()
        };
        let snapshot = TraySnapshot::from(&value);
        assert!(snapshot.tooltip.starts_with("CMCloud++\n"));
        assert!(snapshot.tooltip.encode_utf16().count() <= 127);
    }
}
