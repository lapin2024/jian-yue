//! Serializable, non-destructive edit projects. Coordinates are millimetres.
use crate::{
    document::Document,
    model::*,
    render::{Rendered, Renderer},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mark {
    Highlight {
        rect: Rect,
        color: [u8; 4],
    },
    Rectangle {
        rect: Rect,
        color: [u8; 4],
        width: f32,
    },
    Ink {
        points: Vec<Point>,
        color: [u8; 4],
        width: f32,
    },
    Text {
        rect: Rect,
        text: String,
        size: f32,
        color: [u8; 4],
    },
    Stamp {
        rect: Rect,
        png: Vec<u8>,
        label: String,
    },
}
impl Mark {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::Text { text, .. } => text.len() + 256,
            Self::Stamp { png, label, .. } => png.len() + label.len() + 256,
            Self::Ink { points, .. } => points.len() * std::mem::size_of::<Point>() + 256,
            _ => 256,
        }
    }
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Highlight { rect, .. }
            | Self::Rectangle { rect, .. }
            | Self::Text { rect, .. }
            | Self::Stamp { rect, .. } => *rect,
            Self::Ink { points, .. } => {
                let x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
                let y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
                let right = points.iter().map(|p| p.x).fold(0f32, f32::max);
                let bottom = points.iter().map(|p| p.y).fold(0f32, f32::max);
                Rect {
                    x,
                    y,
                    w: right - x,
                    h: bottom - y,
                }
            }
        }
    }
    pub fn validate(&self, area: Rect) -> Result<()> {
        let r = self.bounds();
        ensure!(
            [r.x, r.y, r.w, r.h].iter().all(|n| n.is_finite())
                && r.x >= 0.
                && r.y >= 0.
                && r.w >= 0.
                && r.h >= 0.
                && r.x + r.w <= area.w + 0.1
                && r.y + r.h <= area.h + 0.1,
            "注释位置超出页面或无效"
        );
        match self {
            Self::Text { text, size, .. } => ensure!(
                !text.is_empty()
                    && text.len() <= 12000
                    && size.is_finite()
                    && *size >= 0.5
                    && *size <= 50.,
                "文字长度或字号无效"
            ),
            Self::Stamp { png, .. } => {
                ensure!(png.len() <= 8 * 1024 * 1024, "印章图片超过 8 MiB");
                let mut reader =
                    image::ImageReader::new(std::io::Cursor::new(png)).with_guessed_format()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                reader.decode().context("印章图片损坏或过大")?;
            }
            Self::Ink { points, width, .. } => ensure!(
                points.len() >= 2
                    && points.len() <= 12000
                    && points.iter().all(|p| p.x.is_finite() && p.y.is_finite())
                    && width.is_finite()
                    && *width > 0.
                    && *width <= 10.,
                "手绘数据无效"
            ),
            Self::Rectangle { width, .. } => ensure!(
                width.is_finite() && *width > 0. && *width <= 10.,
                "线宽无效"
            ),
            _ => (),
        }
        Ok(())
    }
    pub fn move_to(&mut self, x: f32, y: f32, area: Rect) {
        let b = self.bounds();
        let dx = x.clamp(0., (area.w - b.w).max(0.)) - b.x;
        let dy = y.clamp(0., (area.h - b.h).max(0.)) - b.y;
        match self {
            Self::Ink { points, .. } => {
                for p in points {
                    p.x += dx;
                    p.y += dy
                }
            }
            Self::Highlight { rect, .. }
            | Self::Rectangle { rect, .. }
            | Self::Text { rect, .. }
            | Self::Stamp { rect, .. } => {
                rect.x += dx;
                rect.y += dy
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditPage {
    pub source: usize,
    pub turns: u32,
    pub marks: Vec<Mark>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub source_sha256: String,
    pub pages: Vec<EditPage>,
}
impl Project {
    pub fn new(doc: &Document, hash: String) -> Self {
        Self {
            version: 1,
            source_sha256: hash,
            pages: (0..doc.pages.len())
                .map(|source| EditPage {
                    source,
                    turns: 0,
                    marks: vec![],
                })
                .collect(),
        }
    }
    pub fn validate(&self, source: &Document) -> Result<()> {
        ensure!(
            self.source_sha256 == source.source_hash,
            "编辑项目原件摘要不一致"
        );
        ensure!(
            self.version == 1 && !self.pages.is_empty() && self.pages.len() <= 2000,
            "项目版本或页面数量无效"
        );
        let mut count = 0usize;
        let mut image_bytes = 0usize;
        let mut mark_bytes = 0usize;
        for p in &self.pages {
            ensure!(
                p.source < source.pages.len() && p.turns < 4,
                "项目页引用无效"
            );
            count += p.marks.len();
            ensure!(count <= 10000, "标注数量超过 10000");
            for mark in &p.marks {
                mark_bytes += mark.estimated_bytes();
                ensure!(mark_bytes <= 16 * 1024 * 1024, "编辑内容总量超过 16 MiB");
                if let Mark::Stamp { png, .. } = mark {
                    image_bytes += png.len();
                }
                mark.validate(Rect {
                    x: 0.,
                    y: 0.,
                    ..source.pages[p.source].area
                })?;
            }
        }
        ensure!(image_bytes <= 16 * 1024 * 1024, "印章图片总量超过 16 MiB");
        Ok(())
    }
    pub fn area(&self, source: &Document, page: &EditPage) -> Rect {
        let mut a = source.pages[page.source].area;
        a.x = 0.;
        a.y = 0.;
        if page.turns % 2 == 1 {
            std::mem::swap(&mut a.w, &mut a.h);
        }
        a
    }
    pub fn load(path: &Path, source: &Document, hash: &str) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)?.len() <= 32 * 1024 * 1024,
            "编辑项目超过 32 MiB"
        );
        let p: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            p.source_sha256 == hash,
            "编辑项目属于其他文件，原件内容哈希不一致"
        );
        p.validate(source)?;
        Ok(p)
    }
}
pub fn source_hash(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(std::fs::read(path)?)))
}
pub struct Editor {
    pub project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    saved: Project,
}
impl Editor {
    pub fn new(project: Project) -> Self {
        Self {
            saved: project.clone(),
            project,
            undo: vec![],
            redo: vec![],
        }
    }
    pub fn dirty(&self) -> bool {
        self.project != self.saved
    }
    pub fn saved(&mut self) {
        self.saved = self.project.clone()
    }
    pub fn commit(&mut self, next: Project, source: &Document) -> Result<()> {
        next.validate(source)?;
        if next != self.project {
            editable(source)?;
        }
        if next != self.project {
            self.undo.push(std::mem::replace(&mut self.project, next));
            self.redo.clear();
            while self.undo.len() > 32
                || self
                    .undo
                    .iter()
                    .map(|p| {
                        p.pages
                            .iter()
                            .flat_map(|p| &p.marks)
                            .map(Mark::estimated_bytes)
                            .sum::<usize>()
                    })
                    .sum::<usize>()
                    > 32 * 1024 * 1024
            {
                self.undo.remove(0);
            }
        }
        Ok(())
    }
    pub fn undo(&mut self) -> bool {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, p));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, p));
            true
        } else {
            false
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
pub fn editable(doc: &Document) -> Result<()> {
    ensure!(
        doc.metadata.doc_type != "OFD-A",
        "OFD-A 档案文档保持只读，不能增加标注、印章或调整页面"
    );
    ensure!(doc.export_allowed, "文档权限禁止编辑副本和导出");
    Ok(())
}

pub fn overlay_page(area: Rect, marks: &[Mark]) -> Result<Page> {
    let mut p = Page {
        id: "overlay".into(),
        area: Rect {
            x: 0.,
            y: 0.,
            ..area
        },
        objects: vec![],
        resources: Resources::default(),
        text: String::new(),
    };
    p.resources.fonts.insert(
        "default".into(),
        Font {
            name: crate::fonts::DEFAULT_CJK_NAME.into(),
            family: crate::fonts::DEFAULT_CJK_NAME.into(),
            bytes: Some(Arc::new(crate::fonts::DEFAULT_CJK.to_vec())),
            bold: false,
            italic: false,
        },
    );
    for (i, mark) in marks.iter().enumerate() {
        mark.validate(area)?;
        let bounds = mark.bounds();
        let mut style = Style::default();
        let mut transform = Matrix::default();
        let kind = match mark {
            Mark::Highlight { rect, color } => {
                style.fill = *color;
                Kind::Path {
                    data: rectangle(rect.w, rect.h),
                    fill: true,
                    stroke: false,
                    even_odd: false,
                }
            }
            Mark::Rectangle { rect, color, width } => {
                style.stroke = *color;
                style.width = *width;
                Kind::Path {
                    data: rectangle(rect.w, rect.h),
                    fill: false,
                    stroke: true,
                    even_odd: false,
                }
            }
            Mark::Ink {
                points,
                color,
                width,
            } => {
                style.stroke = *color;
                style.width = *width;
                style.cap = "Round".into();
                style.join = "Round".into();
                let data = points
                    .iter()
                    .enumerate()
                    .map(|(j, pt)| {
                        format!(
                            "{} {} {} ",
                            if j == 0 { "M" } else { "L" },
                            pt.x - bounds.x,
                            pt.y - bounds.y
                        )
                    })
                    .collect();
                Kind::Path {
                    data,
                    fill: false,
                    stroke: true,
                    even_odd: false,
                }
            }
            Mark::Text {
                text, size, color, ..
            } => {
                style.fill = *color;
                let codes = text
                    .lines()
                    .enumerate()
                    .map(|(j, line)| TextRun {
                        text: line.into(),
                        x: 0.,
                        y: *size * (1. + j as f32 * 1.4),
                        dx: vec![],
                        dy: vec![],
                        glyphs: vec![],
                    })
                    .collect();
                Kind::Text {
                    font: "default".into(),
                    size: *size,
                    hscale: 1.,
                    fill: true,
                    stroke: false,
                    codes,
                }
            }
            Mark::Stamp { png, .. } => {
                p.resources
                    .images
                    .insert(i.to_string(), Arc::new(png.clone()));
                transform = Matrix::scale(bounds.w, bounds.h);
                Kind::Image {
                    resource: i.to_string(),
                    interpolate: true,
                }
            }
        };
        p.objects.push(Graphic {
            id: i.to_string(),
            bounds,
            transform,
            opacity: 1.,
            style,
            clips: vec![],
            kind,
        });
    }
    Ok(p)
}
fn rectangle(w: f32, h: f32) -> String {
    format!("M 0 0 L {w} 0 L {w} {h} L 0 {h} C")
}
pub fn paint(
    renderer: &mut Renderer,
    base: &mut Rendered,
    area: Rect,
    marks: &[Mark],
    zoom: f32,
) -> Result<()> {
    if marks.is_empty() {
        return Ok(());
    }
    let overlay = renderer.render_overlay(&overlay_page(area, marks)?, zoom)?;
    base.pixmap.draw_pixmap(
        0,
        0,
        overlay.pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    base.warnings.extend(overlay.warnings);
    Ok(())
}
