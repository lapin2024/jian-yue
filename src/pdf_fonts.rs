//! Repairs only a render copy for unembedded BMP Unicode Chinese CID fonts.
//! Original document bytes, extracted text, and signature data are never written back.
use crate::model::Issue;
use anyhow::{Context, Result};
use lopdf::{Dictionary, Object, Stream, dictionary};

pub fn prepare(
    original: &lopdf::Document,
    bytes: &[u8],
) -> Result<(Vec<u8>, Vec<Issue>, Vec<String>)> {
    let mut work = original.clone();
    let mut issues = vec![];
    let mut limitations = vec![];
    let mut replacements = vec![];
    for (id, obj) in &original.objects {
        let Ok(font) = obj.as_dict() else { continue };
        if font.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font") {
            continue;
        }
        if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type0") {
            continue;
        }
        let Some(desc) = font
            .get(b"DescendantFonts")
            .ok()
            .and_then(|a| a.as_array().ok())
            .and_then(|a| a.first())
            .and_then(|o| original.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
        else {
            continue;
        };
        let embedded = desc
            .get(b"FontDescriptor")
            .ok()
            .and_then(|o| original.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
            .is_some_and(|f| {
                [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                    .iter()
                    .any(|name| f.has(name))
            });
        if embedded {
            continue;
        }
        let encoding = font
            .get(b"Encoding")
            .and_then(Object::as_name)
            .ok()
            .unwrap_or(b"unknown");
        let name = font
            .get(b"BaseFont")
            .and_then(Object::as_name)
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .unwrap_or_default();
        if matches!(encoding, b"UniGB-UCS2-H" | b"UniGB-UTF16-H") {
            replacements.push((*id, font.clone(), desc.clone()));
            issues.push(Issue{severity:"info".into(),clause:"PDF".into(),location:format!("字体 {name}"),message:"未嵌入中文字体；在内存中的阅读副本使用内置 Noto Sans SC 替代，原文件保持不变。替代字体的字形可能与原字体不同。".into()});
        } else {
            let msg = format!(
                "PDF 字体 {name} 未嵌入，编码 {} 暂不支持替代，相关文字可能缺失",
                String::from_utf8_lossy(encoding)
            );
            limitations.push(msg.clone());
            issues.push(Issue {
                severity: "warning".into(),
                clause: "PDF".into(),
                location: format!("字体对象 {}", id.0),
                message: msg,
            });
        }
    }
    if replacements.is_empty() {
        return Ok((bytes.to_vec(), issues, limitations));
    }
    let face = ttf_parser::Face::parse(crate::fonts::DEFAULT_CJK, 0).context("内置中文字体损坏")?;
    let mut mapping = vec![0u8; 65536 * 2];
    let mut widths: Vec<Object> = vec![];
    for value in 0..=65535u32 {
        if let Some(glyph) = char::from_u32(value).and_then(|c| face.glyph_index(c)) {
            let width = face.glyph_hor_advance(glyph).unwrap_or(face.units_per_em()) as i64 * 1000
                / face.units_per_em() as i64;
            if width != 1000 {
                widths.push(Object::Integer(value as i64));
                widths.push(Object::Array(vec![Object::Integer(width)]));
            }
            mapping[value as usize * 2..value as usize * 2 + 2]
                .copy_from_slice(&glyph.0.to_be_bytes());
        }
    }
    let font_file = work.add_object(Stream::new(
        dictionary! {"Length1"=>crate::fonts::DEFAULT_CJK.len()as i64},
        crate::fonts::DEFAULT_CJK.to_vec(),
    ));
    let gid_map = work.add_object(Stream::new(Dictionary::new(), mapping));
    let descriptor=work.add_object(dictionary!{"Type"=>"FontDescriptor","FontName"=>"NotoSansSC-Regular","Flags"=>4,"FontBBox"=>vec![Object::Integer(-1000),Object::Integer(-1000),Object::Integer(2000),Object::Integer(2000)],"ItalicAngle"=>0,"Ascent"=>1160,"Descent"=>-288,"CapHeight"=>733,"StemV"=>80,"FontFile2"=>font_file});
    for (id, mut font, mut desc) in replacements {
        desc.set("Type", "Font");
        desc.set("Subtype", "CIDFontType2");
        desc.set("BaseFont", "NotoSansSC-Regular");
        desc.set("CIDSystemInfo",dictionary!{"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0});
        desc.set("FontDescriptor", descriptor);
        desc.set("CIDToGIDMap", gid_map);
        // In a UniGB-UCS2/UTF16 PDF content codes are BMP Unicode; remapping the
        // render copy to Identity-H lets the explicit CIDToGIDMap select glyphs.
        // Original W tables are keyed by Adobe-GB1 CID and must not be reused.
        desc.remove(b"W");
        desc.remove(b"W2");
        desc.set("DW", 1000);
        desc.set("W", Object::Array(widths.clone()));
        let desc_id = work.add_object(desc);
        font.set("DescendantFonts", vec![Object::Reference(desc_id)]);
        font.set("Encoding", "Identity-H");
        work.objects.insert(id, Object::Dictionary(font));
    }
    let mut result = vec![];
    work.save_to(&mut result)?;
    Ok((result, issues, limitations))
}
