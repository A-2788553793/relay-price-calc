pub const SHORTCUT_NAME: &str = "中转站计算器";

/// 路径和快捷方式名称均通过环境变量传递，脚本不插入任何调用方数据。
#[cfg(any(windows, test))]
fn shortcut_script() -> &'static str {
    r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
$desktop = [Environment]::GetFolderPath('Desktop')
if ([string]::IsNullOrWhiteSpace($desktop)) { throw '无法获取当前用户桌面目录' }
$path = Join-Path $desktop ($env:RELAY_SHORTCUT_NAME + '.lnk')
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($path)
$shortcut.TargetPath = $env:RELAY_SHORTCUT_EXE
$shortcut.WorkingDirectory = $env:RELAY_SHORTCUT_DIR
$shortcut.IconLocation = $env:RELAY_SHORTCUT_EXE + ',0'
$shortcut.Save()
Write-Output $path
"#
}

#[cfg(windows)]
fn shortcut_command(exe: &std::path::Path) -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;

    let dir = exe.parent().ok_or_else(|| "无法获取程序所在目录".to_string())?;
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            shortcut_script(),
        ])
        .env("RELAY_SHORTCUT_EXE", exe)
        .env("RELAY_SHORTCUT_DIR", dir)
        .env("RELAY_SHORTCUT_NAME", SHORTCUT_NAME)
        .creation_flags(0x08000000); // CREATE_NO_WINDOW
    Ok(command)
}

/// 在当前用户桌面创建或覆盖指向当前程序的快捷方式。
#[cfg(windows)]
pub fn create_desktop_shortcut() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("获取当前程序路径失败：{err}"))?;
    let output = shortcut_command(&exe)?
        .output()
        .map_err(|err| format!("启动 PowerShell 失败：{err}"))?;
    if !output.status.success() {
        return Err(format!(
            "创建桌面快捷方式失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("快捷方式路径不是有效的 UTF-8：{err}"))?;
    let path = stdout.trim_start_matches('\u{feff}').trim();
    if path.is_empty() {
        return Err("PowerShell 未返回快捷方式路径".to_string());
    }
    Ok(std::path::PathBuf::from(path))
}

#[cfg(not(windows))]
pub fn create_desktop_shortcut() -> Result<std::path::PathBuf, String> {
    Err("仅支持 Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_uses_redirected_desktop_utf8_and_environment_variables() {
        let script = shortcut_script();
        assert!(script.starts_with("[Console]::OutputEncoding = [Text.Encoding]::UTF8"));
        for required in [
            "$ErrorActionPreference = 'Stop'", "[Environment]::GetFolderPath('Desktop')",
            "WScript.Shell", ".CreateShortcut($path)", "$env:RELAY_SHORTCUT_NAME + '.lnk'",
            "$shortcut.TargetPath = $env:RELAY_SHORTCUT_EXE",
            "$shortcut.WorkingDirectory = $env:RELAY_SHORTCUT_DIR",
            "$shortcut.IconLocation = $env:RELAY_SHORTCUT_EXE + ',0'",
            "$shortcut.Save()", "Write-Output $path",
        ] {
            assert!(script.contains(required), "缺少脚本内容：{required}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn command_passes_unusual_paths_as_environment_data() {
        let exe = std::path::Path::new(r"C:\中文 空格\O'Brien $x; (test)\calc.exe");
        let command = shortcut_command(exe).unwrap();
        assert_eq!(command.get_program(), "powershell.exe");
        let args: Vec<_> = command.get_args().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(args, ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", shortcut_script()]);
        assert!(!shortcut_script().contains("O'Brien"));
        let envs: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(envs[std::ffi::OsStr::new("RELAY_SHORTCUT_EXE")], Some(exe.as_os_str()));
        assert_eq!(envs[std::ffi::OsStr::new("RELAY_SHORTCUT_DIR")], Some(exe.parent().unwrap().as_os_str()));
        assert_eq!(envs[std::ffi::OsStr::new("RELAY_SHORTCUT_NAME")], Some(std::ffi::OsStr::new(SHORTCUT_NAME)));
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_returns_chinese_error() {
        assert_eq!(create_desktop_shortcut(), Err("仅支持 Windows".to_string()));
    }
}
