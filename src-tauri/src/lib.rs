mod projects;
mod runtime;

use projects::{
    add_project, add_service, list_projects, list_services, open_project_folder, remove_project,
    remove_service, rename_project, resolve_service_directory, update_service, ProjectStore,
    ProjectsState,
};
use runtime::{
    clear_service_logs, get_service_logs, get_service_runtime, get_service_start_preview,
    start_service, stop_service, RuntimeManager,
};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_file = app.path().app_data_dir()?.join("projects.json");
            app.manage(ProjectsState::new(ProjectStore::new(data_file)));
            app.manage(RuntimeManager::new(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            add_project,
            rename_project,
            remove_project,
            open_project_folder,
            list_services,
            add_service,
            update_service,
            remove_service,
            resolve_service_directory,
            get_service_start_preview,
            start_service,
            stop_service,
            get_service_runtime,
            get_service_logs,
            clear_service_logs
        ])
        .build(tauri::generate_context!())
        .expect("error while building Server Dashboard");

    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            if let Some(runtime) = app_handle.try_state::<RuntimeManager>() {
                if let Err(error) = runtime.shutdown() {
                    eprintln!("Server Dashboard shutdown cleanup failed: {error}");
                }
            }
        }
    });
}
