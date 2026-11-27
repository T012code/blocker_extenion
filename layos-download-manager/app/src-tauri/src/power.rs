//! What happens when a scheduled queue finishes: quit, sleep or shut down.

use ldm_service::queues::{Queue, WhenDone};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn queue_finished(app: &AppHandle, queue: &Queue, action: WhenDone) {
    let name = &queue.name;
    match action {
        WhenDone::Nothing => notify(
            app,
            "Queue finished",
            &format!("\"{name}\" has downloaded everything."),
        ),
        WhenDone::Quit => {
            notify(
                app,
                "Queue finished",
                &format!("\"{name}\" is done. Closing Layó's Download Manager."),
            );
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                app.exit(0);
            });
        }
        WhenDone::Sleep => {
            notify(
                app,
                "Queue finished",
                &format!("\"{name}\" is done. Putting the computer to sleep."),
            );
            sleep_now();
        }
        WhenDone::ShutDown => {
            notify(
                app,
                "Shutting down in 60 seconds",
                &format!("\"{name}\" is done. Open Layó's Download Manager to cancel."),
            );
            if shut_down_soon(name).is_ok() {
                crate::state::to_main(app, "shutdown-pending", name);
            }
        }
    }
}

#[cfg(windows)]
fn run(program: &str, args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
}

#[cfg(windows)]
fn sleep_now() {
    // Sleep (not hibernate), let wake timers work.
    let _ = run("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]);
}

#[cfg(windows)]
fn shut_down_soon(queue: &str) -> std::io::Result<()> {
    let reason = format!("Layó's Download Manager finished \"{queue}\".");
    run("shutdown.exe", &["/s", "/t", "60", "/c", &reason]).map(|_| ())
}

#[cfg(windows)]
pub fn cancel_shutdown() -> Result<(), String> {
    match run("shutdown.exe", &["/a"]) {
        Ok(s) if s.success() => Ok(()),
        _ => Err("No shutdown was pending".into()),
    }
}

// Elsewhere (development on Linux/macOS) these only log, so testing never powers off.
#[cfg(not(windows))]
fn sleep_now() {
    tracing::info!("would put the computer to sleep");
}

#[cfg(not(windows))]
fn shut_down_soon(queue: &str) -> std::io::Result<()> {
    tracing::info!("would shut down after \"{queue}\"");
    Ok(())
}

#[cfg(not(windows))]
pub fn cancel_shutdown() -> Result<(), String> {
    Ok(())
}
