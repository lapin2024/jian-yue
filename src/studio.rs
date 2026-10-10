use super::*;
use anyhow::{Context, Result, ensure};
use ofd_reader::{
    conversion::{self, Format},
    editing::{self, Editor, Mark, Point, Project},
    model::Rect,
    signing,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum WorkTab {
    Read,
    Annotate,
    Convert,
    Sign,
    Edit,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tool {
    Select,
    Highlight,
    Rectangle,
    Ink,
    Text,
    Stamp,
}
pub(super) struct Studio {
    pub tab: WorkTab,
    pub startup_project: Option<PathBuf>,
    drop_choices: Vec<PathBuf>,
    pub tool: Tool,
    source: Option<Arc<Document>>,
    editor: Option<Editor>,
    staged: Option<Project>,
    selected: Option<(usize, usize)>,
    drag: Option<(usize, Point, Vec<Point>)>,
    text: String,
    size: f32,
    color: [u8; 4],
    width: f32,
    stamp: Option<(Vec<u8>, String)>,
    list_open: bool,
    convert_open: bool,
    format: Format,
    range: String,
    dpi: u32,
    sign_open: bool,
    certificate: Option<PathBuf>,
    password: String,
    reason: String,
    native: bool,
    report: Option<String>,
    default_ofd: bool,
    default_pdf: bool,
    pending_open: Option<PathBuf>,
    icon: Option<TextureHandle>,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            tab: WorkTab::Read,
            startup_project: None,
            drop_choices: vec![],
            tool: Tool::Select,
            source: None,
            editor: None,
            staged: None,
            selected: None,
            drag: None,
            text: "文字批注".into(),
            size: 4.,
            color: [208, 49, 49, 255],
            width: 0.6,
            stamp: None,
            list_open: false,
            convert_open: false,
            format: Format::Pdf,
            range: String::new(),
            dpi: 150,
            sign_open: false,
            certificate: None,
            password: String::new(),
            reason: "Document approval".into(),
            native: true,
            report: None,
            default_ofd: true,
            default_pdf: true,
            pending_open: None,
            icon: None,
        }
    }
}
impl Studio {
    pub fn reset(&mut self) {
        self.source = None;
        self.editor = None;
        self.staged = None;
        self.selected = None;
        self.drag = None;
        self.sign_open = false;
        use zeroize::Zeroize;
        self.password.zeroize();
    }
    pub fn attach(&mut self, doc: Arc<Document>) {
        self.editor = Some(Editor::new(Project::new(&doc, doc.source_hash.clone())));
        self.source = Some(doc);
    }
    fn editable(&self) -> Result<()> {
        editing::editable(self.source.as_ref().context("请先打开文档")?)
    }
    fn unchanged(&self) -> bool {
        match (&self.source, &self.editor) {
            (Some(source), Some(editor)) => {
                editor.project == Project::new(source, source.source_hash.clone())
            }
            _ => false,
        }
    }
    fn stage(&mut self, change: impl FnOnce(&mut Project)) {
        if let Some(editor) = &self.editor {
            let mut next = self
                .staged
                .clone()
                .unwrap_or_else(|| editor.project.clone());
            change(&mut next);
            self.staged = Some(next);
        }
    }
}
pub(super) enum Job {
    Convert {
        doc: Arc<Document>,
        source: PathBuf,
        target: PathBuf,
        options: conversion::Options,
    },
    Sign {
        source: PathBuf,
        target: PathBuf,
        certificate: PathBuf,
        password: Zeroizing<String>,
        reason: String,
        native: bool,
        hash: String,
    },
    Verify {
        source: PathBuf,
        signature: Option<PathBuf>,
    },
    LoadProject {
        path: PathBuf,
        source: Arc<Document>,
    },
    Defaults {
        ofd: bool,
        pdf: bool,
    },
}
pub(super) enum Outcome {
    Done(Result<String, String>),
    Report(Result<String, String>),
    Project(Result<Project, String>),
}
pub(super) fn execute(job: Job) -> Outcome {
    match job {
        Job::Convert {
            doc,
            source,
            target,
            options,
        } => Outcome::Done(
            (|| {
                let bytes = conversion::convert(&doc, &options)?;
                ofd_reader::export::write(&source, &target, &bytes)?;
                Ok(format!(
                    "已保存 {} 页的新文档副本：{}",
                    options.pages.len(),
                    target.display()
                ))
            })()
            .map_err(|e: anyhow::Error| format!("{e:#}")),
        ),
        Job::Sign {
            source,
            target,
            certificate,
            password,
            reason,
            native,
            hash,
        } => Outcome::Done(
            (|| {
                ofd_reader::export::ensure_distinct(&source, &target)?;
                ofd_reader::export::ensure_distinct(&certificate, &target)?;
                ensure!(
                    std::fs::metadata(&source)?.len() <= 128 * 1024 * 1024,
                    "签名文件超过 128 MiB"
                );
                ensure!(
                    std::fs::metadata(&certificate)?.len() <= 8 * 1024 * 1024,
                    "证书文件过大"
                );
                let bytes = std::fs::read(&source)?;
                use sha2::Digest;
                ensure!(
                    hex::encode(sha2::Sha256::digest(&bytes)) == hash,
                    "原件在打开后已变更，请重新打开后再签名"
                );
                let identity =
                    signing::Identity::from_pkcs12(&std::fs::read(&certificate)?, &password)?;
                let signed = if native {
                    signing::sign_pdf(&bytes, &identity, &reason)?
                } else {
                    identity.detached(&bytes)?
                };
                ofd_reader::export::write(&source, &target, &signed)?;
                Ok(format!(
                    "证书签名已保存：{}。签名者：{}；CA 信任链尚未验证。",
                    target.display(),
                    identity.subject()
                ))
            })()
            .map_err(|e: anyhow::Error| format!("{e:#}")),
        ),
        Job::Verify { source, signature } => Outcome::Report(
            (|| {
                ensure!(
                    std::fs::metadata(&source)?.len() <= 128 * 1024 * 1024,
                    "验签文件过大"
                );
                let verified_path = source.display().to_string();
                let data = std::fs::read(&source)?;
                let reports = if let Some(path) = signature {
                    ensure!(
                        std::fs::metadata(&path)?.len() <= 1024 * 1024,
                        "签名文件过大"
                    );
                    vec![signing::verify_detached(&data, &std::fs::read(path)?)?]
                } else {
                    signing::verify_pdf(&data)?
                };
                Ok(format!(
                    "已验证文件：{verified_path}\n{}",
                    serde_json::to_string_pretty(&reports)?
                ))
            })()
            .map_err(|e: anyhow::Error| format!("{e:#}")),
        ),
        Job::LoadProject { path, source } => Outcome::Project(
            Project::load(&path, &source, &source.source_hash).map_err(|e| format!("{e:#}")),
        ),
        Job::Defaults { ofd, pdf } => Outcome::Done(
            ofd_reader::associations::set_defaults(ofd, pdf).map_err(|e| format!("{e:#}")),
        ),
    }
}
pub(super) fn window_icon() -> egui::IconData {
    let im = image::load_from_memory(include_bytes!("../assets/icons/icon-256.png"))
        .expect("bundled icon")
        .to_rgba8();
    egui::IconData {
        width: im.width(),
        height: im.height(),
        rgba: im.into_raw(),
    }
}
fn button(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    ui.add(
        egui::Button::new(label)
            .fill(if selected {
                Color32::from_rgb(221, 240, 254)
            } else {
                Color32::from_rgb(244, 247, 250)
            })
            .min_size(Vec2::new(70., 34.)),
    )
    .clicked()
}
impl ReaderApp {
    pub(super) fn icon_header(&mut self, ui: &mut egui::Ui) {
        let t = self.studio.icon.get_or_insert_with(|| {
            let im = image::load_from_memory(include_bytes!("../assets/icons/icon-48.png"))
                .unwrap()
                .to_rgba8();
            ui.ctx().load_texture(
                "application-icon",
                egui::ColorImage::from_rgba_unmultiplied([48, 48], im.as_raw()),
                TextureOptions::LINEAR,
            )
        });
        ui.add(egui::Image::from_texture(&*t).fit_to_exact_size(Vec2::splat(28.)));
    }
    pub(super) fn dispatch(&mut self, job: Job, message: &str) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = message.into();
        let _ = self.tx.send(Command::Studio(job));
    }
    pub(super) fn accept_studio(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Done(result) => match result {
                Ok(s) => {
                    self.status = s.clone();
                    self.studio.report = Some(s);
                }
                Err(e) => self.error = Some(e),
            },
            Outcome::Report(result) => match result {
                Ok(s) => self.studio.report = Some(s),
                Err(e) => self.error = Some(e),
            },
            Outcome::Project(result) => match result {
                Ok(p) => {
                    if self
                        .studio
                        .source
                        .as_ref()
                        .is_none_or(|d| d.source_hash != p.source_sha256)
                    {
                        self.error = Some("当前文件已切换，未载入编辑项目".into());
                        return;
                    }
                    if let Err(e) = self.studio.editable() {
                        self.error = Some(e.to_string());
                        return;
                    }
                    self.studio.editor = Some(Editor::new(p));
                    self.rebuild_work();
                    self.status = "已载入可编辑项目".into();
                }
                Err(e) => self.error = Some(e),
            },
        }
    }
    fn rebuild_work(&mut self) {
        if let (Some(source), Some(editor)) = (&self.studio.source, &self.studio.editor) {
            match Document::edited(source.clone(), editor.project.clone()) {
                Ok(doc) => {
                    self.doc = Some(Arc::new(doc));
                    self.generation += 1;
                    self.live_generation
                        .store(self.generation, std::sync::atomic::Ordering::Relaxed);
                    self.cache.clear();
                    self.pending.clear();
                    self.thumbnails.clear();
                    self.thumb_pending.clear();
                    self.page = self.page.min(self.doc.as_ref().unwrap().pages.len() - 1);
                    self.page_input = self.page + 1;
                    self.studio.selected = None;
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
    }
    pub(super) fn apply_staged(&mut self) {
        if let Some(next) = self.studio.staged.take() {
            let Some(source) = self.studio.source.clone() else {
                return;
            };
            if let Some(editor) = &mut self.studio.editor {
                match editor.commit(next, &source) {
                    Ok(()) => {
                        self.rebuild_work();
                        self.status = "编辑已应用到工作副本；原文件未修改".into();
                    }
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
            }
        }
    }
    fn save_project(&mut self) {
        let Some(editor) = &self.studio.editor else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Reader 编辑项目", &["json"])
            .set_file_name("document.reader.json")
            .save_file()
        {
            let result = serde_json::to_vec_pretty(&editor.project)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| {
                    ensure!(bytes.len() <= 32 * 1024 * 1024, "编辑项目超过 32 MiB");
                    ofd_reader::export::write(self.path.as_ref().unwrap(), &path, &bytes)
                });
            if result.is_ok()
                && let Some(editor) = &mut self.studio.editor
            {
                editor.saved();
            }
            self.save_result(result, "编辑项目已保存；需和原文档一同保留");
        }
    }
    fn load_project(&mut self) {
        let Some(source) = self.studio.source.clone() else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Reader 编辑项目", &["json"])
            .pick_file()
        {
            self.dispatch(Job::LoadProject { path, source }, "正在载入编辑项目…");
        }
    }
    fn tool(&mut self, tool: Tool) {
        if let Err(e) = self.studio.editable() {
            self.error = Some(e.to_string());
            return;
        }
        self.studio.tool = tool;
        self.status = match tool {
            Tool::Select => "点击选择新增标注，拖动可移动",
            Tool::Highlight => "在页面上拖动绘制高亮区域",
            Tool::Rectangle => "在页面上拖动绘制矩形",
            Tool::Ink => "按住鼠标手绘，松开完成",
            Tool::Text => "点击页面放置文字，可在工具栏修改内容",
            Tool::Stamp => "点击页面放置印章；这是外观贴图，不是数字签名",
        }
        .into();
    }
    fn pick_stamp(&mut self) {
        if let Err(e) = self.studio.editable() {
            self.error = Some(e.to_string());
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("印章图片", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            let result = (|| -> Result<Vec<u8>> {
                ensure!(
                    std::fs::metadata(&path)?.len() <= 8 * 1024 * 1024,
                    "图片超过 8 MiB"
                );
                let mut reader = image::ImageReader::open(&path)?.with_guessed_format()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                let im = reader.decode()?;
                let mut buffer = std::io::Cursor::new(vec![]);
                im.write_to(&mut buffer, image::ImageFormat::Png)?;
                Ok(buffer.into_inner())
            })();
            match result {
                Ok(bytes) => {
                    self.studio.stamp = Some((
                        bytes,
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    ));
                    self.tool(Tool::Stamp)
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
    }
    pub(super) fn studio_toolbar(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let tab = self.studio.tab;
        egui::ScrollArea::horizontal()
            .id_salt("studio-tools")
            .show(ui, |ui| {
                ui.horizontal(|ui| match tab {
                    WorkTab::Annotate | WorkTab::Edit => {
                        for (tool, label) in [
                            (Tool::Select, "选择 / 移动"),
                            (Tool::Text, "添加文字"),
                            (Tool::Highlight, "高亮"),
                            (Tool::Rectangle, "矩形"),
                            (Tool::Ink, "手绘"),
                        ] {
                            if button(ui, label, self.studio.tool == tool) {
                                self.tool(tool)
                            }
                        }
                        if button(ui, "标注列表", self.studio.list_open) {
                            self.studio.list_open = !self.studio.list_open
                        }
                        widgets::divider(ui);
                        if ui
                            .add_enabled(
                                self.studio.editor.as_ref().is_some_and(Editor::can_undo),
                                egui::Button::new("撤销"),
                            )
                            .clicked()
                        {
                            self.studio.editor.as_mut().unwrap().undo();
                            self.rebuild_work();
                        }
                        if ui
                            .add_enabled(
                                self.studio.editor.as_ref().is_some_and(Editor::can_redo),
                                egui::Button::new("重做"),
                            )
                            .clicked()
                        {
                            self.studio.editor.as_mut().unwrap().redo();
                            self.rebuild_work();
                        }
                        if tab == WorkTab::Edit {
                            if button(ui, "页面调整", false) {
                                self.studio.list_open = true
                            }
                            if button(ui, "保存文档副本", false) {
                                self.studio.convert_open = true
                            }
                        }
                        if button(ui, "保存编辑项目", false) {
                            self.save_project()
                        }
                        if button(ui, "载入项目", false) {
                            self.load_project()
                        }
                    }
                    WorkTab::Convert => {
                        for (format, label) in [
                            (Format::Pdf, "转换 / 保存 PDF"),
                            (Format::Ofd, "转换 / 保存 OFD"),
                            (Format::PngZip, "页面转 PNG"),
                        ] {
                            if button(ui, label, false) {
                                self.studio.format = format;
                                self.studio.convert_open = true
                            }
                        }
                        ui.label(
                            RichText::new("页面外观转换；文本、附件、签名结构不保留").color(MUTED),
                        );
                    }
                    WorkTab::Sign => {
                        if button(ui, "导入印章图片", self.studio.tool == Tool::Stamp) {
                            self.pick_stamp()
                        }
                        if button(ui, "PDF 证书签名", false) {
                            self.studio.native = true;
                            self.studio.sign_open = true
                        }
                        if button(ui, "文件分离签名 .p7s", false) {
                            self.studio.native = false;
                            self.studio.sign_open = true
                        }
                        if button(ui, "验证 PDF 签名", false)
                            && let Some(source) = self.path.clone()
                        {
                            self.dispatch(
                                Job::Verify {
                                    source,
                                    signature: None,
                                },
                                "正在验证 PDF 签名…",
                            )
                        }
                        if button(ui, "验证 .p7s", false)
                            && let Some(signature) = rfd::FileDialog::new()
                                .add_filter("CMS 分离签名", &["p7s"])
                                .pick_file()
                            && let Some(source) = self.path.clone()
                        {
                            self.dispatch(
                                Job::Verify {
                                    source,
                                    signature: Some(signature),
                                },
                                "正在验证文件签名…",
                            )
                        }
                    }
                    WorkTab::Settings => {
                        ui.checkbox(&mut self.studio.default_ofd, ".ofd");
                        ui.checkbox(&mut self.studio.default_pdf, ".pdf");
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new("设置默认打开程序")
                                    .min_size(Vec2::new(150., 36.)),
                            )
                            .clicked()
                        {
                            self.dispatch(
                                Job::Defaults {
                                    ofd: self.studio.default_ofd,
                                    pdf: self.studio.default_pdf,
                                },
                                "正在注册默认应用…",
                            );
                        }
                        ui.label(
                            RichText::new(if cfg!(windows) {
                                "Windows 将打开系统设置，由您确认默认应用。"
                            } else {
                                "设置当前用户的文件关联，程序应保留在稳定路径。"
                            })
                            .color(MUTED),
                        );
                    }
                    _ => (),
                });
            });
        if matches!(tab, WorkTab::Annotate | WorkTab::Edit | WorkTab::Sign)
            && self.studio.tool != Tool::Select
        {
            ui.horizontal(|ui| {
                if self.studio.tool == Tool::Text {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.studio.text)
                            .desired_width(350.)
                            .hint_text("新增文字内容"),
                    );
                    ui.label("字号(mm)");
                    ui.add(egui::DragValue::new(&mut self.studio.size).range(1.0..=30.0));
                }
                if self.studio.tool != Tool::Stamp {
                    let mut color = egui::Color32::from_rgba_unmultiplied(
                        self.studio.color[0],
                        self.studio.color[1],
                        self.studio.color[2],
                        self.studio.color[3],
                    );
                    ui.color_edit_button_srgba(&mut color);
                    self.studio.color = color.to_array();
                    ui.label("线宽(mm)");
                    ui.add(egui::DragValue::new(&mut self.studio.width).range(0.1..=5.0));
                }
                ui.small("点击或拖动页面完成；原文内容不直接重排");
            });
        }
    }
    pub(super) fn preview(&mut self, doc: &Arc<Document>, page: usize) {
        if self.thumbnails.contains_key(&page)
            || self.thumb_pending.contains(&page)
            || !self.thumb_pending.is_empty()
        {
            return;
        }
        let area = doc.pages[page].area;
        let zoom = (0.28f32).min((300. / (area.w * 96. / 25.4)).min(400. / (area.h * 96. / 25.4)));
        let key = Key {
            page,
            zoom: (zoom * 1000.) as u32,
            rotation: self.rotation,
            generation: self.generation,
        };
        self.thumb_pending.insert(page);
        let _ = self.tx.send(Command::Render {
            doc: doc.clone(),
            key,
            thumb: true,
        });
    }
    pub(super) fn mark_interaction(&mut self, ui: &mut egui::Ui, page: usize, rect: egui::Rect) {
        if !matches!(
            self.studio.tab,
            WorkTab::Annotate | WorkTab::Edit | WorkTab::Sign
        ) {
            return;
        }
        let (Some(source), Some(editor)) = (&self.studio.source, &self.studio.editor) else {
            return;
        };
        let source = source.clone();
        let edit = editor.project.pages[page].clone();
        let a = source.pages[edit.source].area;
        let turns = (edit.turns + self.rotation / 90) % 4;
        let to_point = |pos: egui::Pos2| {
            let x = ((pos.x - rect.left()) / rect.width()).clamp(0., 1.);
            let y = ((pos.y - rect.top()) / rect.height()).clamp(0., 1.);
            match turns {
                1 => Point {
                    x: y * a.w,
                    y: (1. - x) * a.h,
                },
                2 => Point {
                    x: (1. - x) * a.w,
                    y: (1. - y) * a.h,
                },
                3 => Point {
                    x: (1. - y) * a.w,
                    y: x * a.h,
                },
                _ => Point {
                    x: x * a.w,
                    y: y * a.h,
                },
            }
        };
        let response = ui.interact(
            rect,
            egui::Id::new(("edit-page", page)),
            egui::Sense::click_and_drag(),
        );
        let tool = self.studio.tool;
        if let Some((p, i)) = self.studio.selected
            && p == page
            && let Some(mark) = edit.marks.get(i)
        {
            let b = mark.bounds();
            let uv = |pt: Point| {
                let (x, y) = (pt.x / a.w, pt.y / a.h);
                let (x, y) = match turns {
                    1 => (1. - y, x),
                    2 => (1. - x, 1. - y),
                    3 => (y, 1. - x),
                    _ => (x, y),
                };
                egui::pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                )
            };
            let corners = [
                uv(Point { x: b.x, y: b.y }),
                uv(Point {
                    x: b.x + b.w,
                    y: b.y,
                }),
                uv(Point {
                    x: b.x + b.w,
                    y: b.y + b.h,
                }),
                uv(Point {
                    x: b.x,
                    y: b.y + b.h,
                }),
            ];
            let selected = egui::Rect::from_min_max(
                egui::pos2(
                    corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min),
                    corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min),
                ),
                egui::pos2(
                    corners.iter().map(|p| p.x).fold(0f32, f32::max),
                    corners.iter().map(|p| p.y).fold(0f32, f32::max),
                ),
            );
            ui.painter().rect_stroke(
                selected.expand(2.),
                1.,
                Stroke::new(1.5, BLUE),
                egui::StrokeKind::Outside,
            );
        }
        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = to_point(pos - response.drag_delta());
            self.studio.drag = Some((page, p, vec![p]));
            if tool == Tool::Select {
                self.studio.selected = edit
                    .marks
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, m)| contains(m.bounds(), p))
                    .map(|(i, _)| (page, i));
            }
        }
        if response.dragged()
            && let (Some((which, _, points)), Some(pos)) =
                (&mut self.studio.drag, response.interact_pointer_pos())
            && *which == page
            && points.len() < 12000
        {
            let point = to_point(pos);
            if points
                .last()
                .is_none_or(|p| (p.x - point.x).abs() + (p.y - point.y).abs() > 0.1)
            {
                points.push(point);
            }
        }
        if response.drag_stopped()
            && let (Some((which, start, points)), Some(pos)) =
                (self.studio.drag.take(), response.interact_pointer_pos())
            && which == page
        {
            let end = to_point(pos);
            let r = Rect {
                x: start.x.min(end.x),
                y: start.y.min(end.y),
                w: (start.x - end.x).abs(),
                h: (start.y - end.y).abs(),
            };
            let color = self.studio.color;
            let width = self.studio.width;
            let mark = match tool {
                Tool::Highlight if r.w > 0.4 && r.h > 0.4 => Some(Mark::Highlight {
                    rect: r,
                    color: [color[0], color[1], color[2], 70],
                }),
                Tool::Rectangle if r.w > 0.4 && r.h > 0.4 => Some(Mark::Rectangle {
                    rect: r,
                    color,
                    width,
                }),
                Tool::Ink if points.len() >= 2 => Some(Mark::Ink {
                    points,
                    color,
                    width,
                }),
                _ => None,
            };
            if let Some(mark) = mark {
                if let Err(e) = self.studio.editable() {
                    self.error = Some(e.to_string());
                } else {
                    self.studio.stage(|p| p.pages[page].marks.push(mark));
                }
            } else if tool == Tool::Select
                && let Some((p, i)) = self.studio.selected
                && p == page
            {
                let b = edit.marks[i].bounds();
                let x = b.x + end.x - start.x;
                let y = b.y + end.y - start.y;
                self.studio
                    .stage(|p| p.pages[page].marks[i].move_to(x, y, Rect { x: 0., y: 0., ..a }));
            }
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = to_point(pos);
            match tool {
                Tool::Select => {
                    self.studio.selected = edit
                        .marks
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, m)| contains(m.bounds(), p))
                        .map(|(i, _)| (page, i));
                    if let Some((_, i)) = self.studio.selected
                        && let Mark::Text { text, size, .. } = &edit.marks[i]
                    {
                        self.studio.text = text.clone();
                        self.studio.size = *size;
                    }
                }
                Tool::Text => {
                    if let Err(e) = self.studio.editable() {
                        self.error = Some(e.to_string());
                        return;
                    }
                    let size = self.studio.size;
                    let h = (self.studio.text.lines().count().max(1) as f32 * size * 1.5).min(a.h);
                    let r = Rect {
                        x: p.x.min((a.w - 80.).max(0.)),
                        y: p.y.min((a.h - h).max(0.)),
                        w: 80f32.min(a.w),
                        h,
                    };
                    let mark = Mark::Text {
                        rect: r,
                        text: self.studio.text.clone(),
                        size,
                        color: self.studio.color,
                    };
                    self.studio.stage(|p| p.pages[page].marks.push(mark));
                }
                Tool::Stamp => {
                    if let Some((png, label)) = self.studio.stamp.clone() {
                        let r = Rect {
                            x: p.x.min((a.w - 40.).max(0.)),
                            y: p.y.min((a.h - 40.).max(0.)),
                            w: 40f32.min(a.w),
                            h: 40f32.min(a.h),
                        };
                        self.studio.stage(|p| {
                            p.pages[page].marks.push(Mark::Stamp {
                                rect: r,
                                png,
                                label,
                            })
                        });
                    }
                }
                _ => (),
            }
        }
        if let Some((p, start, points)) = &self.studio.drag
            && *p == page
            && matches!(tool, Tool::Ink | Tool::Highlight | Tool::Rectangle)
        {
            let (screen_x, screen_y) = match turns {
                1 => (1. - start.y / a.h, start.x / a.w),
                2 => (1. - start.x / a.w, 1. - start.y / a.h),
                3 => (start.y / a.h, 1. - start.x / a.w),
                _ => (start.x / a.w, start.y / a.h),
            };
            let begin = egui::pos2(
                rect.left() + screen_x * rect.width(),
                rect.top() + screen_y * rect.height(),
            );
            if let Some(pos) = response.interact_pointer_pos() {
                if tool == Tool::Ink {
                    let line: Vec<_> = points
                        .iter()
                        .map(|p| {
                            let (x, y) = match turns {
                                1 => (1. - p.y / a.h, p.x / a.w),
                                2 => (1. - p.x / a.w, 1. - p.y / a.h),
                                3 => (p.y / a.h, 1. - p.x / a.w),
                                _ => (p.x / a.w, p.y / a.h),
                            };
                            egui::pos2(
                                rect.left() + x * rect.width(),
                                rect.top() + y * rect.height(),
                            )
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(
                        line,
                        Stroke::new(
                            2.,
                            egui::Color32::from_rgb(
                                self.studio.color[0],
                                self.studio.color[1],
                                self.studio.color[2],
                            ),
                        ),
                    ));
                } else {
                    ui.painter().rect_stroke(
                        egui::Rect::from_two_pos(begin, pos),
                        0.,
                        Stroke::new(1.5, BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
    }
    pub(super) fn studio_windows(&mut self, ctx: &egui::Context) {
        self.drop_choices_window(ctx);
        if self.studio.list_open {
            let mut open = true;
            let source = self.studio.source.clone();
            let project = self.studio.editor.as_ref().map(|e| e.project.clone());
            egui::Window::new("标注与页面调整")
                .open(&mut open)
                .default_width(480.)
                .show(ctx, |ui| {
                    if let (Some(source), Some(project)) = (&source, &project) {
                        ui.label(format!(
                            "当前第 {} 页，源第 {} 页",
                            self.page + 1,
                            project.pages[self.page].source + 1
                        ));
                        ui.add_enabled_ui(self.studio.editable().is_ok(), |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("页面左移").clicked() && self.page > 0 {
                                    let p = self.page;
                                    self.studio.stage(|s| s.pages.swap(p, p - 1));
                                    self.page -= 1;
                                    self.scroll_target = Some(self.page);
                                }
                                if ui.button("页面右移").clicked()
                                    && self.page + 1 < project.pages.len()
                                {
                                    let p = self.page;
                                    self.studio.stage(|s| s.pages.swap(p, p + 1));
                                    self.page += 1;
                                    self.scroll_target = Some(self.page);
                                }
                                if ui.button("永久旋转页面").clicked() {
                                    let p = self.page;
                                    self.studio
                                        .stage(|s| s.pages[p].turns = (s.pages[p].turns + 1) % 4)
                                }
                                if ui
                                    .add_enabled(
                                        project.pages.len() > 1,
                                        egui::Button::new("删除此页"),
                                    )
                                    .clicked()
                                {
                                    let p = self.page;
                                    self.studio.stage(|s| {
                                        s.pages.remove(p);
                                    });
                                }
                            });
                            ui.small("页面调整可撤销；只有保存文档副本才写入新文件。");
                            ui.separator();
                            for (i, mark) in project.pages[self.page.min(project.pages.len() - 1)]
                                .marks
                                .iter()
                                .enumerate()
                            {
                                let title = match mark {
                                    Mark::Text { text, .. } => format!(
                                        "文字：{}",
                                        text.chars().take(30).collect::<String>()
                                    ),
                                    Mark::Stamp { label, .. } => format!("印章：{label}"),
                                    Mark::Ink { .. } => "手绘".into(),
                                    Mark::Highlight { .. } => "高亮".into(),
                                    Mark::Rectangle { .. } => "矩形".into(),
                                };
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(
                                            self.studio.selected == Some((self.page, i)),
                                            title,
                                        )
                                        .clicked()
                                    {
                                        self.studio.selected = Some((self.page, i));
                                        if let Mark::Text { text, size, .. } = mark {
                                            self.studio.text = text.clone();
                                            self.studio.size = *size;
                                        }
                                    }
                                    if ui.small_button("删除").clicked() {
                                        let page = self.page;
                                        self.studio.stage(|s| {
                                            s.pages[page].marks.remove(i);
                                        });
                                    }
                                });
                            }
                            if let Some((page, index)) = self.studio.selected
                                && page < project.pages.len()
                                && let Some(mark) = project.pages[page].marks.get(index)
                            {
                                ui.separator();
                                let mut bounds = mark.bounds();
                                let mut changed = false;
                                ui.horizontal(|ui| {
                                    for (label, value) in [
                                        ("X", &mut bounds.x),
                                        ("Y", &mut bounds.y),
                                        ("宽", &mut bounds.w),
                                        ("高", &mut bounds.h),
                                    ] {
                                        ui.label(label);
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(value)
                                                    .speed(0.2)
                                                    .range(0.0..=20000.0),
                                            )
                                            .changed();
                                    }
                                });
                                if changed {
                                    self.studio.stage(|s| {
                                        let m = &mut s.pages[page].marks[index];
                                        match m {
                                            Mark::Text { rect, .. }
                                            | Mark::Rectangle { rect, .. }
                                            | Mark::Highlight { rect, .. }
                                            | Mark::Stamp { rect, .. } => *rect = bounds,
                                            _ => m.move_to(
                                                bounds.x,
                                                bounds.y,
                                                source.pages[project.pages[page].source].area,
                                            ),
                                        }
                                    });
                                }
                                if matches!(mark, Mark::Text { .. }) {
                                    ui.text_edit_multiline(&mut self.studio.text);
                                    ui.add(
                                        egui::DragValue::new(&mut self.studio.size)
                                            .range(1.0..=30.0),
                                    );
                                    if ui.button("更新选中文字").clicked() {
                                        let text = self.studio.text.clone();
                                        let size = self.studio.size;
                                        self.studio.stage(|s| {
                                            if let Mark::Text {
                                                text: t, size: sz, ..
                                            } = &mut s.pages[page].marks[index]
                                            {
                                                *t = text;
                                                *sz = size;
                                            }
                                        });
                                    }
                                }
                            }
                        });
                    }
                });
            self.studio.list_open = open;
        }
        if self.studio.convert_open {
            let mut open = true;
            let mut save = false;
            egui::Window::new("转换 / 保存文档副本")
                .open(&mut open)
                .resizable(false)
                .default_width(470.)
                .show(ctx, |ui| {
                    ui.label("按页面外观重建为新文档，包含当前的标注、文字、印章和页面调整。");
                    ui.colored_label(
                        Color32::from_rgb(155, 107, 30),
                        "输出为栅格化页面，不保留原文字层、附件、表单、签名结构。",
                    );
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        for (f, label) in [
                            (Format::Pdf, "PDF"),
                            (Format::Ofd, "OFD"),
                            (Format::PngZip, "PNG ZIP"),
                        ] {
                            ui.selectable_value(&mut self.studio.format, f, label);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("页码范围");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.studio.range)
                                .hint_text("留空全部；1-3,5"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("质量");
                        egui::ComboBox::from_id_salt("convert-dpi")
                            .selected_text(format!("{} DPI", self.studio.dpi))
                            .show_ui(ui, |ui| {
                                for dpi in [96, 150, 300] {
                                    ui.selectable_value(
                                        &mut self.studio.dpi,
                                        dpi,
                                        format!("{dpi} DPI"),
                                    );
                                }
                            });
                    });
                    if ui
                        .add_enabled(
                            self.doc.is_some() && !self.busy,
                            egui::Button::new("选择位置并保存新副本"),
                        )
                        .clicked()
                    {
                        save = true
                    }
                });
            self.studio.convert_open = open;
            if save && let Some(doc) = self.doc.clone() {
                match printing::page_range(&self.studio.range, doc.pages.len()) {
                    Ok(pages) => {
                        let (ext, name) = match self.studio.format {
                            Format::Pdf => ("pdf", "edited.pdf"),
                            Format::Ofd => ("ofd", "edited.ofd"),
                            Format::PngZip => ("zip", "pages.zip"),
                        };
                        if let Some(target) = rfd::FileDialog::new()
                            .add_filter("转换文件", &[ext])
                            .set_file_name(name)
                            .save_file()
                        {
                            self.studio.convert_open = false;
                            self.dispatch(
                                Job::Convert {
                                    doc,
                                    source: self.path.clone().unwrap(),
                                    target,
                                    options: conversion::Options {
                                        format: self.studio.format,
                                        pages,
                                        dpi: self.studio.dpi,
                                    },
                                },
                                "正在转换 / 保存副本…",
                            );
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
        if self.studio.sign_open {
            let mut open = true;
            let mut sign = false;
            egui::Window::new("证书数字签名").open(&mut open).resizable(false).default_width(500.).show(ctx,|ui|{
            ui.label(if self.studio.native{"PDF 内嵌签名：RSA / SHA-256，adbe.pkcs7.detached。"}else{"OFD/PDF 文件分离签名：生成 .p7s，绑定整个原文件。"});ui.small("不支持 SM2/国密签章或硬件证书；不验证 CA 信任链、吊销状态、可信时间戳。可见印章需单独添加并保存后再签名。");if ui.button("选择 P12 / PFX 证书").clicked(){self.studio.certificate=rfd::FileDialog::new().add_filter("带私钥证书包",&["p12","pfx"]).pick_file();}ui.label(self.studio.certificate.as_ref().map(|p|p.display().to_string()).unwrap_or("尚未选择证书".into()));ui.add(egui::TextEdit::singleline(&mut self.studio.password).password(true).hint_text("证书包密码（不保存到项目）"));ui.add(egui::TextEdit::singleline(&mut self.studio.reason).hint_text("签名原因"));
            if !self.studio.unchanged(){ui.colored_label(Color32::from_rgb(174,92,25),"工作副本已有编辑。请先保存新文档、打开该文档，再对它签名。");}
            if ui.add_enabled(self.studio.unchanged()&&self.studio.certificate.is_some()&&!self.busy,egui::Button::new("签名并保存新文件")).clicked(){sign=true}});
            self.studio.sign_open = open;
            if !open {
                use zeroize::Zeroize;
                self.studio.password.zeroize();
            }
            if sign {
                let native = self.studio.native;
                if native && self.doc.as_ref().is_none_or(|d| !d.is_pdf()) {
                    self.error =
                        Some("OFD 请使用分离式 .p7s 签名；PDF 内嵌签名只适用于 PDF".into());
                } else {
                    let ext = if native { "pdf" } else { "p7s" };
                    if let Some(target) = rfd::FileDialog::new()
                        .add_filter("签名文件", &[ext])
                        .set_file_name(if native { "signed.pdf" } else { "document.p7s" })
                        .save_file()
                    {
                        let password = Zeroizing::new(std::mem::take(&mut self.studio.password));
                        self.studio.sign_open = false;
                        self.dispatch(
                            Job::Sign {
                                source: self.path.clone().unwrap(),
                                target,
                                certificate: self.studio.certificate.clone().unwrap(),
                                password,
                                reason: self.studio.reason.clone(),
                                native,
                                hash: self.studio.source.as_ref().unwrap().source_hash.clone(),
                            },
                            "正在生成证书签名…",
                        );
                    }
                }
            }
        }
        if let Some(report) = self.studio.report.clone() {
            let mut open = true;
            egui::Window::new("操作结果 / 验签报告")
                .open(&mut open)
                .default_width(620.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(500.)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(report).wrap());
                        });
                });
            if !open {
                self.studio.report = None;
            }
        }
        if let Some(path) = self.studio.pending_open.clone() {
            egui::Window::new("尚有未保存的编辑项目")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("保存项目可保留可修改的标注和页面调整，原文件一直未改写。");
                    ui.horizontal(|ui| {
                        if ui.button("先保存项目").clicked() {
                            self.save_project()
                        }
                        if ui.button("丢弃工作副本并打开").clicked() {
                            self.studio.pending_open = None;
                            self.load(path.clone(), 0)
                        }
                        if ui.button("取消打开").clicked() {
                            self.studio.pending_open = None
                        }
                    });
                });
        }
    }
    pub(super) fn try_load(&mut self, path: PathBuf) {
        if self.studio.editor.as_ref().is_some_and(Editor::dirty) {
            self.studio.pending_open = Some(path);
        } else {
            self.start_page = 1;
            self.load(path, 0)
        }
    }
}
pub(super) fn parse_tab(s: &str) -> WorkTab {
    match s {
        "annotations" => WorkTab::Annotate,
        "edit" => WorkTab::Edit,
        "convert" => WorkTab::Convert,
        "sign" => WorkTab::Sign,
        "settings" => WorkTab::Settings,
        _ => WorkTab::Read,
    }
}
fn contains(r: Rect, p: Point) -> bool {
    p.x >= r.x - 1. && p.y >= r.y - 1. && p.x <= r.x + r.w + 1. && p.y <= r.y + r.h + 1.
}

