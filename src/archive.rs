use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

pub const MAX_ENTRY: u64 = 32 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 256 * 1024 * 1024;
pub struct Package {
    entries: BTreeMap<String, Arc<Vec<u8>>>,
}

/// OFD ST_Loc: leading slash is package-root-relative, otherwise relative to
/// the XML file containing the reference. Dot segments may never escape root.
pub fn resolve(source: &str, reference: &str) -> Result<String> {
    let reference = reference.trim();
    ensure!(!reference.is_empty(), "空的包内路径引用");
    ensure!(
        !reference.contains([':', '\0', '\\']),
        "不允许外部 URI 或非规范路径: {reference}"
    );
    let mut parts: Vec<&str> = if reference.starts_with('/') {
        vec![]
    } else {
        source.split('/').collect()
    };
    if !reference.starts_with('/') {
        parts.pop();
    }
    for p in reference.split('/') {
        match p {
            "" | "." => (),
            ".." => {
                ensure!(parts.pop().is_some(), "路径越过包根目录: {reference}");
            }
            _ => parts.push(p),
        }
    }
    ensure!(!parts.is_empty(), "路径没有文件名");
    Ok(parts.join("/"))
}
impl Package {
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            std::fs::File::open(path).with_context(|| format!("无法打开 {}", path.display()))?;
        Self::read(file)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::read(Cursor::new(bytes))
    }
    fn read<R: Read + std::io::Seek>(reader: R) -> Result<Self> {
        let mut zip =
            zip::ZipArchive::new(reader).context("不是有效的 OFD/ZIP 文件，或文件已加密")?;
        ensure!(zip.len() <= 8192, "ZIP 条目超过限制 8192");
        let mut entries = BTreeMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut f = zip
                .by_index(i)
                .context("ZIP 条目无法读取（可能加密或损坏）")?;
            if f.is_dir() {
                continue;
            }
            ensure!(!f.name().starts_with('/'), "ZIP 条目不能是绝对路径");
            let name = resolve("", f.name())?;
            ensure!(f.size() <= MAX_ENTRY, "包内文件过大: {name}");
            total = total.checked_add(f.size()).context("ZIP 大小溢出")?;
            ensure!(total <= MAX_TOTAL, "OFD 解压总量超过 256 MiB");
            ensure!(!entries.contains_key(&name), "ZIP 包含重复路径: {name}");
            let mut bytes = Vec::new();
            f.by_ref()
                .take(MAX_ENTRY + 1)
                .read_to_end(&mut bytes)
                .with_context(|| format!("损坏的 ZIP 条目 {name}"))?;
            ensure!(
                bytes.len() as u64 <= MAX_ENTRY,
                "条目实际解压大小过大: {name}"
            );
            entries.insert(name, Arc::new(bytes));
        }
        if !entries.contains_key("OFD.xml") {
            bail!("文件包缺少主入口 OFD.xml")
        }
        Ok(Self { entries })
    }
    pub fn bytes(&self, path: &str) -> Result<Arc<Vec<u8>>> {
        self.entries
            .get(path)
            .cloned()
            .with_context(|| format!("包内引用文件不存在: {path}"))
    }
    pub fn xml(&self, path: &str) -> Result<String> {
        let bytes = self.bytes(path)?;
        ensure!(bytes.len() <= 8 * 1024 * 1024, "XML 文件超过 8 MiB: {path}");
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("XML 不是 UTF-8: {path}"))?
            .trim_start_matches('\u{feff}')
            .to_owned();
        // roxmltree does not load external entities. Explicitly reject all DTDs.
        ensure!(
            !text.contains("<!DOCTYPE") && !text.contains("<!ENTITY"),
            "不接受 DTD / 外部实体: {path}"
        );
        Ok(text)
    }
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}
