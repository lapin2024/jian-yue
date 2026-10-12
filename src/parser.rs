use crate::{
    archive::{Package, resolve},
    model::*,
};
use anyhow::{Context, Result, ensure};
use roxmltree::{Document, Node};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn named(n: Node<'_, '_>, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name
}
fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|x| named(*x, name))
}
fn text(n: Node<'_, '_>, name: &str) -> String {
    child(n, name)
        .and_then(|x| x.text())
        .unwrap_or("")
        .trim()
        .into()
}
fn number(s: Option<&str>, default: f32) -> f32 {
    s.and_then(|s| s.parse::<f32>().ok())
        .filter(|n| n.is_finite())
        .unwrap_or(default)
}
fn nums(s: &str) -> Result<Vec<f32>> {
    s.split_whitespace()
        .map(|x| {
            let n = x.parse::<f32>().with_context(|| format!("无效数值 {x}"))?;
            ensure!(n.is_finite(), "非有限坐标");
            Ok(n)
        })
        .collect()
}
fn boolean(s: Option<&str>, default: bool) -> bool {
    s.map(|s| s != "false" && s != "0").unwrap_or(default)
}
fn rect(s: &str) -> Result<Rect> {
    let n = nums(s)?;
    ensure!(n.len() == 4, "Boundary/PhysicalBox 必须有 4 个数值");
    ensure!(n[2] >= 0. && n[3] >= 0., "负的矩形尺寸");
    Ok(Rect {
        x: n[0],
        y: n[1],
        w: n[2],
        h: n[3],
    })
}
fn matrix(n: Node<'_, '_>) -> Result<Matrix> {
    if let Some(s) = n.attribute("CTM") {
        let v = nums(s)?;
        ensure!(v.len() == 6, "CTM 必须有 6 个数值");
        Ok(Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]))
    } else {
        Ok(Matrix::default())
    }
}
fn parse_xml(s: &str) -> Result<Document<'_>> {
    Document::parse_with_options(
        s,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 200_000,
        },
    )
    .context("OFD XML 解析失败")
}

/// OFD delta compression: g count value repeats value count times.
pub fn deltas(s: &str) -> Result<Vec<f32>> {
    let mut it = s.split_whitespace();
    let mut out = Vec::new();
    while let Some(v) = it.next() {
        if v == "g" {
            let count: usize = it.next().context("g 缺少重复次数")?.parse()?;
            ensure!(
                count <= 100_000 && out.len() + count <= 100_000,
                "Delta 重复次数超限"
            );
            let n = nums(it.next().context("g 缺少间距")?)?[0];
            out.extend(std::iter::repeat_n(n, count));
        } else {
            let n = nums(v)?;
            out.push(n[0]);
            ensure!(out.len() <= 100_000, "Delta 数量超限");
        }
    }
    Ok(out)
}

pub fn open(path: &Path) -> Result<OfdDocument> {
    open_document(path, 0)
}
pub fn open_document(path: &Path, index: usize) -> Result<OfdDocument> {
    from_package(&Package::open(path)?, index)
}
pub fn from_bytes(bytes: &[u8]) -> Result<OfdDocument> {
    from_package(&Package::from_bytes(bytes)?, 0)
}

