#[path = "studio.rs"]
mod studio;
use crate::ui::{self as widgets, BLUE, INK, Icon, LINE, MUTED};
use eframe::egui::{self, Color32, RichText, Stroke, TextureHandle, TextureOptions, Vec2};
use ofd_reader::{
    document::Document,
    printing,
    render::{Rendered, Renderer},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};
use studio::{Studio, Tool, WorkTab};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Pages,
    Outline,
    Attachments,
    Checks,
    Info,
}
#[derive(Clone, Copy, PartialEq)]
enum Fit {
    Width,
    Page,
    Height,
    Manual,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    page: usize,
    zoom: u32,
    rotation: u32,
    generation: u64,
}
enum Command {
    Load {
        path: PathBuf,
        index: usize,
        generation: u64,
    },
    Render {
        doc: Arc<Document>,
        key: Key,
        thumb: bool,
    },
    Export {
        doc: Arc<Document>,
        source: PathBuf,
        target: PathBuf,
        key: Key,
    },
    Print {
        doc: Arc<Document>,
        options: printing::Options,
        target: Option<PathBuf>,
        source: PathBuf,
    },
    Printers,
    Studio(studio::Job),
}
enum Reply {
    Loaded {
        result: Result<Arc<Document>, String>,
        generation: u64,
    },
    Rendered {
        result: Result<Rendered, String>,
        key: Key,
        thumb: bool,
    },
    Done(Result<String, String>),
    Printers(Result<Vec<String>, String>),
    Studio(studio::Outcome),
    Skipped {
        key: Key,
        thumb: bool,
    },
}
struct Cached {
    key: Key,
    texture: Option<TextureHandle>,
    size: usize,
    warnings: Vec<String>,
    error: Option<String>,
}
struct PrintDialog {
    open: bool,
    range: String,
    copies: u16,
    dpi: u32,
    printer: String,
    printers: Vec<String>,
    hint: String,
}
impl Default for PrintDialog {
    fn default() -> Self {
        Self {
            open: false,
            range: String::new(),
            copies: 1,
            dpi: 150,
            printer: String::new(),
            printers: vec![],
            hint: String::new(),
        }
    }
}

