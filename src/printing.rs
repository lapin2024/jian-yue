//! Printing uses the same page rasterization as the reader (150 DPI by default).
use crate::{document::Document, render::Renderer};
#[cfg(target_os = "linux")]
use anyhow::Context;
#[cfg(any(windows, not(any(windows, target_os = "linux"))))]
use anyhow::bail;
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, io::Write};

#[derive(Clone, Debug)]
pub struct Options {
    pub pages: Vec<usize>,
    pub copies: u16,
    pub dpi: u32,
    pub printer: Option<String>,
}
pub fn page_range(input: &str, total: usize) -> Result<Vec<usize>> {
    ensure!(total > 0, "文档没有页面");
    if input.trim().is_empty() {
        return Ok((0..total).collect());
    }
    let mut pages = BTreeSet::new();
    for part in input.split([',', '，']) {
        let part = part.trim();
        ensure!(!part.is_empty(), "页码范围中有空项");
        let (a, b) = if let Some((a, b)) = part.split_once('-') {
            (a.trim().parse::<usize>()?, b.trim().parse::<usize>()?)
        } else {
            let n = part.parse::<usize>()?;
            (n, n)
        };
        ensure!(
            a >= 1 && a <= b && b <= total,
            "页码范围无效，应位于 1 至 {total} 之间"
        );
        pages.extend(a - 1..b);
    }
    Ok(pages.into_iter().collect())
}
fn validate(doc: &Document, options: &Options) -> Result<()> {
    ensure!(doc.print_allowed, "文档权限禁止打印");
    ensure!(
        !options.pages.is_empty() && options.pages.len() <= 2000,
        "打印页数无效"
    );
    ensure!((1..=99).contains(&options.copies), "打印份数应为 1–99");
    ensure!(
        (72..=300).contains(&options.dpi),
        "打印分辨率应为 72–300 DPI"
    );
    ensure!(
        options.pages.iter().all(|p| *p < doc.pages.len()),
        "打印页码超出范围"
    );
    Ok(())
}
fn zoom(doc: &Document, index: usize, dpi: u32) -> f32 {
    let a = doc.pages[index].area;
    let at96 = a.w * a.h * (96f32 / 25.4).powi(2);
    (dpi as f32 / 96.)
        .min((15_000_000. / at96).sqrt())
        .min(16000. / (a.w.max(a.h) * 96. / 25.4))
}
/// Builds a raster print PDF. Does not contact or submit to any printer.
pub fn print_pdf(doc: &Document, options: &Options) -> Result<Vec<u8>> {
    validate(doc, options)?;
    use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
    let mut pdf = Pdf::new();
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    let ids: Vec<_> = (0..options.pages.len())
        .map(|i| Ref::new(3 + i as i32 * 3))
        .collect();
    pdf.pages(Ref::new(2))
        .kids(ids.iter().copied())
        .count(ids.len() as i32);
    let mut renderer = Renderer::new();
    let mut total = 0usize;
    for (i, index) in options.pages.iter().copied().enumerate() {
        let rendered = doc.render(&mut renderer, index, zoom(doc, index, options.dpi), 0)?;
        ensure!(
            rendered.warnings.is_empty(),
            "第 {} 页存在渲染提示，无法保证打印内容完整：{}",
            index + 1,
            rendered.warnings.join("；")
        );
        let im = rendered.pixmap;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        for pixel in im.data().chunks_exact(4) {
            encoder.write_all(&pixel[..3])?;
        }
        let data = encoder.finish()?;
        total += data.len();
        ensure!(
            total <= 256 * 1024 * 1024,
            "打印任务超过 256 MiB，请分批打印"
        );
        let page = ids[i];
        let image = Ref::new(page.get() + 1);
        let stream = Ref::new(page.get() + 2);
        let a = doc.pages[index].area;
        let w = a.w * 72. / 25.4;
        let h = a.h * 72. / 25.4;
        let mut p = pdf.page(page);
        p.parent(Ref::new(2))
            .media_box(Rect::new(0., 0., w, h))
            .contents(stream);
        p.resources().x_objects().pair(Name(b"Page"), image);
        p.finish();
        let mut x = pdf.image_xobject(image, &data);
        x.filter(Filter::FlateDecode);
        x.width(im.width() as i32)
            .height(im.height() as i32)
            .bits_per_component(8);
        x.color_space().device_rgb();
        x.finish();
        let mut c = Content::new();
        c.save_state()
            .transform([w, 0., 0., h, 0., 0.])
            .x_object(Name(b"Page"))
            .restore_state();
        pdf.stream(stream, &c.finish());
    }
    Ok(pdf.finish())
}
#[cfg(target_os = "linux")]
pub fn printers() -> Result<Vec<String>> {
    let out = std::process::Command::new("lpstat")
        .args(["-a"])
        .output()
        .context("未找到 CUPS lpstat，请安装 cups-client 并配置打印机")?;
    ensure!(
        out.status.success(),
        "无法查询打印队列: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}
#[cfg(not(target_os = "linux"))]
pub fn printers() -> Result<Vec<String>> {
    Ok(vec![])
}
#[cfg(target_os = "linux")]
pub fn print(doc: &Document, options: &Options) -> Result<String> {
    let bytes = print_pdf(doc, options)?;
    let mut file = tempfile::Builder::new()
        .prefix("jian-yue-print-")
        .suffix(".pdf")
        .tempfile()?;
    file.write_all(&bytes)?;
    file.flush()?;
    let mut command = std::process::Command::new("lp");
    command
        .arg("-n")
        .arg(options.copies.to_string())
        .arg("-o")
        .arg("fit-to-page");
    if let Some(printer) = options.printer.as_deref().filter(|s| !s.is_empty()) {
        ensure!(!printer.starts_with('-'), "无效打印机名称");
        command.arg("-d").arg(printer);
    }
    command.arg(file.path());
    let out = command
        .output()
        .context("未找到 CUPS lp，请安装 cups-client 并配置打印机")?;
    ensure!(
        out.status.success(),
        "提交打印失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(format!(
        "已提交打印队列：{}",
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}
#[cfg(windows)]
pub fn print(doc: &Document, options: &Options) -> Result<String> {
    validate(doc, options)?;
    use windows_sys::Win32::{
        Foundation::GlobalFree, Graphics::Gdi::*, Storage::Xps::*, UI::Controls::Dialogs::*,
    };
    struct Handles(PRINTDLGW);
    impl Drop for Handles {
        fn drop(&mut self) {
            unsafe {
                if !self.0.hDC.is_null() {
                    DeleteDC(self.0.hDC);
                }
                if !self.0.hDevMode.is_null() {
                    GlobalFree(self.0.hDevMode);
                }
                if !self.0.hDevNames.is_null() {
                    GlobalFree(self.0.hDevNames);
                }
            }
        }
    }
    let mut dialog: PRINTDLGW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<PRINTDLGW>() as u32;
    dialog.Flags = PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION;
    dialog.nCopies = options.copies;
    dialog.nMinPage = 1;
    dialog.nMaxPage = doc.pages.len() as u16;
    let ok = unsafe { PrintDlgW(&mut dialog) };
    let handles = Handles(dialog);
    if ok == 0 {
        let error = unsafe { CommDlgExtendedError() };
        if error == 0 {
            return Ok("已取消打印".into());
        }
        bail!("系统打印窗口失败，错误代码 0x{error:08X}")
    }
    let dc = handles.0.hDC;
    ensure!(!dc.is_null(), "系统未返回打印设备");
    let title: Vec<u16> = "简阅 document".encode_utf16().chain(Some(0)).collect();
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: title.as_ptr(),
        lpszOutput: std::ptr::null(),
        lpszDatatype: std::ptr::null(),
        fwType: 0,
    };
    let mut renderer = Renderer::new();
    let copies = handles.0.nCopies.max(1);
    ensure!(copies <= 99, "打印份数不能超过 99");
    // Validate every page before starting a job; an unsupported later page must
    // not cause a partially submitted document.
    for index in &options.pages {
        let r = doc.render(&mut renderer, *index, zoom(doc, *index, options.dpi), 0)?;
        ensure!(
            r.warnings.is_empty(),
            "第 {} 页存在渲染提示，未提交打印：{}",
            index + 1,
            r.warnings.join("；")
        );
    }
    ensure!(unsafe { StartDocW(dc, &info) } > 0, "无法启动打印任务");
    let result = (|| -> Result<()> {
        for _ in 0..copies {
            for index in &options.pages {
                let rendered =
                    doc.render(&mut renderer, *index, zoom(doc, *index, options.dpi), 0)?;
                ensure!(
                    rendered.warnings.is_empty(),
                    "第 {} 页存在渲染提示，已中止打印：{}",
                    index + 1,
                    rendered.warnings.join("；")
                );
                let image = rendered.pixmap;
                let mut bgra = image.data().to_vec();
                for pixel in bgra.chunks_exact_mut(4) {
                    pixel.swap(0, 2)
                }
                let mut bitmap: BITMAPINFO = unsafe { std::mem::zeroed() };
                bitmap.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bitmap.bmiHeader.biWidth = image.width() as i32;
                bitmap.bmiHeader.biHeight = -(image.height() as i32);
                bitmap.bmiHeader.biPlanes = 1;
                bitmap.bmiHeader.biBitCount = 32;
                bitmap.bmiHeader.biCompression = BI_RGB;
                let pw = unsafe { GetDeviceCaps(dc, HORZRES as i32) };
                let ph = unsafe { GetDeviceCaps(dc, VERTRES as i32) };
                ensure!(pw > 0 && ph > 0, "打印机页面尺寸无效");
                let scale =
                    (pw as f32 / image.width() as f32).min(ph as f32 / image.height() as f32);
                let dw = (image.width() as f32 * scale) as i32;
                let dh = (image.height() as f32 * scale) as i32;
                ensure!(unsafe { StartPage(dc) } > 0, "无法开始打印页");
                unsafe {
                    SetStretchBltMode(dc, HALFTONE);
                }
                let painted = unsafe {
                    StretchDIBits(
                        dc,
                        (pw - dw) / 2,
                        (ph - dh) / 2,
                        dw,
                        dh,
                        0,
                        0,
                        image.width() as i32,
                        image.height() as i32,
                        bgra.as_ptr().cast(),
                        &bitmap,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    )
                };
                ensure!(painted > 0, "页面写入打印设备失败");
                ensure!(unsafe { EndPage(dc) } > 0, "结束打印页失败");
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        unsafe {
            AbortDoc(dc);
        }
        return Err(e);
    }
    ensure!(unsafe { EndDoc(dc) } > 0, "打印任务提交失败");
    Ok("打印任务已提交到系统队列".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn print(_: &Document, _: &Options) -> Result<String> {
    bail!("当前系统不支持打印")
}