struct Reader<'a> {
    pkg: &'a Package,
    issues: Vec<Issue>,
    count: usize,
    checked: BTreeSet<String>,
}
impl Reader<'_> {
    fn issue(&mut self, severity: &str, clause: &str, location: &str, message: impl Into<String>) {
        let message = message.into();
        if self.issues.len() < 2000
            && !self
                .issues
                .iter()
                .any(|x| x.location == location && x.message == message)
        {
            self.issues.push(Issue {
                severity: severity.into(),
                clause: clause.into(),
                location: location.into(),
                message,
            });
        }
    }
    fn audit(&mut self, root: Node<'_, '_>, loc: &str) {
        if !self.checked.insert(loc.into()) {
            return;
        }
        for n in root.descendants().filter(Node::is_element) {
            let name = n.tag_name().name();
            let check = match name {
                "Permissions" => Some((
                    "6.2.2 a",
                    "含权限声明；归档处理建议去除，阅读器会遵守复制限制",
                )),
                "VPreferences" => Some(("6.2.2 b", "含视图首选项；归档处理建议去除")),
                "Extensions" => Some(("6.12", "含扩展信息；显示影响需在归档前固化")),
                "Encryptions" | "Encrypt" => Some(("6.16", "含加密信息；长期保存应使用明文")),
                "Versions" => Some((
                    "6.14",
                    "含历史版本；当前阅读器仅显示主文档版本，未展开版本树",
                )),
                "GouraudShd" | "LaGouraudShd" | "Pattern" => {
                    Some(("6.3", "该类网格渐变或图案填充尚不支持，当前使用纯色回退"))
                }
                "CGTransform" => None,
                "VideoObject" => Some(("6.7", "视频对象尚不支持；归档处理应保留静态外观")),
                _ => None,
            };
            if let Some((clause, msg)) = check {
                self.issue("warning", clause, loc, msg);
            }
            if name == "Action"
                && n.children()
                    .any(|c| c.is_element() && c.tag_name().name() != "Goto")
            {
                self.issue(
                    "warning",
                    "6.9",
                    loc,
                    "包含外部或非 Goto 动作；阅读器不执行该动作",
                );
            }
            if n.attribute("BlendMode").is_some_and(|s| s != "Normal") {
                self.issue(
                    "warning",
                    "6.13.3",
                    loc,
                    "非 Normal 混合模式尚不支持（包括未实现的 Darken 签章外观）",
                );
            }
            if matches!(name, "AxialShd" | "RadialShd") {
                if name == "RadialShd" && n.attribute("MapUnit").is_some() {
                    self.issue(
                        "warning",
                        "6.3",
                        loc,
                        "径向渐变 MapUnit 尚不支持，使用首段颜色回退",
                    );
                }
                if name == "RadialShd"
                    && number(n.attribute("StartRadius"), 0.).abs() > f32::EPSILON
                {
                    self.issue(
                        "warning",
                        "6.3",
                        loc,
                        "非零起始半径的径向渐变尚不支持，使用首段颜色回退",
                    );
                }
                if name == "RadialShd"
                    && (number(n.attribute("Eccentricity"), 0.).abs() > f32::EPSILON
                        || number(n.attribute("Angle"), 0.).abs() > f32::EPSILON)
                {
                    self.issue(
                        "warning",
                        "6.3",
                        loc,
                        "椭圆径向渐变尚不支持，使用首段颜色回退",
                    );
                }
                if name == "RadialShd"
                    && let (Ok(start), Ok(end)) = (
                        nums(n.attribute("StartPoint").unwrap_or("")),
                        nums(n.attribute("EndPoint").unwrap_or("")),
                    )
                    && start.len() == 2
                    && end.len() == 2
                    && (start[0] - end[0]).hypot(start[1] - end[1])
                        >= number(n.attribute("EndRadius"), 0.)
                {
                    self.issue(
                        "warning",
                        "6.3",
                        loc,
                        "焦点位于结束圆外的径向渐变尚不支持，使用首段颜色回退",
                    );
                }
                if n.parent().is_some_and(|parent| named(parent, "Color")) {
                    self.issue("warning", "6.3", loc, "渐变 Segment 只能使用基本颜色");
                }
            }
            if matches!(name, "TextObject")
                && (n.attribute("ReadDirection").is_some_and(|s| s != "0")
                    || n.attribute("CharDirection").is_some_and(|s| s != "0"))
            {
                self.issue("warning", "6.6", loc, "非水平文字方向尚不支持");
            }
        }
    }
    fn load_resources(
        &mut self,
        source: &str,
        root: Node<'_, '_>,
        res: &mut Resources,
    ) -> Result<()> {
        for reference in root
            .children()
            .filter(|n| matches!(n.tag_name().name(), "PublicRes" | "DocumentRes" | "PageRes"))
        {
            let loc = resolve(source, reference.text().unwrap_or(""))?;
            let xml = self.pkg.xml(&loc)?;
            let doc = parse_xml(&xml)?;
            let r = doc.root_element();
            self.audit(r, &loc);
            let base = r.attribute("BaseLoc").unwrap_or(".");
            let base_source = resolve(&loc, &format!("{base}/_"))?;
            for n in r.descendants().filter(|n| named(*n, "ColorSpace")) {
                let palette = child(n, "Palette")
                    .map(|p| {
                        p.children()
                            .filter(|x| named(*x, "CV"))
                            .map(|x| color_channels(x.text().unwrap_or("")))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                res.colors.insert(
                    n.attribute("ID").unwrap_or("").into(),
                    ColorSpace {
                        kind: n.attribute("Type").unwrap_or("RGB").into(),
                        bits: n
                            .attribute("BitsPerComponent")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(8),
                        palette,
                    },
                );
            }
            for n in r.descendants().filter(|n| named(*n, "Font")) {
                let embedded = text(n, "FontFile");
                let bytes = if embedded.is_empty() {
                    None
                } else {
                    let path = resolve(&base_source, &embedded)?;
                    match self.pkg.bytes(&path) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            self.issue("error", "6.2.6", &loc, e.to_string());
                            None
                        }
                    }
                };
                res.fonts.insert(
                    n.attribute("ID").unwrap_or("").into(),
                    Font {
                        name: n.attribute("FontName").unwrap_or("").into(),
                        family: n.attribute("FamilyName").unwrap_or("").into(),
                        bytes,
                        bold: boolean(n.attribute("Bold"), false),
                        italic: boolean(n.attribute("Italic"), false),
                    },
                );
            }
            for n in r.descendants().filter(|n| named(*n, "MultiMedia")) {
                if n.attribute("Type").unwrap_or("Image") != "Image" {
                    self.issue("warning", "6.2.6 g", &loc, "存在音频或视频资源，未播放");
                    continue;
                }
                let path = resolve(&base_source, &text(n, "MediaFile"))?;
                match self.pkg.bytes(&path) {
                    Ok(b) => {
                        res.images.insert(n.attribute("ID").unwrap_or("").into(), b);
                    }
                    Err(e) => self.issue("error", "6.5", &loc, e.to_string()),
                }
                if path.to_lowercase().ends_with(".jb2")
                    || path.to_lowercase().ends_with(".jbig2")
                    || path.to_lowercase().ends_with(".jp2")
                {
                    self.issue(
                        "warning",
                        "6.2.6 e",
                        &loc,
                        "JBIG2/JPEG2000 解码尚不支持；对应图像会缺失",
                    );
                }
            }
            for n in r
                .descendants()
                .filter(|n| named(*n, "CompositeGraphicUnit"))
            {
                let declarations = n
                    .namespaces()
                    .map(|ns| match ns.name() {
                        Some(prefix) => format!(" xmlns:{}=\"{}\"", prefix, ns.uri()),
                        None => format!(" xmlns=\"{}\"", ns.uri()),
                    })
                    .collect::<String>();
                res.vectors.insert(
                    n.attribute("ID").unwrap_or("").into(),
                    format!("<wrapper{declarations}>{}</wrapper>", &xml[n.range()]),
                );
            }
            let draws: BTreeMap<_, _> = r
                .descendants()
                .filter(|n| named(*n, "DrawParam"))
                .map(|n| (n.attribute("ID").unwrap_or("").to_string(), n))
                .collect();
            for id in draws.keys() {
                let s = draw_style(id, &draws, res, &mut BTreeSet::new())?;
                res.draws.insert(id.clone(), s);
            }
        }
        Ok(())
    }
    fn objects(
        &mut self,
        parent: Node<'_, '_>,
        res: &Resources,
        loc: &str,
        inherited: &Style,
        depth: usize,
    ) -> Result<Vec<Graphic>> {
        ensure!(depth <= 32, "PageBlock/复合图元嵌套超过 32 层");
        let mut out = vec![];
        for n in parent.children().filter(Node::is_element) {
            let name = n.tag_name().name();
            if matches!(
                name,
                "Actions" | "Parameters" | "PageRes" | "Area" | "Template" | "PageArea"
            ) {
                continue;
            }
            if name == "Layer" || name == "Content" {
                let style = style(n, res, inherited)?;
                out.extend(self.objects(n, res, loc, &style, depth + 1)?);
                continue;
            }
            if !matches!(
                name,
                "TextObject" | "PathObject" | "ImageObject" | "CompositeObject" | "PageBlock"
            ) {
                self.issue("warning", "7.2", loc, format!("未支持的页面节点 {name}"));
                continue;
            }
            self.count += 1;
            ensure!(self.count <= 200_000, "页面对象数量超过 200000");
            if !boolean(n.attribute("Visible"), true) {
                continue;
            }
            if depth > 3 && name == "PageBlock" {
                self.issue("warning", "6.2.3 e", loc, "页面块嵌套超过归档建议的 3 层");
            }
            let bounds = n
                .attribute("Boundary")
                .map(rect)
                .transpose()?
                .unwrap_or_default();
            let mut transform = matrix(n)?;
            let st = style(n, res, inherited)?;
            let clips = self.clips(n, loc)?;
            let kind = match name {
                "TextObject" => {
                    let size = number(n.attribute("Size"), 3.5);
                    ensure!(size > 0. && size < 10000., "文字字号越界");
                    let font = n.attribute("Font").unwrap_or("").to_string();
                    if !res.fonts.contains_key(&font) {
                        self.issue(
                            "error",
                            "6.2.6",
                            loc,
                            format!("文字引用不存在的字体 ID {font}，将使用替代字体"),
                        );
                    } else if res.fonts.get(&font).is_some_and(|f| f.bytes.is_none()) {
                        self.issue(
                            "warning",
                            "6.2.6 b",
                            loc,
                            format!(
                                "字体 {} 未嵌入；使用系统替代字体，内置字体豁免需人工核对",
                                res.fonts[&font].name
                            ),
                        );
                    }
                    let mut codes = vec![];
                    for tc in n.children().filter(|x| named(*x, "TextCode")) {
                        let t = tc.text().unwrap_or("").to_string();
                        ensure!(t.chars().count() <= 100_000, "TextCode 长度超限");
                        codes.push(TextRun {
                            text: t,
                            x: number(tc.attribute("X"), 0.),
                            y: number(tc.attribute("Y"), 0.),
                            dx: deltas(tc.attribute("DeltaX").unwrap_or(""))?,
                            dy: deltas(tc.attribute("DeltaY").unwrap_or(""))?,
                            glyphs: vec![],
                        });
                    }
                    // Apply one-to-one explicit glyph mapping. Ligature mappings are reported.
                    for cg in n.children().filter(|x| named(*x, "CGTransform")) {
                        let start = cg
                            .attribute("CodePosition")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        let count = cg
                            .attribute("CodeCount")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(1);
                        let gs = text(cg, "Glyphs")
                            .split_whitespace()
                            .map(|s| s.parse::<u16>())
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        if count == gs.len()
                            && codes.len() == 1
                            && start + count <= codes[0].text.chars().count()
                        {
                            let len = codes[0].text.chars().count();
                            if codes[0].glyphs.is_empty() {
                                codes[0].glyphs = vec![u16::MAX; len];
                            }
                            codes[0].glyphs[start..start + count].copy_from_slice(&gs);
                        } else {
                            self.issue(
                                "warning",
                                "6.6",
                                loc,
                                "非一对一或跨 TextCode 的 CGTransform 字形映射尚不支持",
                            );
                        }
                    }
                    Kind::Text {
                        font,
                        size,
                        hscale: number(n.attribute("HScale"), 1.),
                        fill: boolean(n.attribute("Fill"), true),
                        stroke: boolean(n.attribute("Stroke"), false),
                        codes,
                    }
                }
                "PathObject" => Kind::Path {
                    data: text(n, "AbbreviatedData"),
                    fill: boolean(n.attribute("Fill"), false),
                    stroke: boolean(n.attribute("Stroke"), true),
                    even_odd: n.attribute("Rule") == Some("Even-Odd"),
                },
                "ImageObject" => {
                    if n.attribute("CTM").is_none() {
                        transform = Matrix::scale(bounds.w, bounds.h)
                    }
                    if n.attribute("Substitution").is_some() {
                        self.issue("warning", "6.5", loc, "图像替代资源尚不支持");
                    }
                    if n.attribute("Mask").is_some() {
                        self.issue("warning", "6.5", loc, "图像 Mask 尚不支持");
                    }
                    Kind::Image {
                        resource: n.attribute("ResourceID").unwrap_or("").into(),
                        interpolate: boolean(n.attribute("Interpolate"), false),
                    }
                }
                "PageBlock" => Kind::Group(self.objects(n, res, loc, &st, depth + 1)?),
                "CompositeObject" => {
                    let id = n.attribute("ResourceID").unwrap_or("");
                    if let Some(xml) = res.vectors.get(id) {
                        let doc = parse_xml(xml)?;
                        let node = doc
                            .root_element()
                            .children()
                            .find(Node::is_element)
                            .context("复合图元资源为空")?;
                        let content = child(node, "Content").unwrap_or(node);
                        Kind::Group(self.objects(content, res, loc, &st, depth + 1)?)
                    } else {
                        self.issue("error", "6.8", loc, format!("找不到复合图元 {id}"));
                        Kind::Group(vec![])
                    }
                }
                _ => unreachable!(),
            };
            out.push(Graphic {
                id: n.attribute("ID").unwrap_or("").into(),
                bounds,
                transform,
                opacity: number(n.attribute("Alpha"), 255.).clamp(0., 255.) / 255.,
                style: st,
                clips,
                kind,
            });
        }
        Ok(out)
    }
    fn clips(&mut self, n: Node<'_, '_>, loc: &str) -> Result<Vec<Vec<Clip>>> {
        let mut clips = vec![];
        if let Some(cs) = child(n, "Clips") {
            for clip in cs.children().filter(|x| named(*x, "Clip")) {
                let mut areas = vec![];
                for area in clip.children().filter(|x| named(*x, "Area")) {
                    for p in area.children().filter(Node::is_element) {
                        if named(p, "Path") {
                            areas.push(Clip {
                                path: text(p, "AbbreviatedData"),
                                transform: matrix(area)?.concat(matrix(p)?),
                                even_odd: p.attribute("Rule") == Some("Even-Odd"),
                            });
                        } else {
                            self.issue(
                                "warning",
                                "6.3.2",
                                loc,
                                "文字裁剪区尚不支持；当前图元裁剪可能不完整",
                            );
                        }
                    }
                }
                if !areas.is_empty() {
                    clips.push(areas)
                }
            }
        }
        Ok(clips)
    }
    fn page_content(
        &mut self,
        path: &str,
        res: &mut Resources,
        inherited_area: Rect,
    ) -> Result<(Rect, Vec<Graphic>)> {
        let xml = self.pkg.xml(path)?;
        let d = parse_xml(&xml)?;
        let root = d.root_element();
        self.audit(root, path);
        self.load_resources(path, root, res)?;
        let area = child(root, "Area")
            .or_else(|| child(root, "PageArea"))
            .and_then(|n| child(n, "PhysicalBox"))
            .and_then(|n| n.text())
            .map(rect)
            .transpose()?
            .unwrap_or(inherited_area);
        ensure!(
            area.w > 0. && area.h > 0. && area.w <= 20000. && area.h <= 20000.,
            "无效或过大的页面尺寸"
        );
        let objects = if let Some(c) = child(root, "Content") {
            let mut layers: Vec<_> = c.children().filter(|n| named(*n, "Layer")).collect();
            layers.sort_by_key(|n| match n.attribute("Type").unwrap_or("Body") {
                "Background" => 0,
                "Foreground" => 2,
                _ => 1,
            });
            let mut out = vec![];
            for layer in layers {
                out.extend(self.objects(
                    layer,
                    res,
                    path,
                    &style(layer, res, &Style::default())?,
                    0,
                )?);
            }
            out
        } else {
            vec![]
        };
        Ok((area, objects))
    }
}

