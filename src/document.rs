//! Common read-only model and renderer dispatch for OFD and PDF.
use crate::{
    model::*,
    parser,
    render::{Rendered, Renderer},
};
use anyhow::{Context, Result, ensure};
use sha2::Digest;
use std::{
    collections::BTreeSet,
    ops::Deref,
    path::Path,
    sync::{Arc, Mutex},
};

pub struct Document {
    model: OfdDocument,
    pdf: Option<hayro::Pdf>,
    pdf_limitations: Vec<String>,
    origin: Option<Arc<Document>>,
    work: Option<Arc<crate::editing::Project>>,
    pub source_hash: String,
}
impl Deref for Document {
    type Target = OfdDocument;
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}
impl Document {
    pub fn open(path: &Path, index: usize) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "pdf" {
            ensure!(index == 0, "PDF 不支持 OFD 包内多文档索引");
            ensure!(
                std::fs::metadata(path)?.len() <= 128 * 1024 * 1024,
                "PDF 文件超过 128 MiB 限制"
            );
            Self::pdf_from_bytes(std::fs::read(path)?)
        } else {
            Ok(Self {
                model: parser::open_document(path, index)?,
                pdf: None,
                pdf_limitations: vec![],
                origin: None,
                work: None,
                source_hash: crate::editing::source_hash(path)?,
            })
        }
    }
    pub fn from_ofd(model: OfdDocument) -> Self {
        Self {
            model,
            pdf: None,
            pdf_limitations: vec![],
            origin: None,
            work: None,
            source_hash: String::new(),
        }
    }
    pub fn is_pdf(&self) -> bool {
        self.origin
            .as_ref()
            .map(|d| d.is_pdf())
            .unwrap_or(self.pdf.is_some())
    }
    pub fn format(&self) -> &str {
        if self.is_pdf() { "PDF" } else { "OFD" }
    }
    pub fn report(&self) -> serde_json::Value {
        if self.is_pdf() {
            serde_json::json!({"reader_version":env!("CARGO_PKG_VERSION"),"format":"PDF","metadata":self.metadata,"pages":self.pages.len(),"issues":self.issues,"scope":"PDF 阅读提示，非 GB/T 42133 档案检查或签名验签"})
        } else {
            self.model.report()
        }
    }
    pub fn pdf_from_bytes(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() <= 128 * 1024 * 1024,
            "PDF 文件超过 128 MiB 限制"
        );
        let source_hash = hex::encode(sha2::Sha256::digest(&bytes));
        let parsed = lopdf::Document::load_mem(&bytes).context("PDF 文件损坏或格式不支持")?;
        ensure!(
            !parsed.trailer.has(b"Encrypt"),
            "暂不支持加密或密码保护的 PDF，请先保存为未加密文件"
        );
        let (render_bytes, font_issues, pdf_limitations) =
            crate::pdf_fonts::prepare(&parsed, &bytes)?;
        let pdf = hayro::Pdf::new(Arc::new(render_bytes))
            .map_err(|e| anyhow::anyhow!("PDF 解析失败: {e:?}"))?;
        ensure!(
            !pdf.pages().is_empty() && pdf.pages().len() <= 2000,
            "PDF 页面数量无效或超过 2000 页"
        );
        let mut issues = font_issues;
        let info = parsed
            .trailer
            .get(b"Info")
            .ok()
            .and_then(|o| parsed.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        let val = |name: &[u8]| {
            info.and_then(|i| i.get(name).ok())
                .and_then(|v| v.as_str().ok())
                .map(decode_string)
                .unwrap_or_default()
        };
        let metadata = Metadata {
            title: val(b"Title"),
            author: val(b"Author"),
            creator: val(b"Creator"),
            created: val(b"CreationDate"),
            modified: val(b"ModDate"),
            doc_id: String::new(),
            doc_type: "PDF".into(),
            version: parsed.version.clone(),
        };
        let mut pages = vec![];
        for (i, p) in pdf.pages().iter().enumerate() {
            let (w, h) = p.render_dimensions();
            ensure!(
                w.is_finite() && h.is_finite() && w > 0. && h > 0. && w <= 56000. && h <= 56000.,
                "PDF 页面尺寸无效或过大"
            );
            let text = match parsed.extract_text(&[(i + 1) as u32]) {
                Ok(s) => s,
                Err(e) => {
                    if issues.len() < 100 {
                        issues.push(Issue {
                            severity: "warning".into(),
                            clause: "PDF".into(),
                            location: format!("第 {} 页", i + 1),
                            message: format!("文字提取失败，仍可阅读页面: {e}"),
                        });
                    }
                    String::new()
                }
            };
            pages.push(Page {
                id: (i + 1).to_string(),
                area: Rect {
                    x: 0.,
                    y: 0.,
                    w: w * 25.4 / 72.,
                    h: h * 25.4 / 72.,
                },
                objects: vec![],
                resources: Resources::default(),
                text,
            });
        }
        let outlines = parsed
            .get_toc()
            .map(|toc| {
                toc.toc
                    .into_iter()
                    .map(|o| Outline {
                        title: o.title,
                        page: o.page.checked_sub(1).filter(|i| *i < pages.len()),
                        depth: o.level.saturating_sub(1),
                    })
                    .collect()
            })
            .unwrap_or_default();
        issues.push(Issue{severity:"info".into(),clause:"PDF".into(),location:"文档".into(),message:"本地只读显示；不执行 JavaScript、外部动作或表单脚本，未验证数字签名。交互表单、PDF 附件及部分高级渲染特性暂不支持。".into()});
        Ok(Self {
            model: OfdDocument {
                metadata,
                pages,
                outlines,
                attachments: vec![],
                issues,
                document_count: 1,
                signature_count: parsed
                    .objects
                    .values()
                    .filter(|o| {
                        o.as_dict().is_ok_and(|d| {
                            d.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"Sig")
                        })
                    })
                    .count(),
                copy_allowed: true,
                export_allowed: true,
                print_allowed: true,
            },
            pdf: Some(pdf),
            pdf_limitations,
            origin: None,
            work: None,
            source_hash,
        })
    }
    pub fn edited(origin: Arc<Document>, work: crate::editing::Project) -> Result<Self> {
        work.validate(&origin)?;
        if work != crate::editing::Project::new(&origin, origin.source_hash.clone()) {
            crate::editing::editable(&origin)?;
        }
        let mut model = origin.model.clone();
        model.pages = work
            .pages
            .iter()
            .map(|p| {
                let mut page = origin.pages[p.source].clone();
                page.area = work.area(&origin, p);
                for m in &p.marks {
                    if let crate::editing::Mark::Text { text, .. } = m {
                        page.text.push('\n');
                        page.text.push_str(text)
                    }
                }
                page
            })
            .collect();
        model.outlines.clear();
        for outline in &origin.outlines {
            if let Some(page) = outline
                .page
                .and_then(|p| work.pages.iter().position(|w| w.source == p))
            {
                let mut o = outline.clone();
                o.page = Some(page);
                model.outlines.push(o);
            }
        }
        Ok(Self {
            model,
            pdf: None,
            pdf_limitations: vec![],
            source_hash: origin.source_hash.clone(),
            origin: Some(origin),
            work: Some(Arc::new(work)),
        })
    }
    pub fn render(
        &self,
        renderer: &mut Renderer,
        index: usize,
        zoom: f32,
        rotation: u32,
    ) -> Result<Rendered> {
        ensure!(index < self.pages.len(), "页码超过文档范围");
        if let (Some(origin), Some(work)) = (&self.origin, &self.work) {
            let edit = &work.pages[index];
            let mut base = origin.render(renderer, edit.source, zoom, 0)?;
            crate::editing::paint(
                renderer,
                &mut base,
                origin.pages[edit.source].area,
                &edit.marks,
                zoom,
            )?;
            if edit.turns > 0 {
                base.pixmap = rotate(base.pixmap, edit.turns * 90)?;
            }
            if !rotation.is_multiple_of(360) {
                base.pixmap = rotate(base.pixmap, rotation)?;
            }
            return Ok(base);
        }
        let Some(pdf) = &self.pdf else {
            return renderer.render(&self.pages[index], zoom, rotation);
        };
        ensure!(zoom.is_finite() && zoom > 0. && zoom <= 16., "无效缩放比例");
        ensure!(rotation.is_multiple_of(90), "旋转角度必须是 90 的倍数");
        let page = &pdf.pages()[index];
        let (w, h) = page.render_dimensions();
        let scale = 96. / 72. * zoom;
        let width = (w * scale).ceil() as u32;
        let height = (h * scale).ceil() as u32;
        ensure!(
            width > 0
                && height > 0
                && width <= 16384
                && height <= 16384
                && width as u64 * height as u64 <= 16_000_000,
            "渲染尺寸超过 1600 万像素限制，请降低缩放比例"
        );
        let warnings = Arc::new(Mutex::new(
            self.pdf_limitations
                .iter()
                .cloned()
                .collect::<BTreeSet<String>>(),
        ));
        let sink = warnings.clone();
        let settings = hayro::InterpreterSettings {
            font_resolver: Arc::new(|query| match query {
                hayro::FontQuery::Standard(s) => Some(s.get_font_data()),
                hayro::FontQuery::Fallback(f) => {
                    let name = format!(
                        "{:?} {:?} {:?}",
                        f.font_name, f.font_family, f.post_script_name
                    )
                    .to_lowercase();
                    if [
                        "song", "hei", "kai", "fang", "cjk", "chinese", "gb", "宋", "黑", "楷",
                        "仿",
                    ]
                    .iter()
                    .any(|v| name.contains(v))
                    {
                        Some((Arc::new(crate::fonts::DEFAULT_CJK), 0))
                    } else {
                        Some(f.pick_standard_font().get_font_data())
                    }
                }
            }),
            warning_sink: Arc::new(move |w| {
                if let Ok(mut sink) = sink.lock()
                    && sink.len() < 100
                {
                    sink.insert(format!("PDF 渲染提示: {w:?}"));
                }
            }),
        };
        let pixels = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hayro::render(
                page,
                &settings,
                &hayro::RenderSettings {
                    x_scale: scale,
                    y_scale: scale,
                    width: Some(width as u16),
                    height: Some(height as u16),
                },
            )
        }))
        .map_err(|_| anyhow::anyhow!("PDF 引擎无法渲染此页面，可尝试降低缩放或检查文件"))?;
        let mut pixmap = tiny_skia::Pixmap::new(width, height).context("PDF 页面内存不足")?;
        pixmap.data_mut().copy_from_slice(pixels.data_as_u8_slice());
        if !rotation.is_multiple_of(360) {
            pixmap = rotate(pixmap, rotation)?;
        }
        let warnings = warnings
            .lock()
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        Ok(Rendered { pixmap, warnings })
    }
}
fn decode_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[254, 255]) {
        String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|v| u16::from_be_bytes([v[0], v[1]]))
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
fn rotate(src: tiny_skia::Pixmap, angle: u32) -> Result<tiny_skia::Pixmap> {
    let (w, h) = (src.width(), src.height());
    let (nw, nh) = if angle % 180 == 90 { (h, w) } else { (w, h) };
    let mut dst = tiny_skia::Pixmap::new(nw, nh).context("旋转内存不足")?;
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match angle % 360 {
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (y, w - 1 - x),
                _ => (x, y),
            };
            let a = ((y * w + x) * 4) as usize;
            let b = ((dy * nw + dx) * 4) as usize;
            dst.data_mut()[b..b + 4].copy_from_slice(&src.data()[a..a + 4]);
        }
    }
    Ok(dst)
}
