//! Launch-at-login registration.
//!
//! Each platform gets its native mechanism through `auto-launch`: an XDG
//! autostart entry on Linux, a LaunchAgent plist on macOS, and the per-user Run
//! key on Windows. The executable path is taken from the running process, so a
//! moved binary re-registers itself the next time the tray starts.

use auto_launch::{
    AutoLaunch, AutoLaunchBuilder, LinuxLaunchMode, MacOSLaunchMode, WindowsEnableMode,
};

/// Name shown by the desktop's startup settings.
const APP_NAME: &str = "git-check";

fn launcher() -> Result<AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let path = exe
        .to_str()
        .ok_or_else(|| "the executable path is not valid UTF-8".to_string())?;
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(APP_NAME)
        .set_app_path(path)
        .set_linux_launch_mode(LinuxLaunchMode::XdgAutostart)
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser);
    builder.build().map_err(|err| err.to_string())
}

/// Register or remove the login item, matching `enabled`.
pub fn apply(enabled: bool) -> Result<(), String> {
    let launcher = launcher()?;
    if enabled {
        launcher.enable().map_err(|err| err.to_string())
    } else {
        launcher.disable().map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_can_be_built_from_the_running_binary() {
        // Building performs no I/O; it only validates name and path.
        launcher().expect("launcher");
    }
}