fn draw_style(
    id: &str,
    map: &BTreeMap<String, Node<'_, '_>>,
    res: &Resources,
    visited: &mut BTreeSet<String>,
) -> Result<Style> {
    ensure!(
        visited.len() < 64 && visited.insert(id.into()),
        "DrawParam 引用循环或过深: {id}"
    );
    let result = if let Some(n) = map.get(id) {
        let base = if let Some(parent) = n.attribute("Relative") {
            draw_style(parent, map, res, visited)?
        } else {
            Style::default()
        };
        style(*n, res, &base)?
    } else {
        res.draws.get(id).cloned().unwrap_or_default()
    };
    visited.remove(id);
    Ok(result)
}
fn style(n: Node<'_, '_>, res: &Resources, inherited: &Style) -> Result<Style> {
    let mut s = n
        .attribute("DrawParam")
        .and_then(|id| res.draws.get(id))
        .cloned()
        .unwrap_or_else(|| inherited.clone());
    s.width = number(n.attribute("LineWidth"), s.width).max(0.);
    if let Some(v) = n.attribute("DashPattern") {
        s.dash = nums(v)?;
    }
    s.dash_offset = number(n.attribute("DashOffset"), s.dash_offset);
    s.cap = n.attribute("Cap").unwrap_or(&s.cap).to_string();
    s.join = n.attribute("Join").unwrap_or(&s.join).to_string();
    s.miter = number(n.attribute("MiterLimit"), s.miter);
    if let Some(c) = child(n, "FillColor") {
        s.fill = fallback_color(c, res)?;
        s.fill_shader = fill_shader(c, res)?;
    }
    if let Some(c) = child(n, "StrokeColor") {
        s.stroke = fallback_color(c, res)?;
        s.stroke_shader = fill_shader(c, res)?;
    }
    Ok(s)
}
fn color(n: Node<'_, '_>, res: &Resources) -> Result<[u8; 4]> {
    color_with_space(n, res, None)
}