pub fn launch(
    path: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    single: bool,
    start_page: usize,
    project_path: Option<PathBuf>,
    initial_tab: String,
) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440., 940.])
            .with_min_inner_size([900., 600.])
            .with_title("简阅 · OFD / PDF 文档阅读器")
            .with_app_id("jian-yue")
            .with_drag_and_drop(true)
            .with_icon(Arc::new(studio::window_icon())),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "简阅",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ReaderApp::new(
                cc,
                path,
                screenshot,
                single,
                start_page,
                project_path,
                initial_tab,
            )))
        }),
    )
}
struct ReaderApp {
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Reply>,
    doc: Option<Arc<Document>>,
    path: Option<PathBuf>,
    document_index: usize,
    generation: u64,
    page: usize,
    page_input: usize,
    start_page: usize,
    zoom: f32,
    rotation: u32,
    fit: Fit,
    dpi: f32,
    continuous: bool,
    fullscreen: bool,
    cache: BTreeMap<usize, Cached>,
    pending: BTreeMap<usize, Key>,
    thumbnails: BTreeMap<usize, TextureHandle>,
    thumb_pending: BTreeSet<usize>,
    tab: Tab,
    sidebar_open: bool,
    query: String,
    search_open: bool,
    search_focus: bool,
    scroll_target: Option<usize>,
    loading: bool,
    busy: bool,
    status: String,
    error: Option<String>,
    print_dialog: PrintDialog,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    ready_frames: usize,
    studio: Studio,
    last_scroll: f64,
    live_page: Arc<std::sync::atomic::AtomicUsize>,
    live_generation: Arc<std::sync::atomic::AtomicU64>,
}
impl ReaderApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        screenshot: Option<PathBuf>,
        single: bool,
        start_page: usize,
        project_path: Option<PathBuf>,
        initial_tab: String,
    ) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "noto-cjk".into(),
            Arc::new(egui::FontData::from_static(ofd_reader::fonts::DEFAULT_CJK)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "noto-cjk".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = Color32::from_rgb(248, 250, 252);
        style.visuals.override_text_color = Some(INK);
        style.visuals.selection.bg_fill = Color32::from_rgb(223, 239, 251);
        style.visuals.selection.stroke = Stroke::new(1., BLUE);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(244, 247, 250);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(244, 247, 250);
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1., LINE);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(230, 242, 252);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(220, 238, 252);
        style.spacing.item_spacing = Vec2::new(8., 7.);
        style.spacing.button_padding = Vec2::new(11., 6.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
        cc.egui_ctx.set_style(style);
        let (tx, commands) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let live_page = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let live_generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_page = live_page.clone();
        let worker_generation = live_generation.clone();
        let ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("document-render".into())
            .spawn(move || {
                let mut renderer = Renderer::new();
                while let Ok(command) = commands.recv() {
                    let reply = match command {
                        Command::Studio(job) => Reply::Studio(studio::execute(job)),
                        Command::Load {
                            path,
                            index,
                            generation,
                        } => Reply::Loaded {
                            result: Document::open(&path, index)
                                .map(Arc::new)
                                .map_err(|e| format!("{e:#}")),
                            generation,
                        },
                        Command::Render { doc, key, thumb } => {
                            if key.generation
                                != worker_generation.load(std::sync::atomic::Ordering::Relaxed)
                                || key.page.abs_diff(
                                    worker_page.load(std::sync::atomic::Ordering::Relaxed),
                                ) > 2
                            {
                                Reply::Skipped { key, thumb }
                            } else {
                                Reply::Rendered {
                                    result: doc
                                        .render(
                                            &mut renderer,
                                            key.page,
                                            key.zoom as f32 / 1000.,
                                            key.rotation,
                                        )
                                        .map_err(|e| format!("{e:#}")),
                                    key,
                                    thumb,
                                }
                            }
                        }
                        Command::Export {
                            doc,
                            source,
                            target,
                            key,
                        } => Reply::Done(
                            (|| {
                                anyhow::ensure!(doc.export_allowed, "文档权限禁止导出");
                                let r = doc.render(
                                    &mut renderer,
                                    key.page,
                                    key.zoom as f32 / 1000.,
                                    key.rotation,
                                )?;
                                ofd_reader::export::write(
                                    &source,
                                    &target,
                                    &r.pixmap.encode_png()?,
                                )?;
                                Ok::<_, anyhow::Error>(if r.warnings.is_empty() {
                                    "页面图片已保存".into()
                                } else {
                                    format!("页面已保存，包含 {} 项渲染提示", r.warnings.len())
                                })
                            })()
                            .map_err(|e| format!("{e:#}")),
                        ),
                        Command::Print {
                            doc,
                            options,
                            target,
                            source,
                        } => Reply::Done(
                            (|| {
                                if let Some(target) = target {
                                    let bytes = printing::print_pdf(&doc, &options)?;
                                    ofd_reader::export::write(&source, &target, &bytes)?;
                                    Ok("打印 PDF 已保存（未向打印机提交）".into())
                                } else {
                                    printing::print(&doc, &options)
                                }
                            })()
                            .map_err(|e| format!("{e:#}")),
                        ),
                        Command::Printers => {
                            Reply::Printers(printing::printers().map_err(|e| format!("{e:#}")))
                        }
                    };
                    if replies.send(reply).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("cannot spawn renderer");
        let mut app = Self {
            tx,
            rx,
            doc: None,
            path: None,
            document_index: 0,
            generation: 0,
            page: 0,
            page_input: 1,
            start_page,
            zoom: 1.,
            rotation: 0,
            fit: Fit::Manual,
            dpi: 1.,
            continuous: !single,
            fullscreen: false,
            cache: BTreeMap::new(),
            pending: BTreeMap::new(),
            thumbnails: BTreeMap::new(),
            thumb_pending: BTreeSet::new(),
            tab: Tab::Pages,
            sidebar_open: false,
            query: String::new(),
            search_open: false,
            search_focus: false,
            scroll_target: None,
            loading: false,
            busy: false,
            status: "就绪".into(),
            error: None,
            print_dialog: PrintDialog::default(),
            screenshot,
            screenshot_requested: false,
            ready_frames: 0,
            studio: Studio::default(),
            last_scroll: 0.,
            live_page,
            live_generation,
        };
        if let Some(path) = path {
            app.load(path, 0)
        }
        app.studio.startup_project = project_path;
        app.studio.tab = studio::parse_tab(&initial_tab);
        app
    }
    fn key(&self, page: usize) -> Key {
        Key {
            page,
            zoom: (self.zoom * self.dpi * 1000.).round() as u32,
            rotation: self.rotation,
            generation: self.generation,
        }
    }
    fn load(&mut self, path: PathBuf, index: usize) {
        self.generation += 1;
        self.live_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.studio.reset();
        self.path = Some(path.clone());
        self.document_index = index;
        self.doc = None;
        self.cache.clear();
        self.pending.clear();
        self.thumbnails.clear();
        self.thumb_pending.clear();
        self.error = None;
        self.query.clear();
        self.print_dialog.open = false;
        self.page = 0;
        self.page_input = 1;
        self.loading = true;
        self.status = "正在打开文档…".into();
        let _ = self.tx.send(Command::Load {
            path,
            index,
            generation: self.generation,
        });
    }
    fn pick(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("OFD / PDF 文档", &["ofd", "pdf"])
            .add_filter("OFD 文档", &["ofd"])
            .add_filter("PDF 文档", &["pdf"])
            .pick_file()
        {
            self.start_page = 1;
            self.try_load(path)
        }
    }
    fn goto(&mut self, page: usize) {
        if let Some(doc) = &self.doc {
            self.page = page.min(doc.pages.len() - 1);
            self.page_input = self.page + 1;
            self.scroll_target = Some(self.page);
            self.error = None;
        }
    }
    fn change_tab(&mut self, tab: Tab) {
        self.sidebar_open = !(self.sidebar_open && self.tab == tab && !self.search_open);
        self.tab = tab;
        self.search_open = false;
    }
    fn search(&mut self) {
        self.sidebar_open = true;
        self.search_open = true;
        self.search_focus = true;
    }
    fn request(&mut self, doc: &Arc<Document>, page: usize) {
        let key = self.key(page);
        if self.cache.get(&page).is_some_and(|c| c.key == key)
            || self.pending.contains_key(&page)
            || self.pending.len() >= 2
        {
            return;
        }
        self.pending.insert(page, key);
        let _ = self.tx.send(Command::Render {
            doc: doc.clone(),
            key,
            thumb: false,
        });
    }
    fn trim_cache(&mut self) {
        while self.cache.values().map(|c| c.size).sum::<usize>() > 64 * 1024 * 1024
            || self.cache.len() > 12
        {
            let remove = self
                .cache
                .keys()
                .copied()
                .filter(|p| *p != self.page)
                .max_by_key(|p| p.abs_diff(self.page));
            if let Some(remove) = remove {
                self.cache.remove(&remove);
            } else {
                break;
            }
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let mut uploads = 0;
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Studio(outcome) => {
                    self.busy = false;
                    self.accept_studio(outcome);
                }
                Reply::Skipped { key, thumb } => {
                    if thumb {
                        self.thumb_pending.remove(&key.page);
                    } else if self.pending.get(&key.page) == Some(&key) {
                        self.pending.remove(&key.page);
                    }
                }
                Reply::Loaded { result, generation } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.loading = false;
                    match result {
                        Ok(doc) => {
                            self.studio.attach(doc.clone());
                            if let Some(path) = self.studio.startup_project.take() {
                                self.dispatch(
                                    studio::Job::LoadProject {
                                        path,
                                        source: doc.clone(),
                                    },
                                    "正在载入编辑项目…",
                                );
                            }
                            self.page = self.start_page.saturating_sub(1).min(doc.pages.len() - 1);
                            self.page_input = self.page + 1;
                            self.scroll_target = Some(self.page);
                            self.status = format!("{} · {} 页", doc.format(), doc.pages.len());
                            if let Some(path) = &self.path {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                                    "{} — 简阅",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                )));
                            }
                            self.doc = Some(doc);
                        }
                        Err(e) => {
                            self.error = Some(e);
                            self.status = "打开失败".into();
                        }
                    }
                }
                Reply::Rendered { result, key, thumb } => {
                    if key.generation != self.generation {
                        continue;
                    }
                    uploads += 1;
                    if thumb {
                        self.thumb_pending.remove(&key.page);
                        if let Ok(r) = result {
                            let image = egui::ColorImage::from_rgba_premultiplied(
                                [r.pixmap.width() as usize, r.pixmap.height() as usize],
                                r.pixmap.data(),
                            );
                            self.thumbnails.insert(
                                key.page,
                                ctx.load_texture(
                                    format!("thumb-{}", key.page),
                                    image,
                                    TextureOptions::LINEAR,
                                ),
                            );
                            while self.thumbnails.len() > 40 {
                                let p = *self
                                    .thumbnails
                                    .keys()
                                    .max_by_key(|p| p.abs_diff(self.page))
                                    .unwrap();
                                self.thumbnails.remove(&p);
                            }
                        }
                    } else {
                        if self.pending.get(&key.page) == Some(&key) {
                            self.pending.remove(&key.page);
                        }
                        if key != self.key(key.page) {
                            continue;
                        }
                        let cache = match result {
                            Ok(r) => {
                                let size = r.pixmap.data().len();
                                let image = egui::ColorImage::from_rgba_premultiplied(
                                    [r.pixmap.width() as usize, r.pixmap.height() as usize],
                                    r.pixmap.data(),
                                );
                                Cached {
                                    key,
                                    texture: Some(ctx.load_texture(
                                        format!("page-{}", key.page),
                                        image,
                                        TextureOptions::LINEAR,
                                    )),
                                    size,
                                    warnings: r.warnings,
                                    error: None,
                                }
                            }
                            Err(e) => Cached {
                                key,
                                texture: None,
                                size: 0,
                                warnings: vec![],
                                error: Some(e),
                            },
                        };
                        self.cache.insert(key.page, cache);
                        self.trim_cache();
                    }
                }
                Reply::Done(result) => {
                    self.busy = false;
                    match result {
                        Ok(s) => self.status = s,
                        Err(e) => self.error = Some(e),
                    }
                }
                Reply::Printers(result) => match result {
                    Ok(p) => {
                        self.print_dialog.printers = p;
                        self.print_dialog.hint = String::new();
                    }
                    Err(e) => self.print_dialog.hint = e,
                },
            }
            if uploads >= 1 {
                ctx.request_repaint();
                break;
            }
        }
    }
    fn save_result(&mut self, result: anyhow::Result<()>, message: &str) {
        match result {
            Ok(()) => self.status = message.into(),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }
    fn export_png(&mut self) {
        let Some(doc) = self.doc.clone() else { return };
        if !doc.export_allowed {
            self.error = Some("文档权限禁止导出".into());
            return;
        }
        if let Some(target) = rfd::FileDialog::new()
            .add_filter("PNG 图片", &["png"])
            .set_file_name(format!("page-{}.png", self.page + 1))
            .save_file()
        {
            self.busy = true;
            self.status = "正在导出页面…".into();
            let _ = self.tx.send(Command::Export {
                doc,
                source: self.path.clone().unwrap(),
                target,
                key: self.key(self.page),
            });
        }
    }
    fn export_text(&mut self) {
        let Some(doc) = &self.doc else { return };
        if !doc.copy_allowed {
            self.error = Some("文档权限禁止复制文字".into());
            return;
        }
        if let Some(target) = rfd::FileDialog::new()
            .add_filter("UTF-8 文本", &["txt"])
            .set_file_name("document.txt")
            .save_file()
        {
            let text = doc
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| format!("--- 第 {} 页 ---\n{}\n", i + 1, p.text))
                .collect::<String>();
            self.save_result(
                ofd_reader::export::write(self.path.as_ref().unwrap(), &target, text.as_bytes()),
                "全文文字已保存",
            );
        }
    }
    fn export_report(&mut self) {
        let Some(doc) = &self.doc else { return };
        if let Some(target) = rfd::FileDialog::new()
            .add_filter("JSON 报告", &["json"])
            .set_file_name("document-check.json")
            .save_file()
        {
            let result = serde_json::to_vec_pretty(&doc.report())
                .map_err(anyhow::Error::from)
                .and_then(|bytes| {
                    ofd_reader::export::write(self.path.as_ref().unwrap(), &target, &bytes)
                });
            self.save_result(result, "检查报告已保存");
        }
    }
    fn open_print(&mut self) {
        if let Some(doc) = &self.doc {
            if !doc.print_allowed {
                self.error = Some("文档权限禁止打印".into());
                return;
            }
            self.print_dialog.open = true;
            if cfg!(target_os = "linux") {
                let _ = self.tx.send(Command::Printers);
            }
        }
    }
    fn print_window(&mut self, ctx: &egui::Context) {
        if !self.print_dialog.open {
            return;
        }
        let Some(doc) = self.doc.clone() else { return };
        let mut open = self.print_dialog.open;
        let mut submit = false;
        let mut save = false;
        egui::Window::new("打印文档").open(&mut open).collapsible(false).resizable(false).default_width(420.).show(ctx,|ui|{
            ui.label(RichText::new(format!("{} · {} 页",self.path.as_ref().unwrap().file_name().unwrap_or_default().to_string_lossy(),doc.pages.len())).strong());ui.add_space(8.);
            egui::Grid::new("print-options").spacing([16.,14.]).show(ui,|ui|{ui.label("页面范围");ui.add(egui::TextEdit::singleline(&mut self.print_dialog.range).hint_text("留空打印全部，例如 1-3,5").desired_width(260.));ui.end_row();ui.label("份数");ui.add(egui::DragValue::new(&mut self.print_dialog.copies).range(1..=99));ui.end_row();ui.label("打印质量");egui::ComboBox::from_id_salt("print-dpi").selected_text(format!("{} DPI",self.print_dialog.dpi)).show_ui(ui,|ui|{for dpi in [96,150,300]{ui.selectable_value(&mut self.print_dialog.dpi,dpi,format!("{dpi} DPI"));}});ui.end_row();
            if cfg!(target_os="linux"){ui.label("打印机");egui::ComboBox::from_id_salt("printer").selected_text(if self.print_dialog.printer.is_empty(){"系统默认"}else{&self.print_dialog.printer}).show_ui(ui,|ui|{ui.selectable_value(&mut self.print_dialog.printer,String::new(),"系统默认");for printer in &self.print_dialog.printers{ui.selectable_value(&mut self.print_dialog.printer,printer.clone(),printer);}});ui.end_row();}});
            ui.add_space(12.);ui.label(RichText::new(if cfg!(windows){"下一步打开 Windows 系统打印窗口，选择打印机和纸张。"}else{"确认后提交到 CUPS 打印队列。"}).color(MUTED));ui.small("按页面渲染结果打印，自动适合可打印区域；大页面会降低分辨率。存在渲染错误的页面将阻止打印。");if !self.print_dialog.hint.is_empty(){ui.colored_label(Color32::from_rgb(174,105,34),&self.print_dialog.hint);}ui.separator();ui.horizontal(|ui|{if ui.add_enabled(!self.busy,egui::Button::new("保存打印 PDF")).clicked(){save=true}
            if ui.add_enabled(!self.busy,egui::Button::new(if cfg!(windows){"下一步：系统打印"}else{"确认打印"}).fill(Color32::from_rgb(223,239,251))).clicked(){submit=true}});
        });
        self.print_dialog.open = open;
        if submit || save {
            match printing::page_range(&self.print_dialog.range, doc.pages.len()) {
                Ok(pages) => {
                    let target = if save {
                        rfd::FileDialog::new()
                            .add_filter("PDF 打印文件", &["pdf"])
                            .set_file_name("print.pdf")
                            .save_file()
                    } else {
                        None
                    };
                    if save && target.is_none() {
                        return;
                    }
                    let options = printing::Options {
                        pages,
                        copies: self.print_dialog.copies,
                        dpi: self.print_dialog.dpi,
                        printer: Some(self.print_dialog.printer.clone()),
                    };
                    self.busy = true;
                    self.print_dialog.open = false;
                    self.status = "正在准备打印…".into();
                    let _ = self.tx.send(Command::Print {
                        doc,
                        options,
                        target,
                        source: self.path.clone().unwrap(),
                    });
                }
                Err(e) => self.print_dialog.hint = format!("{e:#}"),
            }
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let title = if self.search_open {
            "查找文字"
        } else {
            match self.tab {
                Tab::Pages => "页面缩略图",
                Tab::Outline => "文档大纲",
                Tab::Attachments => "附件",
                Tab::Checks => "文档检查",
                Tab::Info => "文档信息",
            }
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong().size(14.));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("收起面板").clicked() {
                    self.sidebar_open = false
                }
            });
        });
        ui.add_space(7.);
        ui.separator();
        let Some(doc) = self.doc.clone() else {
            ui.label("打开文档后查看详情");
            return;
        };
        if self.search_open {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("输入要查找的文字")
                    .desired_width(f32::INFINITY),
            );
            if self.search_focus {
                response.request_focus();
                self.search_focus = false;
            }
            if self.query.trim().is_empty() {
                ui.add_space(8.);
                ui.label(
                    RichText::new("支持文档文字层搜索，扫描件需要包含 OCR 文字。").color(MUTED),
                );
                return;
            }
            let q = self.query.to_lowercase();
            let hits: Vec<_> = doc
                .pages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.text.to_lowercase().contains(&q))
                .collect();
            ui.label(RichText::new(format!("在 {} 页找到结果", hits.len())).color(MUTED));
            egui::ScrollArea::vertical()
                .id_salt("search-results")
                .show(ui, |ui| {
                    for (i, p) in hits {
                        egui::Frame::new()
                            .fill(Color32::WHITE)
                            .corner_radius(5)
                            .inner_margin(10.)
                            .show(ui, |ui| {
                                if ui
                                    .selectable_label(i == self.page, format!("第 {} 页", i + 1))
                                    .clicked()
                                {
                                    self.goto(i)
                                }
                                let position = p
                                    .text
                                    .to_lowercase()
                                    .find(&q)
                                    .unwrap_or(0)
                                    .min(p.text.len());
                                let start = p
                                    .text
                                    .char_indices()
                                    .take_while(|(b, _)| *b < position)
                                    .count()
                                    .saturating_sub(14);
                                ui.label(
                                    RichText::new(
                                        p.text
                                            .chars()
                                            .skip(start)
                                            .take(90)
                                            .collect::<String>()
                                            .replace('\n', " "),
                                    )
                                    .size(12.)
                                    .color(MUTED),
                                );
                            });
                        ui.add_space(7.);
                    }
                });
            return;
        }
        match self.tab {
            Tab::Pages => {
                egui::ScrollArea::vertical()
                    .id_salt("thumbnails")
                    .show_rows(ui, 192., doc.pages.len(), |ui, range| {
                        for i in range {
                            ui.vertical_centered(|ui| {
                                let selected = i == self.page;
                                let frame = egui::Frame::new()
                                    .fill(if selected {
                                        Color32::from_rgb(232, 244, 253)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .corner_radius(5)
                                    .inner_margin(10.);
                                frame.show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        if let Some(texture) = self.thumbnails.get(&i) {
                                            let response = ui.add(
                                                egui::Image::from_texture(texture)
                                                    .fit_to_exact_size(texture.size_vec2())
                                                    .sense(egui::Sense::click()),
                                            );
                                            if response.clicked() {
                                                self.goto(i)
                                            }
                                        } else {
                                            let (rect, response) = ui.allocate_exact_size(
                                                Vec2::new(106., 150.),
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(rect, 1., Color32::WHITE);
                                            ui.painter().rect_stroke(
                                                rect,
                                                1.,
                                                Stroke::new(1., LINE),
                                                egui::StrokeKind::Inside,
                                            );
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                (i + 1).to_string(),
                                                egui::FontId::proportional(22.),
                                                MUTED,
                                            );
                                            if response.clicked() {
                                                self.goto(i)
                                            }
                                            if self.thumb_pending.is_empty()
                                                && self.thumb_pending.insert(i)
                                            {
                                                let a = doc.pages[i].area;
                                                let key = Key {
                                                    page: i,
                                                    zoom: ((28. / a.w).min(39. / a.h).min(0.2)
                                                        * 1000.)
                                                        as u32,
                                                    rotation: 0,
                                                    generation: self.generation,
                                                };
                                                let _ = self.tx.send(Command::Render {
                                                    doc: doc.clone(),
                                                    key,
                                                    thumb: true,
                                                });
                                            }
                                        }
                                        if ui
                                            .selectable_label(
                                                selected,
                                                format!("{} / {}", i + 1, doc.pages.len()),
                                            )
                                            .clicked()
                                        {
                                            self.goto(i)
                                        }
                                    });
                                });
                            });
                        }
                    });
            }
            Tab::Outline => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if doc.outlines.is_empty() {
                        ui.label(RichText::new("此文档没有大纲").color(MUTED));
                    }
                    for outline in &doc.outlines {
                        ui.horizontal(|ui| {
                            ui.add_space((outline.depth.min(8) * 12) as f32);
                            if ui
                                .add_enabled(
                                    outline.page.is_some(),
                                    egui::Button::new(&outline.title).wrap().frame(false),
                                )
                                .clicked()
                            {
                                self.goto(outline.page.unwrap())
                            }
                        });
                    }
                });
            }
            Tab::Attachments => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if doc.attachments.is_empty() {
                        ui.label(
                            RichText::new(if doc.is_pdf() {
                                "当前 PDF 版本暂不支持附件读取"
                            } else {
                                "此文档没有附件"
                            })
                            .color(MUTED),
                        );
                    }
                    for attachment in &doc.attachments {
                        ui.label(RichText::new(&attachment.name).strong());
                        ui.small(format!("{} · {} 字节", attachment.format, attachment.size));
                        if ui
                            .add_enabled(doc.export_allowed, egui::Button::new("另存附件"))
                            .clicked()
                        {
                            let name = std::path::Path::new(&attachment.name)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("attachment.bin");
                            if let Some(path) =
                                rfd::FileDialog::new().set_file_name(name).save_file()
                            {
                                self.save_result(
                                    ofd_reader::export::write(
                                        self.path.as_ref().unwrap(),
                                        &path,
                                        &attachment.bytes,
                                    ),
                                    "附件已保存",
                                );
                            }
                        }
                        ui.separator();
                    }
                });
            }
            Tab::Checks => {
                ui.label(
                    RichText::new(if doc.is_pdf() {
                        "PDF 阅读提示"
                    } else {
                        "GB/T 42133—2022"
                    })
                    .strong(),
                );
                ui.small(if doc.is_pdf() {
                    "PDF 不适用 OFD 档案格式检查"
                } else {
                    "静态检查结果需人工复核，不代表合规认证"
                });
                if ui.button("导出报告").clicked() {
                    self.export_report()
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for issue in &doc.issues {
                        egui::Frame::new()
                            .fill(Color32::WHITE)
                            .corner_radius(5)
                            .inner_margin(10.)
                            .show(ui, |ui| {
                                let color = if issue.severity == "error" {
                                    Color32::from_rgb(185, 58, 58)
                                } else if issue.severity == "warning" {
                                    Color32::from_rgb(157, 109, 35)
                                } else {
                                    BLUE
                                };
                                ui.label(RichText::new(&issue.clause).color(color).strong());
                                ui.label(&issue.message);
                                ui.small(RichText::new(&issue.location).color(MUTED));
                            });
                        ui.add_space(8.);
                    }
                    if let Some(cache) = self.cache.get(&self.page) {
                        if let Some(e) = &cache.error {
                            ui.colored_label(Color32::from_rgb(185, 58, 58), e);
                        }
                        for w in &cache.warnings {
                            ui.label(w);
                        }
                    }
                    if doc.issues.is_empty() {
                        ui.label("已实现的静态检查未发现问题。");
                    }
                });
            }
            Tab::Info => {
                if doc.document_count > 1 {
                    let mut index = self.document_index;
                    egui::ComboBox::from_id_salt("documents")
                        .selected_text(format!("文档 {} / {}", index + 1, doc.document_count))
                        .show_ui(ui, |ui| {
                            for i in 0..doc.document_count {
                                ui.selectable_value(&mut index, i, format!("文档 {}", i + 1));
                            }
                        });
                    if index != self.document_index {
                        self.start_page = 1;
                        self.load(self.path.clone().unwrap(), index);
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (k, v) in [
                        ("标题", doc.metadata.title.as_str()),
                        ("作者", &doc.metadata.author),
                        ("生成工具", &doc.metadata.creator),
                        ("创建时间", &doc.metadata.created),
                        ("修改时间", &doc.metadata.modified),
                        ("文档类型", &doc.metadata.doc_type),
                        ("格式版本", &doc.metadata.version),
                    ] {
                        ui.label(RichText::new(k).color(MUTED).size(12.));
                        ui.label(if v.is_empty() { "未提供" } else { v });
                        ui.add_space(8.);
                    }
                    ui.separator();
                    ui.label(format!("{} 页 · 只读阅读", doc.pages.len()));
                    ui.small("默认中文字体：Noto Sans SC（内置）");
                    ui.small(format!("简阅 {}", env!("CARGO_PKG_VERSION")));
                });
            }
        }
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("ribbon")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.icon_header(ui);
                    ui.menu_button("文件", |ui| {
                        if ui.button("打开文档      Ctrl+O").clicked() {
                            ui.close();
                            self.pick()
                        }
                        if ui
                            .add_enabled(
                                self.doc.is_some() && !self.busy,
                                egui::Button::new("打印           Ctrl+P"),
                            )
                            .clicked()
                        {
                            ui.close();
                            self.open_print()
                        }
                        if ui
                            .add_enabled(self.doc.is_some(), egui::Button::new("导出当前页 PNG"))
                            .clicked()
                        {
                            ui.close();
                            self.export_png()
                        }
                        if ui
                            .add_enabled(self.doc.is_some(), egui::Button::new("导出全文文字"))
                            .clicked()
                        {
                            ui.close();
                            self.export_text()
                        }
                        if ui
                            .add_enabled(self.doc.is_some(), egui::Button::new("导出检查报告"))
                            .clicked()
                        {
                            ui.close();
                            self.export_report()
                        }
                    });
                    if widgets::rail(ui, Icon::Open, "打开文档", false).clicked() {
                        self.pick()
                    }
                    if widgets::rail(ui, Icon::Print, "打印", false).clicked() && !self.busy {
                        self.open_print()
                    }
                    ui.separator();
                    let name = self
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or("OFD / PDF 文档阅读器".into());
                    ui.add(egui::Label::new(RichText::new(name).size(13.).color(MUTED)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::rail(
                            ui,
                            Icon::Info,
                            "文档信息",
                            self.sidebar_open && self.tab == Tab::Info,
                        )
                        .clicked()
                        {
                            self.change_tab(Tab::Info)
                        }
                        if ui
                            .add(egui::Button::new("查找文字   Ctrl+F").frame(false))
                            .clicked()
                        {
                            self.search()
                        }
                        for (tab, label) in [
                            (WorkTab::Settings, "设置"),
                            (WorkTab::Edit, "编辑"),
                            (WorkTab::Sign, "签章"),
                            (WorkTab::Convert, "转换"),
                            (WorkTab::Annotate, "注释"),
                            (WorkTab::Read, "阅读"),
                        ] {
                            if ui.selectable_label(self.studio.tab == tab, label).clicked() {
                                self.studio.tab = tab;
                                self.studio.tool = Tool::Select;
                            }
                        }
                    });
                });
                ui.separator();
                if self.studio.tab != WorkTab::Read {
                    self.studio_toolbar(ui, ctx);
                    return;
                }
                let enabled = self.doc.is_some();
                egui::ScrollArea::horizontal()
                    .id_salt("ribbon-scroll")
                    .show(ui, |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                if widgets::tool(ui, Icon::Minus, "缩小", false, enabled).clicked()
                                {
                                    self.fit = Fit::Manual;
                                    self.zoom = (self.zoom / 1.2).max(0.1)
                                }
                                ui.vertical(|ui| {
                                    ui.add_space(7.);
                                    let mut percent = (self.zoom * 100.).round();
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut percent)
                                                .range(10..=400)
                                                .suffix("%")
                                                .speed(1.),
                                        )
                                        .changed()
                                    {
                                        self.fit = Fit::Manual;
                                        self.zoom = percent / 100.;
                                    }
                                    ui.add_space(6.);
                                    ui.label(RichText::new("显示比例").size(12.));
                                });
                                if widgets::tool(ui, Icon::Plus, "放大", false, enabled).clicked()
                                {
                                    self.fit = Fit::Manual;
                                    self.zoom = (self.zoom * 1.2).min(4.)
                                }
                                widgets::divider(ui);
                                if widgets::tool(
                                    ui,
                                    Icon::Actual,
                                    "实际大小",
                                    self.fit == Fit::Manual && (self.zoom - 1.).abs() < 0.005,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.fit = Fit::Manual;
                                    self.zoom = 1.
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Width,
                                    "适合宽度",
                                    self.fit == Fit::Width,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.fit = Fit::Width
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Fit,
                                    "适合页面",
                                    self.fit == Fit::Page,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.fit = Fit::Page
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Fit,
                                    "适合高度",
                                    self.fit == Fit::Height,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.fit = Fit::Height
                                }
                                widgets::divider(ui);
                                if widgets::tool(ui, Icon::Rotate, "顺时针", false, enabled)
                                    .clicked()
                                {
                                    self.rotation = (self.rotation + 90) % 360;
                                    self.scroll_target = Some(self.page)
                                }
                                if widgets::tool(ui, Icon::Rotate, "逆时针", false, enabled)
                                    .clicked()
                                {
                                    self.rotation = (self.rotation + 270) % 360;
                                    self.scroll_target = Some(self.page)
                                }
                                widgets::divider(ui);
                                if widgets::tool(ui, Icon::First, "首页", false, enabled).clicked()
                                {
                                    self.goto(0)
                                }
                                if widgets::tool(ui, Icon::Prev, "上一页", false, enabled).clicked()
                                {
                                    self.goto(self.page.saturating_sub(1))
                                }
                                ui.vertical(|ui| {
                                    ui.add_space(7.);
                                    let count =
                                        self.doc.as_ref().map(|d| d.pages.len()).unwrap_or(1);
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut self.page_input)
                                                .range(1..=count)
                                                .speed(0.1),
                                        )
                                        .changed()
                                    {
                                        self.goto(self.page_input - 1)
                                    }
                                    ui.add_space(6.);
                                    ui.label(RichText::new(format!("共 {count} 页")).size(12.));
                                });
                                if widgets::tool(ui, Icon::Next, "下一页", false, enabled).clicked()
                                {
                                    self.goto(self.page + 1)
                                }
                                if widgets::tool(ui, Icon::Last, "尾页", false, enabled).clicked()
                                    && let Some(doc) = &self.doc
                                {
                                    self.goto(doc.pages.len() - 1)
                                }
                                widgets::divider(ui);
                                if widgets::tool(
                                    ui,
                                    Icon::Continuous,
                                    "连续阅读",
                                    self.continuous,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.continuous = true;
                                    self.scroll_target = Some(self.page)
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Single,
                                    "单页",
                                    !self.continuous,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.continuous = false;
                                    self.scroll_target = Some(self.page)
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Fullscreen,
                                    "全屏",
                                    self.fullscreen,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.fullscreen = !self.fullscreen;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                        self.fullscreen,
                                    ));
                                }
                                widgets::divider(ui);
                                if widgets::tool(
                                    ui,
                                    Icon::Print,
                                    "打印",
                                    false,
                                    enabled && !self.busy,
                                )
                                .clicked()
                                    && !self.busy
                                {
                                    self.open_print()
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Save,
                                    "导出图片",
                                    false,
                                    enabled && !self.busy,
                                )
                                .clicked()
                                    && !self.busy
                                {
                                    self.export_png()
                                }
                                if widgets::tool(
                                    ui,
                                    Icon::Search,
                                    "查找",
                                    self.search_open,
                                    enabled,
                                )
                                .clicked()
                                {
                                    self.search()
                                }
                            });
                        });
                    });
            });
    }
    fn reading(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.doc.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.19).max(45.));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 16., Color32::from_rgb(228, 241, 252));
                widgets::icon(ui.painter(), rect.shrink(18.), Icon::Single, BLUE);
                ui.add_space(22.);
                ui.label(
                    RichText::new(if self.loading {
                        "正在打开文档"
                    } else {
                        "每一页，清晰呈现"
                    })
                    .size(28.)
                    .strong(),
                );
                ui.add_space(10.);
                ui.label(
                    RichText::new("OFD 与 PDF · 连续阅读 · 本地打印")
                        .size(15.)
                        .color(MUTED),
                );
                ui.add_space(28.);
                if self.loading {
                    ui.spinner();
                } else if ui
                    .add(
                        egui::Button::new(
                            RichText::new("打开文档").color(Color32::WHITE).size(15.),
                        )
                        .fill(BLUE)
                        .min_size(Vec2::new(160., 42.)),
                    )
                    .clicked()
                {
                    self.pick()
                }
                ui.add_space(14.);
                ui.small(RichText::new("拖入文档，或按 Ctrl + O").color(MUTED));
                ui.add_space(45.);
                ui.label(
                    RichText::new("内置中文字体     Windows / Linux     文件只读保护")
                        .size(12.)
                        .color(MUTED),
                );
            });
            return;
        };
        let area = doc.pages[self.page].area;
        let (w, h) = if self.rotation % 180 == 90 {
            (area.h, area.w)
        } else {
            (area.w, area.h)
        };
        if self.fit != Fit::Manual {
            let width = if self.continuous {
                doc.pages
                    .iter()
                    .map(|p| {
                        if self.rotation % 180 == 90 {
                            p.area.h
                        } else {
                            p.area.w
                        }
                    })
                    .fold(0f32, f32::max)
            } else {
                w
            };
            let zx = (ui.available_width() - 80.).max(80.) / (width * 96. / 25.4);
            let zy = (ui.available_height() - 48.).max(80.) / (h * 96. / 25.4);
            self.zoom = match self.fit {
                Fit::Width => zx,
                Fit::Page => zx.min(zy),
                Fit::Height => zy,
                Fit::Manual => self.zoom,
            }
            .clamp(0.1, 4.);
        }
        let scale = 96. / 25.4 * self.zoom;
        let spacing = 26.;
        let sizes: Vec<_> = doc
            .pages
            .iter()
            .map(|p| {
                let a = p.area;
                if self.rotation % 180 == 90 {
                    Vec2::new(
                        (a.h * scale * self.dpi).ceil() / self.dpi,
                        (a.w * scale * self.dpi).ceil() / self.dpi,
                    )
                } else {
                    Vec2::new(
                        (a.w * scale * self.dpi).ceil() / self.dpi,
                        (a.h * scale * self.dpi).ceil() / self.dpi,
                    )
                }
            })
            .collect();
        let indices: Vec<_> = if self.continuous {
            (0..doc.pages.len()).collect()
        } else {
            vec![self.page]
        };
        let max_width = indices.iter().map(|i| sizes[*i].x).fold(0f32, f32::max);
        let mut next_top = 12.;
        let mut starts = Vec::with_capacity(indices.len());
        let mut ends = Vec::with_capacity(indices.len());
        for i in &indices {
            starts.push(next_top);
            ends.push(next_top + sizes[*i].y);
            next_top += sizes[*i].y + spacing;
        }
        let total_height = next_top + 12.;
        let content_width = (ui.available_width() - 18.).max(max_width + 64.);
        let mut visible = vec![];
        let mut active = self.page;
        let target = self.scroll_target;
        let target_y = target.map(|page| {
            let top = if self.continuous {
                12. + sizes.iter().take(page).map(|s| s.y + spacing).sum::<f32>()
            } else {
                0.
            };
            top.min((total_height - ui.available_height() + 18.).max(0.))
        });
        let mut reached = false;
        let mut scroll = egui::ScrollArea::both()
            .id_salt(if self.continuous {
                "continuous-reading"
            } else {
                "single-reading"
            })
            .auto_shrink([false, false]);
        if let Some(y) = target_y {
            scroll = scroll.vertical_scroll_offset(y);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            reached = target_y.is_none_or(|y| (viewport.min.y - y).abs() < 3.);
            let (origin, _) = ui
                .allocate_exact_size(Vec2::new(content_width, total_height), egui::Sense::hover());
            let first = ends.partition_point(|end| *end < viewport.min.y - 280.);
            let last = starts
                .partition_point(|top| *top <= viewport.max.y + 280.)
                .max(first);
            let mut closest = f32::INFINITY;
            if let Some(page) = target {
                active = page;
            }
            for position in first..last {
                let i = indices[position];
                let top = starts[position];
                let size = sizes[i];
                let rect = egui::Rect::from_min_size(
                    origin.min + Vec2::new((content_width - size.x) * 0.5, top),
                    size,
                );
                if target == Some(i) {
                    active = i;
                }
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        (rect.min.x * self.dpi).round() / self.dpi,
                        (rect.min.y * self.dpi).round() / self.dpi,
                    ),
                    size,
                );
                let local = rect.translate(-origin.min.to_vec2());
                let distance = (local.center().y - viewport.center().y).abs();
                if target.is_none() && distance < closest {
                    closest = distance;
                    active = i;
                }
                if local.intersects(viewport.expand(280.)) {
                    visible.push(i);
                    let shadow = rect.translate(Vec2::new(0., 2.)).expand(2.);
                    ui.painter()
                        .rect_filled(shadow, 1., Color32::from_black_alpha(18));
                    ui.painter().rect_filled(rect, 0., Color32::WHITE);
                    let key = self.key(i);
                    let cached = self.cache.get(&i).filter(|c| c.key == key);
                    let texture = cached
                        .and_then(|c| c.texture.as_ref())
                        .or_else(|| self.cache.get(&i).and_then(|c| c.texture.as_ref()))
                        .or_else(|| self.thumbnails.get(&i));
                    if let Some(texture) = texture {
                        ui.painter().image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            cached
                                .and_then(|c| c.error.as_deref())
                                .unwrap_or("正在渲染页面…"),
                            egui::FontId::proportional(14.),
                            MUTED,
                        );
                    }
                    self.mark_interaction(ui, i, rect);
                    ui.painter().rect_stroke(
                        rect,
                        0.,
                        Stroke::new(1., Color32::from_rgb(220, 225, 231)),
                        egui::StrokeKind::Outside,
                    );
                    ui.painter().text(
                        rect.center_bottom() + Vec2::new(0., 13.),
                        egui::Align2::CENTER_CENTER,
                        format!("{} / {}", i + 1, doc.pages.len()),
                        egui::FontId::proportional(10.),
                        MUTED,
                    );
                }
            }
        });
        if reached {
            self.scroll_target = None;
        }
        self.live_page
            .store(active, std::sync::atomic::Ordering::Relaxed);
        self.page = active;
        self.page_input = active + 1;
        visible.sort_by_key(|i| i.abs_diff(active));
        let now = ui.input(|i| i.time);
        let moving =
            ui.input(|i| i.raw_scroll_delta.y.abs() > 0.1 || i.smooth_scroll_delta.y.abs() > 0.1);
        if moving {
            self.last_scroll = now;
        }
        for i in visible {
            if self.screenshot.is_some() || now - self.last_scroll > 0.16 {
                self.request(&doc, i);
            } else {
                self.preview(&doc, i);
            }
        }
        if now - self.last_scroll <= 0.16 {
            ui.ctx().request_repaint_after(Duration::from_millis(170));
        }
        self.trim_cache();
    }
}
impl eframe::App for ReaderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.dpi = ctx.pixels_per_point().clamp(0.5, 4.);
        self.poll(ctx);
        self.studio_shortcuts(ctx);
        self.handle_drop(ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O)) {
            self.pick()
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
            self.search()
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P)) && !self.busy {
            self.open_print()
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| {
                i.key_pressed(egui::Key::PageDown) || i.key_pressed(egui::Key::ArrowRight)
            }) {
                self.goto(self.page + 1)
            }
            if ctx
                .input(|i| i.key_pressed(egui::Key::PageUp) || i.key_pressed(egui::Key::ArrowLeft))
            {
                self.goto(self.page.saturating_sub(1))
            }
        }
        let delta = ctx.input(|i| i.zoom_delta());
        if (delta - 1.).abs() > 0.001 {
            self.fit = Fit::Manual;
            self.zoom = (self.zoom * delta).clamp(0.1, 4.);
        }
        self.toolbar(ctx);
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(248, 250, 252))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("‹").clicked() {
                        self.goto(self.page.saturating_sub(1))
                    }
                    ui.label(format!(
                        "{} / {}",
                        self.page + 1,
                        self.doc.as_ref().map(|d| d.pages.len()).unwrap_or(0)
                    ));
                    if ui.small_button("›").clicked() {
                        self.goto(self.page + 1)
                    }
                    ui.separator();
                    if self.loading || self.busy || !self.pending.is_empty() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).size(11.).color(MUTED));
                    if let Some(doc) = &self.doc {
                        if doc.metadata.doc_type == "OFD-A" {
                            ui.label(RichText::new("OFD-A · 只读").color(BLUE).size(11.));
                        }
                        if let Some(cache) = self.cache.get(&self.page) {
                            let n = cache.warnings.len() + usize::from(cache.error.is_some());
                            if n > 0 && ui.small_button(format!("{n} 项渲染提示")).clicked() {
                                self.sidebar_open = true;
                                self.tab = Tab::Checks;
                                self.search_open = false;
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("内置中文字体").size(11.).color(MUTED));
                        let mut percent = self.zoom * 100.;
                        if ui
                            .add(
                                egui::Slider::new(&mut percent, 10.0..=400.)
                                    .show_value(false)
                                    .logarithmic(true),
                            )
                            .changed()
                        {
                            self.fit = Fit::Manual;
                            self.zoom = percent / 100.
                        }
                        ui.label(format!("{:.0}%", self.zoom * 100.));
                    });
                });
            });
        egui::SidePanel::left("navigation-rail")
            .exact_width(43.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(248, 250, 252))
                    .inner_margin(egui::Margin::symmetric(3, 10)),
            )
            .show(ctx, |ui| {
                for (tab, icon, label) in [
                    (Tab::Outline, Icon::Outline, "文档大纲"),
                    (Tab::Pages, Icon::Image, "页面缩略图"),
                    (Tab::Attachments, Icon::Attach, "附件"),
                    (Tab::Checks, Icon::Check, "文档检查"),
                    (Tab::Info, Icon::Info, "文档信息"),
                ] {
                    if widgets::rail(
                        ui,
                        icon,
                        label,
                        self.sidebar_open && self.tab == tab && !self.search_open,
                    )
                    .clicked()
                    {
                        self.change_tab(tab)
                    }
                    ui.add_space(3.);
                }
                ui.separator();
                if widgets::rail(ui, Icon::Search, "查找文字", self.search_open).clicked() {
                    self.search()
                }
            });
        if self.sidebar_open {
            egui::SidePanel::left("details")
                .default_width(236.)
                .min_width(210.)
                .max_width(410.)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(248, 250, 252))
                        .inner_margin(12.),
                )
                .show(ctx, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(243, 245, 248))
                    .inner_margin(egui::Margin::symmetric(4, 8)),
            )
            .show(ctx, |ui| self.reading(ui));
        self.print_window(ctx);
        self.studio_windows(ctx);
        self.drop_overlay(ctx);
        self.apply_staged();
        if let Some(error) = self.error.clone() {
            egui::Window::new("提示")
                .collapsible(false)
                .default_width(420.)
                .show(ctx, |ui| {
                    ui.label(error);
                    if ui.button("关闭").clicked() {
                        self.error = None;
                    }
                });
        }
        if self.screenshot.is_some()
            && !self.screenshot_requested
            && !self.busy
            && self
                .cache
                .get(&self.page)
                .is_some_and(|c| c.key == self.key(self.page) && c.texture.is_some())
            && self.scroll_target.is_none()
        {
            self.ready_frames += 1;
            if self.ready_frames >= 3 {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let images = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for image in images {
            if let Some(path) = self.screenshot.take() {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    path,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("截图保存失败: {e}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.loading || self.busy || !self.pending.is_empty() || !self.thumb_pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn app() -> ReaderApp {
        let (tx, _) = mpsc::channel();
        let (_, rx) = mpsc::channel();
        let doc = Arc::new(Document::from_ofd(
            ofd_reader::parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap(),
        ));
        ReaderApp {
            tx,
            rx,
            doc: Some(doc),
            path: Some(PathBuf::from("demo.ofd")),
            document_index: 0,
            generation: 1,
            page: 0,
            page_input: 1,
            start_page: 1,
            zoom: 1.,
            rotation: 0,
            fit: Fit::Manual,
            dpi: 1.,
            continuous: true,
            fullscreen: false,
            cache: BTreeMap::new(),
            pending: BTreeMap::new(),
            thumbnails: BTreeMap::new(),
            thumb_pending: BTreeSet::new(),
            tab: Tab::Pages,
            sidebar_open: false,
            query: String::new(),
            search_open: false,
            search_focus: false,
            scroll_target: None,
            loading: false,
            busy: false,
            status: String::new(),
            error: None,
            print_dialog: PrintDialog::default(),
            screenshot: None,
            screenshot_requested: false,
            ready_frames: 0,
            studio: Studio::default(),
            last_scroll: 0.,
            live_page: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            live_generation: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }
    fn frame(ctx: &egui::Context, app: &mut ReaderApp, time: f64, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000., 760.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.reading(ui));
        });
    }
    #[test]
    fn wheel_scrolls_continuous_reader_across_pages() {
        let ctx = egui::Context::default();
        let mut app = app();
        frame(&ctx, &mut app, 0., vec![]);
        for i in 1..=12 {
            frame(
                &ctx,
                &mut app,
                i as f64 / 30.,
                vec![
                    egui::Event::PointerMoved(egui::pos2(500., 350.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0., -95.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.page >= 1, "wheel did not reach next page: {}", app.page);
        assert_eq!(app.page_input, app.page + 1);
        assert!(app.pending.len() <= 2);
    }
    #[test]
    fn jump_targets_page_in_continuous_and_single_modes() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.goto(2);
        frame(&ctx, &mut app, 0., vec![]);
        frame(&ctx, &mut app, 0.1, vec![]);
        assert_eq!(app.page, 2);
        app.continuous = false;
        app.goto(1);
        frame(&ctx, &mut app, 0.2, vec![]);
        assert_eq!(app.page, 1);
        assert!(app.scroll_target.is_none());
    }
    #[test]
    fn render_cache_stays_bounded_and_keeps_current_page() {
        let mut app = app();
        app.page = 1;
        for page in 0..6 {
            app.cache.insert(
                page,
                Cached {
                    key: app.key(page),
                    texture: None,
                    size: 24 * 1024 * 1024,
                    warnings: vec![],
                    error: None,
                },
            );
        }
        app.trim_cache();
        assert!(app.cache.contains_key(&1));
        assert!(app.cache.values().map(|c| c.size).sum::<usize>() <= 64 * 1024 * 1024);
    }
    #[test]
    fn long_document_scroll_visits_only_nearby_page_shapes() {
        let mut app = app();
        let mut model =
            ofd_reader::parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
        let first = model.pages[0].clone();
        model.pages = (0..500)
            .map(|i| {
                let mut p = first.clone();
                p.id = (i + 1).to_string();
                p
            })
            .collect();
        app.doc = Some(Arc::new(Document::from_ofd(model)));
        let ctx = egui::Context::default();
        let mut samples = vec![];
        for i in 0..120 {
            let now = std::time::Instant::now();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000., 760.),
                )),
                time: Some(i as f64 / 60.),
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(500., 350.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0., -90.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.reading(ui));
            });
            assert!(
                output.shapes.len() < 100,
                "off-screen pages should not generate drawing commands"
            );
            assert!(app.pending.len() <= 2);
            samples.push(now.elapsed().as_secs_f64() * 1000.);
        }
        assert!(app.page > 2);
        samples.sort_by(|a, b| a.total_cmp(b));
        println!(
            "500-page headless layout: median {:.3} ms, p95 {:.3} ms (excludes rasterization/GPU)",
            samples[60], samples[114]
        );
    }
}
