use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Matrix(pub [f32; 6]);
impl Default for Matrix {
    fn default() -> Self {
        Self([1., 0., 0., 1., 0., 0.])
    }
}
impl Matrix {
    /// self * rhs, using column vectors in the OFD coordinate system.
    pub fn concat(self, rhs: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    pub fn translate(x: f32, y: f32) -> Self {
        Self([1., 0., 0., 1., x, y])
    }
    pub fn scale(x: f32, y: f32) -> Self {
        Self([x, 0., 0., y, 0., 0.])
    }
}
#[derive(Clone, Debug)]
pub struct Style {
    pub fill: [u8; 4],
    pub stroke: [u8; 4],
    pub width: f32,
    pub dash: Vec<f32>,
    pub dash_offset: f32,
    pub cap: String,
    pub join: String,
    pub miter: f32,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            width: 0.353,
            dash: vec![],
            dash_offset: 0.,
            cap: "Butt".into(),
            join: "Miter".into(),
            miter: 3.528,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Clip {
    pub path: String,
    pub transform: Matrix,
    pub even_odd: bool,
}
#[derive(Clone, Debug)]
pub struct Graphic {
    pub id: String,
    pub bounds: Rect,
    pub transform: Matrix,
    pub opacity: f32,
    pub style: Style,
    pub clips: Vec<Vec<Clip>>,
    pub kind: Kind,
}
#[derive(Clone, Debug)]
pub enum Kind {
    Text {
        font: String,
        size: f32,
        hscale: f32,
        fill: bool,
        stroke: bool,
        codes: Vec<TextRun>,
    },
    Path {
        data: String,
        fill: bool,
        stroke: bool,
        even_odd: bool,
    },
    Image {
        resource: String,
        interpolate: bool,
    },
    Group(Vec<Graphic>),
}
#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub dx: Vec<f32>,
    pub dy: Vec<f32>,
    pub glyphs: Vec<u16>,
}
#[derive(Clone, Debug)]
pub struct Font {
    pub name: String,
    pub family: String,
    pub bytes: Option<Arc<Vec<u8>>>,
    pub bold: bool,
    pub italic: bool,
}
#[derive(Clone, Default, Debug)]
pub struct Resources {
    pub fonts: BTreeMap<String, Font>,
    pub images: BTreeMap<String, Arc<Vec<u8>>>,
    pub vectors: BTreeMap<String, String>,
    pub draws: BTreeMap<String, Style>,
    pub colors: BTreeMap<String, ColorSpace>,
    pub default_cs: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ColorSpace {
    pub kind: String,
    pub bits: u32,
    pub palette: Vec<Vec<f32>>,
}
#[derive(Clone, Debug)]
pub struct Page {
    pub id: String,
    pub area: Rect,
    pub objects: Vec<Graphic>,
    pub resources: Resources,
    pub text: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Issue {
    pub severity: String,
    pub clause: String,
    pub location: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Outline {
    pub title: String,
    pub page: Option<usize>,
    pub depth: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Attachment {
    pub name: String,
    pub path: String,
    pub format: String,
    pub size: usize,
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub creator: String,
    pub created: String,
    pub modified: String,
    pub doc_id: String,
    pub doc_type: String,
    pub version: String,
}
#[derive(Clone, Debug)]
pub struct OfdDocument {
    pub metadata: Metadata,
    pub pages: Vec<Page>,
    pub outlines: Vec<Outline>,
    pub attachments: Vec<Attachment>,
    pub issues: Vec<Issue>,
    pub document_count: usize,
    pub signature_count: usize,
    pub copy_allowed: bool,
    pub export_allowed: bool,
    pub print_allowed: bool,
}
impl OfdDocument {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"reader_version":env!("CARGO_PKG_VERSION"),"standard":"GB/T 42133-2022", "scope":"静态格式检查，非合规认证；数字签名未验签", "metadata":self.metadata,"pages":self.pages.len(),"document_count":self.document_count,"signature_count":self.signature_count,"attachments":self.attachments,"issues":self.issues})
    }
}