fn fallback_color(n: Node<'_, '_>, res: &Resources) -> Result<[u8; 4]> {
    if let Some(shader) = n
        .children()
        .find(|node| named(*node, "AxialShd") || named(*node, "RadialShd"))
        && let Some(segment) = child(shader, "Segment")
        && let Some(c) = child(segment, "Color")
    {
        let mut rgba = color_with_space(c, res, n.attribute("ColorSpace"))?;
        rgba[3] = (rgba[3] as f32 * number(n.attribute("Alpha"), 255.).clamp(0., 255.) / 255.)
            .round() as u8;
        return Ok(rgba);
    }
    color(n, res)
}

fn color_with_space(
    n: Node<'_, '_>,
    res: &Resources,
    inherited_space: Option<&str>,
) -> Result<[u8; 4]> {
    let cs = n
        .attribute("ColorSpace")
        .or(inherited_space)
        .or(res.default_cs.as_deref())
        .and_then(|id| res.colors.get(id));
    let mut values = color_channels(n.attribute("Value").unwrap_or("0 0 0"))?;
    if n.attribute("Value").is_none()
        && let Some(index) = n.attribute("Index").and_then(|v| v.parse::<usize>().ok())
        && let Some(v) = cs.and_then(|c| c.palette.get(index))
    {
        values = v.clone();
    }
    let max = ((1u32 << cs.map(|c| c.bits.clamp(1, 16)).unwrap_or(8)) - 1) as f32;
    let f: Vec<_> = values.iter().map(|v| (v / max).clamp(0., 1.)).collect();
    let kind = cs.map(|c| c.kind.as_str()).unwrap_or(if f.len() == 1 {
        "GRAY"
    } else if f.len() == 4 {
        "CMYK"
    } else {
        "RGB"
    });
    let rgb = match kind {
        "Gray" | "GRAY" => [f.first().copied().unwrap_or(0.); 3],
        "CMYK" => {
            let c = f.first().copied().unwrap_or(0.);
            let m = f.get(1).copied().unwrap_or(0.);
            let y = f.get(2).copied().unwrap_or(0.);
            let k = f.get(3).copied().unwrap_or(0.);
            [
                (1. - c) * (1. - k),
                (1. - m) * (1. - k),
                (1. - y) * (1. - k),
            ]
        }
        _ => [
            f.first().copied().unwrap_or(0.),
            f.get(1).copied().unwrap_or(0.),
            f.get(2).copied().unwrap_or(0.),
        ],
    };
    Ok([
        (rgb[0] * 255.).round() as u8,
        (rgb[1] * 255.).round() as u8,
        (rgb[2] * 255.).round() as u8,
        number(n.attribute("Alpha"), 255.).clamp(0., 255.) as u8,
    ])
}

