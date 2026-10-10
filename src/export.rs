use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
fn resolved(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path.canonicalize().context("无法解析导出文件路径");
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent
        .canonicalize()
        .context("导出目录不存在")?
        .join(path.file_name().context("导出路径没有文件名")?))
}
pub fn ensure_distinct(source: &Path, target: &Path) -> Result<()> {
    let source = resolved(source)?;
    let target = resolved(target)?;
    #[cfg(windows)]
    let same = source.to_string_lossy().to_lowercase() == target.to_string_lossy().to_lowercase();
    #[cfg(not(windows))]
    let same = source == target;
    ensure!(!same, "导出路径不能是原始 OFD 文件，请选择其他文件名");
    Ok(())
}
pub fn write(source: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    ensure_distinct(source, target)?;
    std::fs::write(target, bytes).context("导出文件写入失败")
}
