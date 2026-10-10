//! Appearance conversion: reconstructs pages, without claiming semantic preservation.
use crate::{document::Document, render::Renderer};
use anyhow::{Result, ensure};
use std::io::{Cursor, Write};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Pdf,
    Ofd,
    PngZip,
}
#[derive(Clone, Debug)]
pub struct Options {
    pub format: Format,
    pub pages: Vec<usize>,
    pub dpi: u32,
}
pub fn convert(doc: &Document, options: &Options) -> Result<Vec<u8>> {
    ensure!(doc.export_allowed, "文档权限禁止转换和导出");
    ensure!(
        !options.pages.is_empty()
            && options.pages.len() <= 2000
            && options.pages.iter().all(|p| *p < doc.pages.len()),
        "转换页面范围无效"
    );
    ensure!(
        (72..=300).contains(&options.dpi),
        "转换分辨率应为 72–300 DPI"
    );
    let mut writer = if options.format == Format::Pdf {
        None
    } else {
        Some(zip::ZipWriter::new(Cursor::new(vec![])))
    };
    let mut pdf = pdf_writer::Pdf::new();
    let mut renderer = Renderer::new();
    let mut count = 0usize;
    use pdf_writer::{Content, Filter, Finish, Name, Rect, Ref};
    let ids: Vec<_> = (0..options.pages.len())
        .map(|i| Ref::new(3 + i as i32 * 3))
        .collect();
    if options.format == Format::Pdf {
        pdf.catalog(Ref::new(1)).pages(Ref::new(2));
        pdf.pages(Ref::new(2))
            .kids(ids.iter().copied())
            .count(ids.len() as i32);
    }
    let mut page_refs = String::new();
    let mut image_refs = String::new();
    for (i, index) in options.pages.iter().copied().enumerate() {
        let a = doc.pages[index].area;
        let pixels96 = a.w * a.h * (96f32 / 25.4).powi(2);
        let zoom = (options.dpi as f32 / 96.)
            .min((15_000_000. / pixels96).sqrt())
            .min(16000. / (a.w.max(a.h) * 96. / 25.4));
        let r = doc.render(&mut renderer, index, zoom, 0)?;
        ensure!(
            r.warnings.is_empty(),
            "第 {} 页存在渲染提示，已停止转换以避免内容缺失：{}",
            index + 1,
            r.warnings.join("；")
        );
        let image = r.pixmap;
        if let Some(zip) = &mut writer {
            let png = image.encode_png()?;
            count += png.len();
            ensure!(count <= 192 * 1024 * 1024, "转换结果过大，请分批转换");
            ensure!(
                png.len() <= 32 * 1024 * 1024,
                "单页图像超过 OFD 条目限制，请降低 DPI"
            );
            if options.format == Format::PngZip {
                entry(zip, &format!("page-{:04}.png", i + 1), &png)?;
            } else {
                let res_id = 10000 + i;
                entry(zip, &format!("Doc_0/Res/page-{i}.png"), &png)?;
                let page = wrap(
                    "Page",
                    &format!(
                        "<ofd:Area><ofd:PhysicalBox>0 0 {} {}</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer ID=\"{}\"><ofd:ImageObject ID=\"{}\" Boundary=\"0 0 {} {}\" CTM=\"{} 0 0 {} 0 0\" ResourceID=\"{res_id}\" Interpolate=\"true\"/></ofd:Layer></ofd:Content>",
                        a.w,
                        a.h,
                        20000 + i * 2,
                        20001 + i * 2,
                        a.w,
                        a.h,
                        a.w,
                        a.h
                    ),
                    "",
                );
                entry(
                    zip,
                    &format!("Doc_0/Pages/Page_{i}/Content.xml"),
                    page.as_bytes(),
                )?;
                page_refs.push_str(&format!(
                    "<ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{i}/Content.xml\"/>",
                    i + 1
                ));
                image_refs.push_str(&format!("<ofd:MultiMedia ID=\"{res_id}\" Type=\"Image\" Format=\"PNG\"><ofd:MediaFile>page-{i}.png</ofd:MediaFile></ofd:MultiMedia>"));
            }
        } else {
            let mut compressor =
                flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
            for px in image.data().chunks_exact(4) {
                compressor.write_all(&px[..3])?;
            }
            let data = compressor.finish()?;
            count += data.len();
            ensure!(
                count <= 96 * 1024 * 1024,
                "PDF 转换结果超过限制，请分批转换"
            );
            let page = ids[i];
            let id = Ref::new(page.get() + 1);
            let stream = Ref::new(page.get() + 2);
            let w = a.w * 72. / 25.4;
            let h = a.h * 72. / 25.4;
            let mut p = pdf.page(page);
            p.parent(Ref::new(2))
                .media_box(Rect::new(0., 0., w, h))
                .contents(stream);
            p.resources().x_objects().pair(Name(b"Page"), id);
            p.finish();
            let mut x = pdf.image_xobject(id, &data);
            x.filter(Filter::FlateDecode);
            x.width(image.width() as i32)
                .height(image.height() as i32)
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
    }
    if let Some(mut zip) = writer {
        if options.format == Format::Ofd {
            let first = doc.pages[options.pages[0]].area;
            let title = xml_escape(&doc.metadata.title);
            let root = wrap(
                "OFD",
                &format!(
                    "<ofd:DocBody><ofd:DocInfo><ofd:DocID>reader-converted</ofd:DocID><ofd:Title>{title}</ofd:Title><ofd:Creator>简阅 {}</ofd:Creator></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody>",
                    env!("CARGO_PKG_VERSION")
                ),
                "Version=\"1.0\" DocType=\"OFD\"",
            );
            entry(&mut zip, "OFD.xml", root.as_bytes())?;
            let document = wrap(
                "Document",
                &format!(
                    "<ofd:CommonData><ofd:MaxUnitID>999999</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 {} {}</ofd:PhysicalBox></ofd:PageArea><ofd:DocumentRes>DocumentRes.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages>{page_refs}</ofd:Pages>",
                    first.w, first.h
                ),
                "",
            );
            entry(&mut zip, "Doc_0/Document.xml", document.as_bytes())?;
            entry(
                &mut zip,
                "Doc_0/DocumentRes.xml",
                wrap(
                    "Res",
                    &format!("<ofd:MultiMedias>{image_refs}</ofd:MultiMedias>"),
                    "BaseLoc=\"Res\"",
                )
                .as_bytes(),
            )?;
        }
        Ok(zip.finish()?.into_inner())
    } else {
        Ok(pdf.finish())
    }
}
fn entry(zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) -> Result<()> {
    zip.start_file(
        name,
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    zip.write_all(bytes)?;
    Ok(())
}
fn wrap(name: &str, body: &str, attributes: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ofd:{name} xmlns:ofd=\"http://www.ofdspec.org/2016\" {attributes}>{body}</ofd:{name}>"
    )
}
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
}