fn color_channels(s: &str) -> Result<Vec<f32>> {
    s.split_whitespace()
        .map(|token| {
            let value = if let Some(hex) = token.strip_prefix('#') {
                ensure!(!hex.is_empty(), "空的十六进制颜色通道");
                u32::from_str_radix(hex, 16)
                    .with_context(|| format!("无效十六进制颜色通道 {token}"))?
                    as f32
            } else {
                token
                    .parse::<f32>()
                    .with_context(|| format!("无效颜色通道 {token}"))?
            };
            ensure!(value.is_finite(), "颜色通道不是有限数值");
            Ok(value)
        })
        .collect()
}

fn fill_shader(n: Node<'_, '_>, res: &Resources) -> Result<Option<FillShader>> {
    let Some(shader) = n.children().find(Node::is_element) else {
        return Ok(None);
    };
    let kind = shader.tag_name().name();
    if !matches!(kind, "AxialShd" | "RadialShd") {
        return Ok(None);
    }
    let start = nums(shader.attribute("StartPoint").unwrap_or(""))?;
    let end = nums(shader.attribute("EndPoint").unwrap_or(""))?;
    ensure!(
        start.len() == 2 && end.len() == 2,
        "渐变 StartPoint/EndPoint 必须各包含两个坐标"
    );
    let segments: Vec<_> = shader
        .children()
        .filter(|child| named(*child, "Segment"))
        .collect();
    ensure!(
        (2..=64).contains(&segments.len()),
        "渐变至少需要 2 个 Segment，最多支持 64 个"
    );
    let mut positions: Vec<Option<f32>> = segments
        .iter()
        .map(|segment| {
            segment
                .attribute("Position")
                .map(|value| value.parse::<f32>().context("无效渐变 Segment Position"))
                .transpose()
        })
        .collect::<Result<_>>()?;
    if positions[0].is_none() {
        positions[0] = Some(0.);
    }
    let last = positions.len() - 1;
    if positions[last].is_none() {
        positions[last] = Some(1.);
    }
    let mut left = 0;
    while left < last {
        let right = (left + 1..=last)
            .find(|&index| positions[index].is_some())
            .context("渐变颜色位置缺失")?;
        let a = positions[left].unwrap();
        let b = positions[right].unwrap();
        ensure!(
            a.is_finite()
                && b.is_finite()
                && (0. ..=1.).contains(&a)
                && (0. ..=1.).contains(&b)
                && a <= b,
            "渐变 Segment Position 必须在 0 到 1 之间且递增"
        );
        let span = (right - left) as f32;
        for (index, position) in positions.iter_mut().enumerate().take(right).skip(left + 1) {
            *position = Some(a + (b - a) * (index - left) as f32 / span);
        }
        left = right;
    }
    let inherited_space = n.attribute("ColorSpace");
    let alpha = number(n.attribute("Alpha"), 255.).clamp(0., 255.) / 255.;
    let stops = segments
        .iter()
        .zip(positions)
        .map(|(segment, position)| {
            let node = child(*segment, "Color").context("渐变 Segment 缺少 Color")?;
            let mut color = color_with_space(node, res, inherited_space)?;
            color[3] = (color[3] as f32 * alpha).round() as u8;
            Ok(GradientStop {
                position: position.context("渐变 Segment Position 未解析")?,
                color,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let spread = match shader.attribute("MapType").unwrap_or("Direct") {
        "Repeat" => GradientSpread::Repeat,
        "Reflect" => GradientSpread::Reflect,
        "Direct" => GradientSpread::Pad,
        other => anyhow::bail!("不支持的渐变 MapType {other}"),
    };
    let start = [start[0], start[1]];
    let end = [end[0], end[1]];
    let extend = shader
        .attribute("Extend")
        .unwrap_or("0")
        .parse::<u8>()
        .context("无效渐变 Extend")?;
    ensure!(extend <= 3, "渐变 Extend 必须在 0 到 3 之间");
    if kind == "AxialShd" {
        let length = (end[0] - start[0]).hypot(end[1] - start[1]);
        ensure!(
            length.is_finite() && length > 0.,
            "轴向渐变的轴线长度必须有限且大于 0"
        );
        let map_unit = shader
            .attribute("MapUnit")
            .map(|value| value.parse::<f32>().context("无效渐变 MapUnit"))
            .transpose()?;
        if let Some(unit) = map_unit {
            ensure!(unit.is_finite() && unit > 0., "渐变 MapUnit 必须大于 0");
        }
        Ok(Some(FillShader::Linear {
            start,
            end,
            stops,
            spread,
            map_unit,
            extend,
        }))
    } else {
        let radius = shader
            .attribute("EndRadius")
            .context("径向渐变缺少 EndRadius")?
            .parse::<f32>()
            .context("无效径向渐变 EndRadius")?;
        ensure!(
            radius.is_finite() && radius > 0.,
            "径向渐变 EndRadius 必须大于 0"
        );
        let parameter = |name: &str| -> Result<f32> {
            let value = shader
                .attribute(name)
                .unwrap_or("0")
                .parse::<f32>()
                .with_context(|| format!("无效径向渐变 {name}"))?;
            ensure!(value.is_finite(), "径向渐变 {name} 必须是有限数值");
            Ok(value)
        };
        let start_radius = parameter("StartRadius")?;
        let eccentricity = parameter("Eccentricity")?;
        let angle = parameter("Angle")?;
        ensure!(start_radius >= 0., "径向渐变 StartRadius 不能为负数");
        ensure!(
            (0. ..1.).contains(&eccentricity),
            "径向渐变 Eccentricity 必须在 0 到 1 之间（不含 1）"
        );
        if start_radius.abs() > f32::EPSILON
            || eccentricity.abs() > f32::EPSILON
            || angle.abs() > f32::EPSILON
            || shader.attribute("MapUnit").is_some()
            || (start[0] - end[0]).hypot(start[1] - end[1]) >= radius
        {
            return Ok(None);
        }
        Ok(Some(FillShader::Radial {
            start,
            end,
            radius,
            stops,
            spread,
            extend,
        }))
    }
}

fn collect_text(objects: &[Graphic], out: &mut String) {
    for o in objects {
        match &o.kind {
            Kind::Text { codes, .. } => {
                for c in codes {
                    out.push_str(&c.text)
                }
                out.push('\n')
            }
            Kind::Group(g) => collect_text(g, out),
            _ => (),
        }
    }
}

fn from_package(pkg: &Package, index: usize) -> Result<OfdDocument> {
    let xml = pkg.xml("OFD.xml")?;
    let d = parse_xml(&xml)?;
    let root = d.root_element();
    ensure!(named(root, "OFD"), "主入口根节点不是 OFD");
    let bodies: Vec<_> = root.children().filter(|n| named(*n, "DocBody")).collect();
    ensure!(index < bodies.len(), "文档索引不存在");
    let body = bodies[index];
    let mut r = Reader {
        pkg,
        issues: vec![],
        count: 0,
        checked: BTreeSet::new(),
    };
    r.audit(root, "OFD.xml");
    if bodies.len() > 1 {
        r.issue(
            "warning",
            "6.2.1 c",
            "OFD.xml",
            format!(
                "包内含 {} 个文档，归档建议拆分；当前显示第 {} 个",
                bodies.len(),
                index + 1
            ),
        );
    }
    let info = child(body, "DocInfo").unwrap_or(body);
    let meta = Metadata {
        title: text(info, "Title"),
        author: text(info, "Author"),
        creator: text(info, "Creator"),
        created: text(info, "CreationDate"),
        modified: text(info, "ModDate"),
        doc_id: text(info, "DocID"),
        doc_type: root.attribute("DocType").unwrap_or("OFD").into(),
        version: root.attribute("Version").unwrap_or("").into(),
    };
    if meta.doc_type == "OFD-A" {
        r.issue(
            "info",
            "7.2",
            "OFD.xml",
            "OFD-A 档案文档：只读显示，不提供文档编辑功能",
        );
    }
    let docloc = resolve("OFD.xml", &text(body, "DocRoot"))?;
    let xml = pkg.xml(&docloc)?;
    let doc = parse_xml(&xml)?;
    let dr = doc.root_element();
    ensure!(named(dr, "Document"), "DocRoot 未指向 Document");
    r.audit(dr, &docloc);
    let common = child(dr, "CommonData").context("Document 缺少 CommonData")?;
    let area = child(common, "PageArea")
        .and_then(|n| child(n, "PhysicalBox"))
        .and_then(|n| n.text())
        .map(rect)
        .transpose()?
        .context("没有默认 PhysicalBox")?;
    let mut res = Resources {
        default_cs: Some(text(common, "DefaultCS")).filter(|s| !s.is_empty()),
        ..Resources::default()
    };
    r.load_resources(&docloc, common, &mut res)?;
    let templates: BTreeMap<String, String> = common
        .children()
        .filter(|n| named(*n, "TemplatePage"))
        .map(|n| {
            Ok((
                n.attribute("ID").unwrap_or("").to_string(),
                resolve(&docloc, n.attribute("BaseLoc").unwrap_or(""))?,
            ))
        })
        .collect::<Result<_>>()?;
    let pn = child(dr, "Pages").context("Document 缺少 Pages")?;
    let mut pages = vec![];
    for n in pn.children().filter(|n| named(*n, "Page")) {
        ensure!(pages.len() < 2000, "页面数量超过 2000");
        let id = n.attribute("ID").unwrap_or("").to_string();
        ensure!(!pages.iter().any(|p: &Page| p.id == id), "重复的 Page ID");
        let loc = resolve(&docloc, n.attribute("BaseLoc").unwrap_or(""))?;
        let mut pr = res.clone();
        let (pa, objects) = r.page_content(&loc, &mut pr, area)?;
        let xml = pkg.xml(&loc)?;
        let pd = parse_xml(&xml)?;
        let mut back = vec![];
        let mut front = vec![];
        for t in pd
            .root_element()
            .children()
            .filter(|n| named(*n, "Template"))
        {
            if let Some(path) = templates.get(t.attribute("TemplateID").unwrap_or("")) {
                let (_, objs) = r.page_content(path, &mut pr, pa)?;
                if t.attribute("ZOrder") == Some("Foreground") {
                    front.extend(objs)
                } else {
                    back.extend(objs)
                }
            } else {
                r.issue("error", "6.2.3", &loc, "引用的模板页不存在");
            }
        }
        back.extend(objects);
        back.extend(front);
        let mut content = String::new();
        collect_text(&back, &mut content);
        pages.push(Page {
            id,
            area: pa,
            objects: back,
            resources: pr,
            text: content,
        });
    }
    ensure!(!pages.is_empty(), "文档没有页面");
    let mut bookmarks = BTreeMap::new();
    for n in dr.descendants().filter(|n| named(*n, "Bookmark")) {
        if let Some(id) = n
            .descendants()
            .find(|x| named(*x, "Dest"))
            .and_then(|d| d.attribute("PageID"))
        {
            bookmarks.insert(
                n.attribute("Name").unwrap_or("").to_string(),
                id.to_string(),
            );
        }
    }
    let mut outlines = vec![];
    for n in dr.descendants().filter(|n| named(*n, "OutlineElem")) {
        let actions = child(n, "Actions");
        let dest = actions.and_then(|a| a.descendants().find(|x| named(*x, "Dest")));
        let mut page_id = dest.and_then(|n| n.attribute("PageID")).map(str::to_owned);
        if page_id.is_none() {
            let mark = actions
                .and_then(|a| a.descendants().find(|x| named(*x, "Bookmark")))
                .and_then(|x| x.attribute("Name"))
                .unwrap_or("");
            page_id = bookmarks.get(mark).cloned();
        }
        let page = page_id.and_then(|id| pages.iter().position(|p| p.id == id));
        outlines.push(Outline {
            title: n.attribute("Title").unwrap_or("未命名大纲").into(),
            page,
            depth: n
                .ancestors()
                .filter(|a| named(*a, "OutlineElem"))
                .count()
                .saturating_sub(1),
        });
    }
    // Static annotation appearances are composed on the page without altering the source.
    if let Some(reference) = child(dr, "Annotations").and_then(|n| n.text()) {
        let loc = resolve(&docloc, reference)?;
        let xml = pkg.xml(&loc)?;
        let ad = parse_xml(&xml)?;
        for ap in ad.descendants().filter(|n| named(*n, "Page")) {
            let Some(p) = pages
                .iter_mut()
                .find(|p| Some(p.id.as_str()) == ap.attribute("PageID"))
            else {
                continue;
            };
            let name = text(ap, "FileLoc");
            if name.is_empty() {
                continue;
            }
            let aloc = resolve(&loc, &name)?;
            let ax = pkg.xml(&aloc)?;
            let ann = parse_xml(&ax)?;
            r.audit(ann.root_element(), &aloc);
            for n in ann.descendants().filter(|n| named(*n, "Annot")) {
                if !boolean(n.attribute("Visible"), true) {
                    continue;
                }
                if let Some(a) = child(n, "Appearance") {
                    let bounds = a
                        .attribute("Boundary")
                        .map(rect)
                        .transpose()?
                        .unwrap_or_default();
                    let objects = r.objects(a, &p.resources, &aloc, &Style::default(), 0)?;
                    p.objects.push(Graphic {
                        id: n.attribute("ID").unwrap_or("").into(),
                        bounds,
                        transform: Matrix::default(),
                        opacity: 1.,
                        style: Style::default(),
                        clips: vec![],
                        kind: Kind::Group(objects),
                    });
                }
            }
        }
    }
    let mut attachments = vec![];
    if let Some(reference) = child(dr, "Attachments").and_then(|n| n.text()) {
        let loc = resolve(&docloc, reference)?;
        let xml = pkg.xml(&loc)?;
        let ad = parse_xml(&xml)?;
        for n in ad.descendants().filter(|n| named(*n, "Attachment")) {
            let path = resolve(&loc, &text(n, "FileLoc"))?;
            match pkg.bytes(&path) {
                Ok(bytes) => {
                    let format = n.attribute("Format").unwrap_or("").to_owned();
                    if !matches!(format.to_lowercase().as_str(), "txt" | "xml") {
                        r.issue("warning", "6.15", &loc, "附件的长期保存适用性需人工核对");
                    }
                    attachments.push(Attachment {
                        name: n.attribute("Name").unwrap_or("附件").into(),
                        path,
                        format,
                        size: bytes.len(),
                        bytes,
                    });
                }
                Err(e) => r.issue("error", "6.15", &loc, e.to_string()),
            }
        }
    }
    let mut signature_count = 0;
    if let Some(reference) = child(body, "Signatures").and_then(|n| n.text()) {
        let loc = resolve("OFD.xml", reference)?;
        match pkg.xml(&loc) {
            Ok(xml) => {
                let sd = parse_xml(&xml)?;
                signature_count = sd.descendants().filter(|n| named(*n, "Signature")).count();
            }
            Err(e) => r.issue("error", "6.13", &loc, e.to_string()),
        }
        r.issue(
            "warning",
            "6.13",
            &loc,
            "数字签名未验证；未实现 SM2/SM3、证书链与撤销状态校验，也未呈现签章专用二进制外观",
        );
    }
    if pkg.paths().any(|p| p.to_lowercase().contains("encrypt")) {
        r.issue(
            "warning",
            "6.16",
            "OFD.xml",
            "包内存在加密描述文件；不支持解密",
        );
    }
    let perm = child(dr, "Permissions");
    let copy_allowed = perm
        .map(|p| boolean(child(p, "Copy").and_then(|n| n.text()), true))
        .unwrap_or(true);
    let export_allowed = perm
        .map(|p| boolean(child(p, "Export").and_then(|n| n.text()), true))
        .unwrap_or(true);
    if !outlines.is_empty() && outlines.iter().any(|o| o.page.is_none()) {
        r.issue(
            "warning",
            "6.2.5",
            "Document.xml",
            "部分大纲目标未解析，可能是无效页码或未支持的目标类型",
        );
    }
    Ok(OfdDocument {
        metadata: meta,
        pages,
        outlines,
        attachments,
        issues: r.issues,
        document_count: bodies.len(),
        signature_count,
        copy_allowed,
        export_allowed,
        print_allowed: perm
            .and_then(|p| child(p, "Print"))
            .map(|p| boolean(p.attribute("Printable"), true))
            .unwrap_or(true),
    })
}
