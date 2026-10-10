//! Default-app registration runs only when the user clicks the settings button.
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{Result, ensure};
use std::path::Path;
#[derive(Debug)]
pub struct Registration {
    pub key: String,
    pub name: String,
    pub value: String,
    pub kind: &'static str,
}
pub fn windows_plan(exe: &Path) -> Result<Vec<Registration>> {
    let exe = exe.to_string_lossy();
    ensure!(
        !exe.contains(['\n', '\r', '\0', '"']),
        "程序路径含不支持字符"
    );
    let mut values = vec![];
    let mut add = |key: String, name: &str, value: String, kind: &'static str| {
        values.push(Registration {
            key,
            name: name.into(),
            value,
            kind,
        })
    };
    for (ext, label) in [("ofd", "OFD document"), ("pdf", "PDF document")] {
        let prog = format!("JianYue.{ext}");
        let root = format!("HKCU\\Software\\Classes\\{prog}");
        add(root.clone(), "", label.into(), "REG_SZ");
        add(
            format!("{root}\\DefaultIcon"),
            "",
            format!("\"{exe}\",0"),
            "REG_SZ",
        );
        add(
            format!("{root}\\shell\\open\\command"),
            "",
            format!("\"{exe}\" \"%1\""),
            "REG_SZ",
        );
        add(
            format!("HKCU\\Software\\Classes\\.{ext}\\OpenWithProgids"),
            &prog,
            String::new(),
            "REG_NONE",
        );
        add(
            "HKCU\\Software\\JianYue\\Capabilities\\FileAssociations".into(),
            &format!(".{ext}"),
            prog,
            "REG_SZ",
        );
    }
    add(
        "HKCU\\Software\\JianYue\\Capabilities".into(),
        "ApplicationName",
        "简阅".into(),
        "REG_SZ",
    );
    add(
        "HKCU\\Software\\JianYue\\Capabilities".into(),
        "ApplicationDescription",
        "OFD and PDF document reader".into(),
        "REG_SZ",
    );
    add(
        "HKCU\\Software\\RegisteredApplications".into(),
        "JianYue",
        "Software\\JianYue\\Capabilities".into(),
        "REG_SZ",
    );
    Ok(values)
}
pub fn desktop_entry(exe: &Path, icon: &Path) -> Result<String> {
    let quote = |p: &Path| -> Result<String> {
        let s = p.to_string_lossy();
        ensure!(!s.contains(['\n', '\r', '\0']), "路径含不支持字符");
        let mut quoted = String::new();
        for c in s.chars() {
            if matches!(c, '\\' | '"' | '$') || c as u32 == 96 {
                quoted.push('\\');
            }
            if c == '%' {
                quoted.push('%');
            }
            quoted.push(c);
        }
        Ok(quoted)
    };
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=简阅\nComment=OFD and PDF document reader\nExec=\"{}\" %f\nIcon={}\nTerminal=false\nCategories=Office;Viewer;\nMimeType=application/pdf;application/ofd;application/x-ofd;\n",
        quote(exe)?,
        icon.to_string_lossy()
    ))
}
#[cfg(windows)]
pub fn set_defaults(ofd: bool, pdf: bool) -> Result<String> {
    use std::os::windows::process::CommandExt;
    ensure!(ofd || pdf, "请选择至少一种文件类型");
    let exe = std::env::current_exe()?.canonicalize()?;
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    ensure!(
        count > 0 && count < buffer.len(),
        "无法查询 Windows 系统目录"
    );
    let reg =
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count])).join("reg.exe");
    for v in windows_plan(&exe)? {
        let mut cmd = std::process::Command::new(&reg);
        cmd.arg("add").arg(&v.key);
        if v.name.is_empty() {
            cmd.arg("/ve");
        } else {
            cmd.arg("/v").arg(v.name);
        }
        cmd.arg("/t").arg(v.kind);
        if v.kind != "REG_NONE" {
            cmd.arg("/d").arg(v.value);
        }
        let out = cmd.arg("/f").creation_flags(0x08000000).output()?;
        ensure!(
            out.status.success(),
            "无法注册文件类型：{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let uri: Vec<u16> = "ms-settings:defaultapps?registeredAppUser=JianYue"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let status = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            uri.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    } as isize;
    ensure!(
        status > 32,
        "已注册，但无法打开系统默认应用设置，请手动在设置中选择 简阅"
    );
    Ok("已注册 OFD/PDF 文件类型，请在系统设置中确认所选扩展名的默认程序。Windows 的默认选择需在系统窗口中完成。".into())
}
#[cfg(target_os = "linux")]
pub fn set_defaults(ofd: bool, pdf: bool) -> Result<String> {
    ensure!(ofd || pdf, "请选择至少一种文件类型");
    let exe = std::env::current_exe()?.canonicalize()?;
    let base = if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        std::path::PathBuf::from(dir)
    } else {
        std::path::PathBuf::from(std::env::var_os("HOME").context("无法确定用户目录")?)
            .join(".local/share")
    };
    ensure!(base.is_absolute(), "XDG_DATA_HOME 必须是绝对路径");
    let apps = base.join("applications");
    let icons = base.join("icons/hicolor/512x512/apps");
    std::fs::create_dir_all(&apps)?;
    std::fs::create_dir_all(&icons)?;
    let icon = icons.join("jian-yue.png");
    std::fs::write(&icon, include_bytes!("../assets/icons/icon-512.png"))?;
    std::fs::write(apps.join("jian-yue.desktop"), desktop_entry(&exe, &icon)?)?;
    if ofd {
        let dir = base.join("mime/packages");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("jian-yue.xml"),
            r#"<?xml version="1.0"?><mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info"><mime-type type="application/ofd"><comment>OFD document</comment><glob pattern="*.ofd"/></mime-type></mime-info>"#,
        )?;
        let out = std::process::Command::new("update-mime-database")
            .arg(base.join("mime"))
            .output()
            .context("请安装 shared-mime-info")?;
        ensure!(out.status.success(), "更新 OFD MIME 数据库失败");
    }
    let mimes = if ofd && pdf {
        vec!["application/ofd", "application/x-ofd", "application/pdf"]
    } else if ofd {
        vec!["application/ofd", "application/x-ofd"]
    } else {
        vec!["application/pdf"]
    };
    for mime in mimes {
        let out = std::process::Command::new("xdg-mime")
            .args(["default", "jian-yue.desktop", mime])
            .output()
            .context("请安装 xdg-utils")?;
        ensure!(out.status.success(), "设置 {mime} 默认程序失败");
        let query = std::process::Command::new("xdg-mime")
            .args(["query", "default", mime])
            .output()?;
        ensure!(
            String::from_utf8_lossy(&query.stdout).trim() == "jian-yue.desktop",
            "已提交设置，但桌面环境未确认 {mime} 默认程序，请在系统设置中手动选择"
        );
    }
    Ok("已设置所选 OFD/PDF 文件类型的默认程序".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn set_defaults(_: bool, _: bool) -> Result<String> {
    anyhow::bail!("当前平台不支持设置默认程序")
}
