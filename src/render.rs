use crate::model::*;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeSet, HashMap},
    io::Cursor,
    sync::Arc,
};
use tiny_skia::{FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

pub struct Rendered {
    pub pixmap: Pixmap,
    pub warnings: Vec<String>,
}
#[derive(Clone)]
struct FontHandle {
    bytes: Arc<Vec<u8>>,
    index: u32,
}
pub struct Renderer {
    db: fontdb::Database,
    fonts: HashMap<fontdb::ID, FontHandle>,
    choices: HashMap<(String, char), Option<fontdb::ID>>,
}
impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
fn ts(m: Matrix) -> Transform {
    let [a, b, c, d, e, f] = m.0;
    Transform::from_row(a, b, c, d, e, f)
}
fn rule(even: bool) -> FillRule {
    if even {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    }
}

/// Converts OFD S/M/L/Q/B/A/C commands to SVG, whose arc implementation
/// converts elliptical arcs to cubic curves. Scientific-notation E is retained.
pub fn ofd_path(data: &str) -> Result<Option<Path>> {
    ensure!(data.len() <= 4 * 1024 * 1024, "路径数据超限");
    let svg: String = data
        .chars()
        .map(|c| match c {
            'S' => 'M',
            'B' => 'C',
            'C' => 'Z',
            _ => c,
        })
        .collect();
    let mut b = PathBuilder::new();
    let mut count = 0;
    for segment in svgtypes::SimplifyingPathParser::from(svg.as_str()) {
        count += 1;
        ensure!(count <= 100_000, "路径段数量超限");
        use svgtypes::SimplePathSegment::*;
        match segment.context("无效 OFD AbbreviatedData")? {
            MoveTo { x, y } => b.move_to(x as f32, y as f32),
            LineTo { x, y } => b.line_to(x as f32, y as f32),
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => b.cubic_to(
                x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
            ),
            Quadratic { x1, y1, x, y } => b.quad_to(x1 as f32, y1 as f32, x as f32, y as f32),
            ClosePath => b.close(),
        }
    }
    Ok(b.finish())
}
struct OutlineBuilder(PathBuilder);
impl ttf_parser::OutlineBuilder for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y)
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y)
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y)
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y)
    }
    fn close(&mut self) {
        self.0.close()
    }
}
impl Renderer {
    pub fn new() -> Self {
        Self::with_system_fonts(std::env::var_os("OFD_READER_NO_SYSTEM_FONTS").is_none())
    }
    pub fn with_system_fonts(system_fonts: bool) -> Self {
        let mut db = fontdb::Database::new();
        if system_fonts {
            db.load_system_fonts();
        }
        db.load_font_data(crate::fonts::DEFAULT_CJK.to_vec());
        Self {
            db,
            fonts: HashMap::new(),
            choices: HashMap::new(),
        }
    }
    pub fn ui_font(&mut self) -> Option<(Vec<u8>, u32)> {
        Some((crate::fonts::DEFAULT_CJK.to_vec(), 0))
    }
    fn system_font(&mut self, id: fontdb::ID) -> Option<FontHandle> {
        if let Some(h) = self.fonts.get(&id) {
            return Some(h.clone());
        }
        let h = self.db.with_face_data(id, |bytes, index| FontHandle {
            bytes: Arc::new(bytes.to_vec()),
            index,
        })?;
        self.fonts.insert(id, h.clone());
        Some(h)
    }
    fn font_for(&mut self, font: Option<&Font>, ch: char) -> Option<FontHandle> {
        if let Some(f) = font
            && let Some(bytes) = &f.bytes
            && ttf_parser::Face::parse(bytes, 0).is_ok_and(|face| face.glyph_index(ch).is_some())
        {
            return Some(FontHandle {
                bytes: bytes.clone(),
                index: 0,
            });
        }
        let name = font
            .map(|f| {
                if f.name.is_empty() {
                    f.family.as_str()
                } else {
                    f.name.as_str()
                }
            })
            .unwrap_or("sans-serif");
        let key = (
            format!(
                "{}:{}:{}",
                name,
                font.is_some_and(|f| f.bold),
                font.is_some_and(|f| f.italic)
            ),
            ch,
        );
        if let Some(id) = self.choices.get(&key).copied() {
            return id.and_then(|id| self.system_font(id));
        }
        let weight = if font.is_some_and(|f| f.bold) {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        };
        let style = if font.is_some_and(|f| f.italic) {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };
        let candidates = [
            self.db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                weight,
                style,
                ..Default::default()
            }),
            self.db.query(&fontdb::Query {
                families: &[
                    fontdb::Family::Name(crate::fonts::DEFAULT_CJK_NAME),
                    fontdb::Family::Name("Microsoft YaHei"),
                    fontdb::Family::Name("Noto Sans CJK SC"),
                    fontdb::Family::SansSerif,
                ],
                weight,
                style,
                ..Default::default()
            }),
        ];
        let mut chosen = None;
        for id in candidates.into_iter().flatten() {
            if self
                .db
                .with_face_data(id, |b, i| {
                    ttf_parser::Face::parse(b, i).is_ok_and(|f| f.glyph_index(ch).is_some())
                })
                .unwrap_or(false)
            {
                chosen = Some(id);
                break;
            }
        }
        if chosen.is_none() {
            chosen = self
                .db
                .faces()
                .find(|info| {
                    self.db
                        .with_face_data(info.id, |b, i| {
                            ttf_parser::Face::parse(b, i).is_ok_and(|f| f.glyph_index(ch).is_some())
                        })
                        .unwrap_or(false)
                })
                .map(|i| i.id);
        }
        self.choices.insert(key, chosen);
        chosen.and_then(|id| self.system_font(id))
    }
    pub fn render(&mut self, page: &Page, zoom: f32, rotation: u32) -> Result<Rendered> {
        self.render_inner(page, zoom, rotation, false)
    }
    pub fn render_overlay(&mut self, page: &Page, zoom: f32) -> Result<Rendered> {
        self.render_inner(page, zoom, 0, true)
    }
    fn render_inner(
        &mut self,
        page: &Page,
        zoom: f32,
        rotation: u32,
        transparent: bool,
    ) -> Result<Rendered> {
        ensure!(zoom.is_finite() && zoom > 0. && zoom <= 16., "无效缩放比例");
        let scale = 96. / 25.4 * zoom;
        let (w, h) = if rotation % 180 == 90 {
            (page.area.h, page.area.w)
        } else {
            (page.area.w, page.area.h)
        };
        let width = (w * scale).ceil() as u32;
        let height = (h * scale).ceil() as u32;
        ensure!(
            width > 0
                && height > 0
                && width <= 16384
                && height <= 16384
                && (width as u64 * height as u64) <= 16_000_000,
            "渲染尺寸超过 1600 万像素限制，请降低缩放比例"
        );
        let mut pixmap = Pixmap::new(width, height).context("无法分配页面图像")?;
        if !transparent {
            pixmap.fill(tiny_skia::Color::WHITE);
        }
        let rot = match rotation % 360 {
            90 => Matrix([0., 1., -1., 0., page.area.h, 0.]),
            180 => Matrix([-1., 0., 0., -1., page.area.w, page.area.h]),
            270 => Matrix([0., -1., 1., 0., 0., page.area.w]),
            _ => Matrix::default(),
        };
        let transform = Matrix::scale(scale, scale)
            .concat(rot)
            .concat(Matrix::translate(-page.area.x, -page.area.y));
        let mut warnings = BTreeSet::new();
        let mut images = HashMap::new();
        self.draw_objects(
            &mut pixmap,
            &page.objects,
            &page.resources,
            transform,
            1.,
            None,
            &mut warnings,
            &mut images,
        )?;
        Ok(Rendered {
            pixmap,
            warnings: warnings.into_iter().collect(),
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn draw_objects(
        &mut self,
        pixmap: &mut Pixmap,
        objects: &[Graphic],
        res: &Resources,
        parent: Matrix,
        alpha: f32,
        parent_mask: Option<&Mask>,
        warnings: &mut BTreeSet<String>,
        images: &mut HashMap<String, Pixmap>,
    ) -> Result<()> {
        for o in objects {
            let anchor = parent.concat(Matrix::translate(o.bounds.x, o.bounds.y));
            let transform = anchor.concat(o.transform);
            let opacity = alpha * o.opacity;
            let mut mask = parent_mask.cloned();
            if o.bounds.w > 0. && o.bounds.h > 0. {
                let mut next =
                    Mask::new(pixmap.width(), pixmap.height()).context("裁剪内存不足")?;
                if let Some(r) = tiny_skia::Rect::from_xywh(0., 0., o.bounds.w, o.bounds.h) {
                    next.fill_path(
                        &PathBuilder::from_rect(r),
                        FillRule::Winding,
                        true,
                        ts(anchor),
                    );
                }
                intersect_mask(&mut next, mask.as_ref());
                mask = Some(next);
            }
            for areas in &o.clips {
                let mut next =
                    Mask::new(pixmap.width(), pixmap.height()).context("裁剪内存不足")?;
                for a in areas {
                    match ofd_path(&a.path) {
                        Ok(Some(p)) => next.fill_path(
                            &p,
                            rule(a.even_odd),
                            true,
                            ts(transform.concat(a.transform)),
                        ),
                        Ok(None) => (),
                        Err(e) => {
                            warnings.insert(e.to_string());
                        }
                    }
                }
                intersect_mask(&mut next, mask.as_ref());
                mask = Some(next);
            }
            match &o.kind {
                Kind::Path {
                    data,
                    fill,
                    stroke,
                    even_odd,
                } => match ofd_path(data) {
                    Ok(Some(path)) => draw_path(
                        pixmap,
                        &path,
                        &o.style,
                        *fill,
                        *stroke,
                        *even_odd,
                        transform,
                        opacity,
                        mask.as_ref(),
                    ),
                    Ok(None) => (),
                    Err(e) => {
                        warnings.insert(format!("路径 {}: {e}", o.id));
                    }
                },
                Kind::Text {
                    font,
                    size,
                    hscale,
                    fill,
                    stroke,
                    codes,
                } => {
                    let f = res.fonts.get(font);
                    if let Some(f) = f
                        && f.bytes
                            .as_ref()
                            .is_some_and(|b| ttf_parser::Face::parse(b, 0).is_err())
                    {
                        warnings.insert(format!("嵌入字体 {} 无法解析，使用系统替代字体", f.name));
                    }
                    for run in codes {
                        let mut x = run.x;
                        let mut y = run.y;
                        for (i, ch) in run.text.chars().enumerate() {
                            if let Some(f) = f
                                && f.bytes.as_ref().is_some_and(|b| {
                                    ttf_parser::Face::parse(b, 0)
                                        .is_ok_and(|face| face.glyph_index(ch).is_none())
                                })
                                && run.glyphs.get(i).is_none_or(|id| *id == u16::MAX)
                            {
                                warnings.insert(format!(
                                    "嵌入字体 {} 缺少 U+{:04X}，使用系统替代字体",
                                    f.name, ch as u32
                                ));
                            }
                            let explicit = run.glyphs.get(i).copied().filter(|id| *id != u16::MAX);
                            let h = if explicit.is_some() {
                                f.and_then(|f| f.bytes.as_ref())
                                    .map(|bytes| FontHandle {
                                        bytes: bytes.clone(),
                                        index: 0,
                                    })
                                    .or_else(|| self.font_for(f, ch))
                            } else {
                                self.font_for(f, ch)
                            };
                            let mut advance = *size * *hscale;
                            if let Some(h) = h {
                                if let Ok(face) = ttf_parser::Face::parse(&h.bytes, h.index) {
                                    let glyph = explicit
                                        .filter(|_| f.is_some_and(|f| f.bytes.is_some()))
                                        .map(ttf_parser::GlyphId)
                                        .or_else(|| face.glyph_index(ch));
                                    if let Some(glyph) = glyph {
                                        let unit = *size / face.units_per_em() as f32;
                                        advance = face
                                            .glyph_hor_advance(glyph)
                                            .unwrap_or(face.units_per_em())
                                            as f32
                                            * unit
                                            * *hscale;
                                        let mut builder = OutlineBuilder(PathBuilder::new());
                                        face.outline_glyph(glyph, &mut builder);
                                        if let Some(path) = builder.0.finish().and_then(|p| {
                                            p.transform(ts(Matrix::translate(x, y)
                                                .concat(Matrix::scale(unit * *hscale, -unit))))
                                        }) {
                                            draw_path(
                                                pixmap,
                                                &path,
                                                &o.style,
                                                *fill,
                                                *stroke,
                                                false,
                                                transform,
                                                opacity,
                                                mask.as_ref(),
                                            );
                                        }
                                    } else {
                                        warnings
                                            .insert(format!("无法找到字形 U+{:04X}", ch as u32));
                                    }
                                } else {
                                    warnings.insert(format!("字体 {font} 数据无效"));
                                }
                            } else {
                                warnings.insert(format!(
                                    "缺少字符 U+{:04X} 的字体，请安装中文字体",
                                    ch as u32
                                ));
                            }
                            x += run.dx.get(i).copied().unwrap_or(advance);
                            y += run.dy.get(i).copied().unwrap_or(0.);
                        }
                    }
                }
                Kind::Image {
                    resource,
                    interpolate,
                } => {
                    if !images.contains_key(resource) {
                        if let Some(bytes) = res.images.get(resource) {
                            match decode_image(bytes) {
                                Ok(image) => {
                                    images.insert(resource.clone(), image);
                                }
                                Err(e) => {
                                    warnings.insert(format!("图像 {resource} 未渲染: {e}"));
                                }
                            }
                        } else {
                            warnings.insert(format!("缺少图像资源 {resource}"));
                        }
                    }
                    if let Some(image) = images.get(resource) {
                        let paint = tiny_skia::PixmapPaint {
                            opacity,
                            quality: if *interpolate {
                                tiny_skia::FilterQuality::Bilinear
                            } else {
                                tiny_skia::FilterQuality::Nearest
                            },
                            ..Default::default()
                        };
                        let im = transform.concat(Matrix::scale(
                            1. / image.width() as f32,
                            1. / image.height() as f32,
                        ));
                        pixmap.draw_pixmap(0, 0, image.as_ref(), &paint, ts(im), mask.as_ref());
                    }
                }
                Kind::Group(g) => {
                    if o.opacity < 1. {
                        warnings.insert(
                            "复合图元透明度采用逐图元近似，重叠区域可能与离屏合成不同".into(),
                        );
                    }
                    self.draw_objects(
                        pixmap,
                        g,
                        res,
                        transform,
                        opacity,
                        mask.as_ref(),
                        warnings,
                        images,
                    )?;
                }
            }
        }
        Ok(())
    }
}
fn intersect_mask(next: &mut Mask, previous: Option<&Mask>) {
    if let Some(p) = previous {
        for (a, b) in next.data_mut().iter_mut().zip(p.data()) {
            *a = ((*a as u16 * *b as u16 + 127) / 255) as u8;
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn draw_path(
    pixmap: &mut Pixmap,
    path: &Path,
    style: &Style,
    fill: bool,
    stroke: bool,
    even_odd: bool,
    transform: Matrix,
    opacity: f32,
    mask: Option<&Mask>,
) {
    let paint = |color: [u8; 4]| {
        let mut p = Paint::default();
        p.set_color_rgba8(
            color[0],
            color[1],
            color[2],
            (color[3] as f32 * opacity).round() as u8,
        );
        p.anti_alias = true;
        p
    };
    if fill {
        pixmap.fill_path(
            path,
            &paint(style.fill),
            rule(even_odd),
            ts(transform),
            mask,
        )
    }
    if stroke {
        let cap = match style.cap.as_str() {
            "Round" => tiny_skia::LineCap::Round,
            "Square" => tiny_skia::LineCap::Square,
            _ => tiny_skia::LineCap::Butt,
        };
        let join = match style.join.as_str() {
            "Round" => tiny_skia::LineJoin::Round,
            "Bevel" => tiny_skia::LineJoin::Bevel,
            _ => tiny_skia::LineJoin::Miter,
        };
        let st = Stroke {
            width: style.width.max(0.01),
            line_cap: cap,
            line_join: join,
            miter_limit: style.miter.max(1.),
            dash: tiny_skia::StrokeDash::new(style.dash.clone(), style.dash_offset),
        };
        pixmap.stroke_path(path, &paint(style.stroke), &st, ts(transform), mask);
    }
}
fn decode_image(bytes: &[u8]) -> Result<Pixmap> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().context("格式不支持或图像损坏")?.to_rgba8();
    ensure!(
        image.width() as u64 * image.height() as u64 <= 16_000_000,
        "图像像素数过大"
    );
    let mut pixmap = Pixmap::new(image.width(), image.height()).context("图像内存不足")?;
    for (src, dst) in image
        .as_raw()
        .chunks_exact(4)
        .zip(pixmap.data_mut().chunks_exact_mut(4))
    {
        let alpha = src[3] as u16;
        for i in 0..3 {
            dst[i] = ((src[i] as u16 * alpha + 127) / 255) as u8;
        }
        dst[3] = src[3];
    }
    Ok(pixmap)
}