impl ReaderApp {
    pub(super) fn studio_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
            self.save_project()
        }
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z))
                && self.studio.editor.as_mut().is_some_and(Editor::undo)
            {
                self.rebuild_work()
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y))
                && self.studio.editor.as_mut().is_some_and(Editor::redo)
            {
                self.rebuild_work()
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                && let Some((page, index)) = self.studio.selected
            {
                self.studio.stage(|s| {
                    if page < s.pages.len() && index < s.pages[page].marks.len() {
                        s.pages[page].marks.remove(index);
                    }
                });
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> ReaderApp {
        let mut a = super::super::tests::app();
        let source = Arc::new(
            Document::pdf_from_bytes(include_bytes!("../samples/demo.pdf").to_vec()).unwrap(),
        );
        a.doc = Some(source.clone());
        a.studio.attach(source);
        a.studio.tab = WorkTab::Annotate;
        a
    }
    fn input(ctx: &egui::Context, app: &mut ReaderApp, time: f64, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000., 760.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.reading(ui));
            app.apply_staged();
        });
    }
    fn mouse(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn click_places_text_and_undo_restores_page() {
        let ctx = egui::Context::default();
        let mut app = setup();
        app.studio.tool = Tool::Text;
        input(&ctx, &mut app, 0., vec![]);
        let pos = egui::pos2(400., 250.);
        input(
            &ctx,
            &mut app,
            0.1,
            vec![egui::Event::PointerMoved(pos), mouse(pos, true)],
        );
        input(&ctx, &mut app, 0.2, vec![mouse(pos, false)]);
        assert_eq!(
            app.studio.editor.as_ref().unwrap().project.pages[0]
                .marks
                .len(),
            1
        );
        assert!(app.doc.as_ref().unwrap().pages[0].text.contains("文字批注"));
        assert!(app.studio.editor.as_mut().unwrap().undo());
        app.rebuild_work();
        assert_eq!(
            app.studio.editor.as_ref().unwrap().project.pages[0]
                .marks
                .len(),
            0
        );
    }
    #[test]
    fn drag_places_rectangle() {
        let ctx = egui::Context::default();
        let mut app = setup();
        app.studio.tool = Tool::Rectangle;
        input(&ctx, &mut app, 0., vec![]);
        let start = egui::pos2(350., 300.);
        let end = egui::pos2(450., 380.);
        input(
            &ctx,
            &mut app,
            0.1,
            vec![egui::Event::PointerMoved(start), mouse(start, true)],
        );
        input(&ctx, &mut app, 0.2, vec![egui::Event::PointerMoved(end)]);
        input(&ctx, &mut app, 0.3, vec![mouse(end, false)]);
        let editor = app.studio.editor.as_ref().unwrap();
        assert_eq!(editor.project.pages[0].marks.len(), 1);
        assert!(matches!(
            editor.project.pages[0].marks[0],
            Mark::Rectangle { .. }
        ));
    }
    #[test]
    fn fast_scroll_defers_high_resolution_requests() {
        let ctx = egui::Context::default();
        let mut app = setup();
        app.studio.tab = WorkTab::Read;
        input(&ctx, &mut app, 0., vec![]);
        app.pending.clear();
        input(
            &ctx,
            &mut app,
            0.1,
            vec![
                egui::Event::PointerMoved(egui::pos2(500., 350.)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., -95.),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(
            app.pending.is_empty(),
            "HQ requests should wait until wheel settles"
        );
        input(&ctx, &mut app, 2., vec![]);
        input(&ctx, &mut app, 3., vec![]);
        assert!(app.pending.len() <= 2);
    }
}

impl ReaderApp {
    pub(super) fn handle_drop(&mut self, ctx: &egui::Context) {
        // Consume drop events once, rather than repeatedly opening the same path
        // during additional egui passes/frames. Do not silently choose a directory
        // or an unsupported first item from a mixed batch.
        let files = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if files.is_empty() {
            return;
        }
        let mut paths = vec![];
        let mut rejected = 0;
        for file in files {
            if let Some(path) = file.path {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if path.is_file()
                    && (ext.eq_ignore_ascii_case("pdf") || ext.eq_ignore_ascii_case("ofd"))
                {
                    if let Ok(path) = path.canonicalize() {
                        if !paths.contains(&path) {
                            paths.push(path)
                        } else {
                            rejected += 1
                        }
                    } else {
                        rejected += 1
                    }
                } else {
                    rejected += 1
                }
            } else {
                rejected += 1
            }
        }
        match paths.len() {
            0 => {
                self.error=Some("未收到可打开的本地文档。请拖入磁盘上的 .ofd 或 .pdf 文件（支持大写扩展名和中文路径），不要拖入文件夹。邮件/浏览器中的虚拟附件请先保存到磁盘。".into());
            }
            1 => {
                self.start_page = 1;
                self.studio.tab = WorkTab::Read;
                self.try_load(paths.remove(0));
                if rejected > 0 {
                    self.status = format!("已接收文档，忽略了 {rejected} 个重复/非文档项");
                }
            }
            _ => {
                self.studio.drop_choices = paths;
                self.status = "拖入了多份文档，请选择要打开的文件".into();
            }
        }
        ctx.request_repaint();
    }
    fn drop_choices_window(&mut self, ctx: &egui::Context) {
        if self.studio.drop_choices.is_empty() {
            return;
        }
        let mut open = true;
        let mut selected = None;
        egui::Window::new("选择拖入的文档")
            .open(&mut open)
            .collapsible(false)
            .default_width(480.)
            .show(ctx, |ui| {
                ui.label("一次打开一份文档，请选择：");
                for path in &self.studio.drop_choices {
                    if ui
                        .button(path.file_name().unwrap_or_default().to_string_lossy())
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        selected = Some(path.clone());
                    }
                }
            });
        if let Some(path) = selected {
            self.studio.drop_choices.clear();
            self.studio.tab = WorkTab::Read;
            self.try_load(path);
        } else if !open {
            self.studio.drop_choices.clear();
        }
    }
    pub(super) fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let layer = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("document-drop-hint"),
        ));
        let rect = ctx.content_rect();
        layer.rect_filled(rect, 0., Color32::from_black_alpha(80));
        let box_rect = egui::Rect::from_center_size(rect.center(), Vec2::new(410., 100.));
        layer.rect_filled(box_rect, 12., Color32::WHITE);
        layer.rect_stroke(
            box_rect,
            12.,
            Stroke::new(2., BLUE),
            egui::StrokeKind::Inside,
        );
        layer.text(
            box_rect.center(),
            egui::Align2::CENTER_CENTER,
            "松开鼠标，打开 OFD / PDF 文档",
            egui::FontId::proportional(21.),
            INK,
        );
    }
}
#[cfg(test)]
mod drop_tests {
    use super::*;
    fn run_drop(ctx: &egui::Context, app: &mut ReaderApp, files: Vec<egui::DroppedFile>) {
        let input = egui::RawInput {
            dropped_files: files,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.handle_drop(ctx);
            app.handle_drop(ctx);
        });
    }
    #[test]
    fn uppercase_chinese_path_is_consumed_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("中文 文档.PDF");
        std::fs::write(&path, include_bytes!("../samples/demo.pdf")).unwrap();
        let mut app = super::super::tests::app();
        let before = app.generation;
        run_drop(
            &egui::Context::default(),
            &mut app,
            vec![egui::DroppedFile {
                path: Some(path.clone()),
                ..Default::default()
            }],
        );
        assert_eq!(app.generation, before + 1);
        assert_eq!(app.path, Some(path.canonicalize().unwrap()));
        assert!(app.loading);
        assert!(
            Document::open(app.path.as_ref().unwrap(), 0)
                .unwrap()
                .is_pdf()
        );
    }
    #[test]
    fn directory_does_not_hide_supported_ofd() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("中文.OFD");
        std::fs::write(&path, include_bytes!("../samples/demo.ofd")).unwrap();
        let mut app = super::super::tests::app();
        run_drop(
            &egui::Context::default(),
            &mut app,
            vec![
                egui::DroppedFile {
                    path: Some(temp.path().into()),
                    ..Default::default()
                },
                egui::DroppedFile {
                    path: Some(path.clone()),
                    ..Default::default()
                },
            ],
        );
        assert_eq!(app.path, Some(path.canonicalize().unwrap()));
        assert_eq!(
            Document::open(app.path.as_ref().unwrap(), 0)
                .unwrap()
                .pages
                .len(),
            3
        );
    }
    #[test]
    fn unsupported_drop_is_reported_without_closing_current_document() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("other.txt");
        std::fs::write(&path, b"text").unwrap();
        let mut app = super::super::tests::app();
        let before = app.generation;
        run_drop(
            &egui::Context::default(),
            &mut app,
            vec![egui::DroppedFile {
                path: Some(path),
                ..Default::default()
            }],
        );
        assert_eq!(app.generation, before);
        assert!(app.doc.is_some());
        assert!(app.error.is_some());
    }
    #[test]
    fn multiple_documents_require_selection() {
        let temp = tempfile::tempdir().unwrap();
        let mut files = vec![];
        for i in 0..2 {
            let path = temp.path().join(format!("document-{i}.pdf"));
            std::fs::write(&path, include_bytes!("../samples/demo.pdf")).unwrap();
            files.push(egui::DroppedFile {
                path: Some(path),
                ..Default::default()
            });
        }
        let mut app = super::super::tests::app();
        let before = app.generation;
        run_drop(&egui::Context::default(), &mut app, files);
        assert_eq!(app.generation, before);
        assert_eq!(app.studio.drop_choices.len(), 2);
    }
}
