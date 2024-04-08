use dirs;
use log::{error, info, warn};
use tauri::AppHandle;
use crate::commands::app_settings_commands::read_settings;

use crate::helpers::app_images_helpers::{app_image_extract_all, app_image_extract_desktop_file, install_app_image, install_icons};
use crate::helpers::desktop_file_builder::DesktopFileBuilder;
use crate::helpers::desktop_file_helpers::{delete_desktop_file_by_name, find_desktop_entry, find_desktop_file_location, read_all_app};
use crate::helpers::file_system_helper::{add_executable_permission, find_desktop_file_in_dir, rm_dir_all, rm_file};
use crate::models::app_list::{App, AppList};
use crate::models::request_installation::RequestInstallation;

#[tauri::command]
pub async fn install_app(app: AppHandle, request_installation: RequestInstallation) -> Result<String, String> {

    info!("##### REQUESTED TO INSTALL APP ####");
    info!("# File path: {:?}", request_installation.file_path);
    info!("# No sandbox: {:?}", request_installation.no_sandbox);
    info!("#################################");

    // Add executable permission to the AppImage
    add_executable_permission(&request_installation.file_path);

    // Read path where to install apps
    let apps_installation_path = match read_settings(app).await {
        Ok(settings) => settings.install_path.unwrap(),
        Err(err) => {
            error!("{}", err);
            return Err(err);
        }
    };

    // AppImage file path selected by the user
    let app_image_file_path = std::path::PathBuf::from(&request_installation.file_path);
    // AppImage file name
    let file_name = app_image_file_path.file_name().expect("Failed to get file name");
    // AppImage to install parent directory path
    let app_image_directory_path = app_image_file_path.parent().expect("Failed to get directory").to_path_buf();

    // Extract the AppImage .desktop file
    match app_image_extract_desktop_file(
        app_image_directory_path.to_str().unwrap(),
        file_name.to_string_lossy().to_string().as_str()
    ) {
        Ok(_) => {
            info!("AppImage extracted .desktop file successfully");
        }
        Err(err) => {
            error!("{}", err);
            return Err(err.to_string());
        }
    }

    // Squashfs-root directory path (app image extracted directory)
    let squashfs_path = std::path::PathBuf::from(app_image_directory_path.clone()).join("squashfs-root");

    // Find the desktop file in the squashfs-root directory
    //TODO can be improved? (.desktop file location is known)
    let desktop_file_path = match find_desktop_file_in_dir(
        squashfs_path.to_string_lossy().to_string().as_str()
    ) {
        Ok(path) => {
            info!("Desktop file found at: {:?}", path);
            path
        }
        Err(err) => {
            return Err(err);
        }
    };

    // Parse the desktop file
    let mut desktop_builder = match DesktopFileBuilder::from_desktop_entry_path(
        desktop_file_path.to_string(),
        false
    ) {
        Ok(db) => {
            info!("Desktop entry parsed successfully");
            db
        }
        Err(error) => {
            return Err(error);
        }
    };

    // Extract all appImage content
    match app_image_extract_all(
        app_image_directory_path.to_str().unwrap(),
        file_name.to_string_lossy().to_string().as_str()
    ) {
        Ok(_res) => {
            info!("AppImage extracted successfully");
        }
        Err(error) => {
            error!("Error extracting all app image content: {}", error);
            return Err(error.to_string());
        }
    }

    // Move icons to the system icons folder
    match install_icons(
        squashfs_path.to_str().unwrap(),
    ) {
        Ok(path) => {
            info!("Icon file copied to: {:?}", path);
        }
        Err(err) => {
            error!("{}", err);
            return Err(err.to_string());
        }
    }

    // Set mandatory fields
    desktop_builder.set_exec(format!(
        "{}{}",
        apps_installation_path,
        file_name.to_string_lossy()
    ));

    // Set optional fields
    //desktop_builder.set_icon(copied_icon_path);

    // Create destination path
    let desktop_files_location = find_desktop_file_location().map_err(|err| err.to_string())?;
    let desktop_files_location_path = std::path::PathBuf::from(desktop_files_location);
    //TODO: Check if the app name is present
    let desktop_entry_path = desktop_files_location_path
        .join(format!("{}.desktop", desktop_builder.name().unwrap()));

    // Set no sandbox
    if request_installation.no_sandbox.is_some() && request_installation.no_sandbox.unwrap() {
        info!("Setting no sandbox");
        desktop_builder.set_no_sandbox(true);
    }

    // Create the desktop entry
    match desktop_builder.write_to_file(desktop_entry_path.to_string_lossy().to_string()) {
        Ok(_) => {
            info!("Desktop entry created successfully");
        }
        Err(err) => {
            return Err(err);
        }
    }

    // Install the AppImage
    match install_app_image(
        &app_image_file_path.to_str().unwrap().to_string(),
        &apps_installation_path
    ) {
        Ok(res) => {
            info!("AppImage installed successfully");
        }
        Err(err) => {
            error!("{}", err);
            return Err(err);
        }
    }

    rm_dir_all(squashfs_path.to_str().unwrap()).expect("Failed to remove squashfs-root directory");

    Ok("Installation successful".to_string())
}

#[tauri::command]
pub async fn read_app_list() -> Result<AppList, String> {
    let apps: Vec<App> = read_all_app()?;
    Ok(AppList { apps })
}

#[tauri::command]
pub async fn uninstall_app(app: App) -> Result<bool, String> {
    let desktop_entry = match find_desktop_entry(app.name.clone()) {
        Ok(path) => path,
        Err(err) => {
            return Err(err);
        }
    };

    info!("Uninstalling app at: {:?}", &desktop_entry.exec);

    // Remove the AppImage
    let app_removed: bool = match rm_file(&desktop_entry.exec) {
        Ok(result) => {
            info!("AppImage removed successfully");
            result
        }
        Err(err) => {
            error!("{}", err);
            return Err(err);
        }
    };

    // Remove the icon
    let icon_removed: bool = match rm_file(&desktop_entry.icon_path) {
        Ok(result) => {
            info!("Icon removed successfully");
            result
        }
        Err(err) => {
            error!("{}", err);
            return Err(err);
        }
    };

    if !icon_removed {
        warn!("Failed to remove icon");
    }

    // Remove the desktop entry
    let desktop_removed: bool = match delete_desktop_file_by_name(&app.name) {
        Ok(result) => {
            info!("Desktop entry removed successfully");
            result
        }
        Err(err) => {
            error!("{}", err);
            return Err(err);
        }
    };

    if app_removed && desktop_removed {
        info!("App uninstalled successfully");
        Ok(true)
    } else {
        Err("Failed to remove app".to_string())
    }
}
