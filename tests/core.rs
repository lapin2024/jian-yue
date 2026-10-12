use ofd_reader::{
    archive::{Package, resolve},
    model::*,
    parser,
    render::{Renderer, ofd_path},
};
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

fn archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in entries {
        z.start_file(
            *n,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn b(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}
fn fixture(
    objects: &str,
    extra_document: &str,
    extra_resources: &str,
    files: &[(&str, Vec<u8>)],
) -> Vec<u8> {
    let mut entries = vec![
        (
            "OFD.xml",
            b(
                r#"<OFD xmlns="http://www.ofdspec.org/2016" Version="1.0" DocType="OFD-A"><DocBody><DocInfo><Title>测试文档</Title></DocInfo><DocRoot>Doc_0/Document.xml</DocRoot></DocBody></OFD>"#,
            ),
        ),
        (
            "Doc_0/Document.xml",
            b(&format!(
                r#"<Document><CommonData><PageArea><PhysicalBox>0 0 40 30</PhysicalBox></PageArea><PublicRes>PublicRes.xml</PublicRes></CommonData><Pages><Page ID="1" BaseLoc="Pages/Content.xml"/></Pages>{extra_document}</Document>"#
            )),
        ),
        (
            "Doc_0/PublicRes.xml",
            b(&format!(r#"<Res BaseLoc="Res">{extra_resources}</Res>"#)),
        ),
        (
            "Doc_0/Pages/Content.xml",
            b(&format!(
                r#"<Page><Content><Layer ID="2">{objects}</Layer></Content></Page>"#
            )),
        ),
    ];
    entries.extend_from_slice(files);
    archive(&entries)
}
fn red_path() -> &'static str {
    r#"<PathObject ID="3" Boundary="5 5 10 10" Fill="true" Stroke="false"><FillColor Value="255 0 0"/><AbbreviatedData>M 0 0 L 10 0 L 10 10 L 0 10 C</AbbreviatedData></PathObject>"#
}
fn pixel(image: &tiny_skia::Pixmap, x: f32, y: f32) -> [u8; 4] {
    let px = (x * 96. / 25.4) as u32;
    let py = (y * 96. / 25.4) as u32;
    let start = ((py * image.width() + px) * 4) as usize;
    image.data()[start..start + 4].try_into().unwrap()
}
#[test]
fn relative_and_root_paths() {
    assert_eq!(
        resolve("D/Pages/P.xml", "../../Res/f.ttf").unwrap(),
        "Res/f.ttf"
    );
    assert_eq!(
        resolve("D/Pages/P.xml", "/D/Res/f.ttf").unwrap(),
        "D/Res/f.ttf"
    );
}
#[test]
fn rejects_escape_and_external_paths() {
    for p in ["../../../x", "https://host/x", "C:/file", "a\\b"] {
        assert!(resolve("D/P.xml", p).is_err(), "{p}")
    }
}
#[test]
fn rejects_duplicate_archive_paths() {
    let a = archive(&[("OFD.xml", b("<OFD/>")), ("x/../OFD.xml", b("<OFD/>"))]);
    assert!(Package::from_bytes(&a).is_err());
}
#[test]
fn rejects_missing_entry() {
    assert!(Package::from_bytes(&archive(&[("x", vec![])])).is_err())
}
#[test]
fn rejects_malformed_zip() {
    assert!(parser::from_bytes(b"not a zip").is_err())
}
#[test]
fn rejects_dtd() {
    let a = archive(&[("OFD.xml", b("<!DOCTYPE OFD [<!ENTITY e 'x'>]><OFD/>"))]);
    assert!(parser::from_bytes(&a).is_err())
}
#[test]
fn rejects_zip_bomb_entry() {
    let a = archive(&[("OFD.xml", vec![0; 33 * 1024 * 1024])]);
    assert!(Package::from_bytes(&a).is_err())
}
#[test]
fn expands_delta_runs() {
    assert_eq!(
        parser::deltas("1 g 3 2.5 -1").unwrap(),
        vec![1., 2.5, 2.5, 2.5, -1.]
    );
}
#[test]
fn rejects_delta_bombs() {
    assert!(parser::deltas("g 999999999 1").is_err());
    assert!(parser::deltas("g 2").is_err());
    assert!(parser::deltas("NaN").is_err());
}
#[test]
fn matrix_order() {
    let m = Matrix::translate(5., 6.).concat(Matrix::scale(2., 3.));
    assert_eq!(m.0, [2., 0., 0., 3., 5., 6.]);
}
#[test]
fn parses_prefixed_namespace() {
    let d = parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
    assert_eq!(d.pages.len(), 3);
    assert_eq!(d.metadata.doc_type, "OFD-A");
    assert_eq!(d.attachments.len(), 1);
    assert_eq!(d.outlines[1].page, Some(1));
    assert!(d.pages[0].text.contains("归档文档阅读器"));
}
#[test]
fn reads_embedded_font_bytes() {
    let d = parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
    assert!(
        d.pages[0].resources.fonts["2"]
            .bytes
            .as_ref()
            .is_some_and(|b| b.len() > 10000)
    );
}
#[test]
fn reads_attachments_without_mutation() {
    let data = include_bytes!("../samples/demo.ofd");
    let d = parser::from_bytes(data).unwrap();
    assert!(
        std::str::from_utf8(&d.attachments[0].bytes)
            .unwrap()
            .contains("原始字节")
    );
    assert_eq!(d.attachments[0].size, d.attachments[0].bytes.len());
}
#[test]
fn archive_check_reports_unembedded_fonts() {
    let d = parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
    assert!(d.issues.iter().any(|x| x.clause == "6.2.6 b"));
    assert!(d.issues.iter().any(|x| x.clause == "7.2"));
    assert!(d.report()["scope"].as_str().unwrap().contains("非合规认证"));
}
#[test]
fn permission_copy_is_respected() {
    let d = parser::from_bytes(&fixture(
        "",
        "<Permissions><Copy>false</Copy></Permissions>",
        "",
        &[],
    ))
    .unwrap();
    assert!(!d.copy_allowed);
    assert!(d.issues.iter().any(|i| i.clause == "6.2.2 a"));
}
#[test]
fn rejects_missing_page_resource() {
    let a = fixture("", "", "", &[]);
    let mut z = zip::ZipArchive::new(Cursor::new(a)).unwrap();
    let mut buf = String::new();
    use std::io::Read;
    z.by_name("Doc_0/Document.xml")
        .unwrap()
        .read_to_string(&mut buf)
        .unwrap();
    assert!(buf.contains("Pages/Content.xml"));
    let d = archive(&[
        (
            "OFD.xml",
            b("<OFD><DocBody><DocRoot>D.xml</DocRoot></DocBody></OFD>"),
        ),
        (
            "D.xml",
            b(
                "<Document><CommonData><PageArea><PhysicalBox>0 0 10 10</PhysicalBox></PageArea></CommonData><Pages><Page ID='1' BaseLoc='missing.xml'/></Pages></Document>",
            ),
        ),
    ]);
    assert!(parser::from_bytes(&d).is_err());
}
#[test]
fn renders_vectors_at_physical_coordinates() {
    let d = parser::from_bytes(&fixture(red_path(), "", "", &[])).unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert_eq!(pixel(&r.pixmap, 8., 8.), [255, 0, 0, 255]);
    assert_eq!(pixel(&r.pixmap, 3., 3.), [255, 255, 255, 255]);
    assert!(r.warnings.is_empty());
}

fn gradient_object(shading: &str, attrs: &str) -> String {
    red_path().replace(
        r#"<FillColor Value="255 0 0"/>"#,
        &format!("<FillColor {attrs}>{shading}</FillColor>"),
    )
}
fn axial(attrs: &str, segments: &str) -> String {
    format!("<AxialShd {attrs}>{segments}</AxialShd>")
}
fn red_blue() -> &'static str {
    r##"<Segment><Color Value="#FF 0 0"/></Segment><Segment><Color Value="0 0 #FF"/></Segment>"##
}
fn render_fixture(objects: &str, resources: &str) -> tiny_skia::Pixmap {
    let doc = parser::from_bytes(&fixture(objects, "", resources, &[])).unwrap();
    let rendered = Renderer::with_system_fonts(false)
        .render(&doc.pages[0], 1., 0)
        .unwrap();
    assert!(rendered.warnings.is_empty(), "{:?}", rendered.warnings);
    rendered.pixmap
}

#[test]
fn axial_gradient_uses_object_coordinates_and_hex_colors() {
    let object = gradient_object(
        &axial(r#"StartPoint="0 5" EndPoint="10 5" Extend="3""#, red_blue()),
        "",
    );
    let pixmap = render_fixture(&object, "");
    let left = pixel(&pixmap, 6., 10.);
    let right = pixel(&pixmap, 14., 10.);
    assert!(left[0] as u16 > left[2] as u16 * 4, "{left:?}");
    assert!(right[2] as u16 > right[0] as u16 * 4, "{right:?}");
    assert_eq!(pixel(&pixmap, 3., 3.), [255, 255, 255, 255]);
    let doc = parser::from_bytes(&fixture(&object, "", "", &[])).unwrap();
    assert!(!doc.issues.iter().any(|i| i.message.contains("渐变")));
}

#[test]
fn gradients_follow_ctm_and_page_rotation() {
    let object = gradient_object(
        &axial(r#"StartPoint="0 5" EndPoint="5 5" Extend="3""#, red_blue()),
        "",
    )
    .replace("Fill=\"true\"", "CTM=\"2 0 0 1 0 0\" Fill=\"true\"");
    let doc = parser::from_bytes(&fixture(&object, "", "", &[])).unwrap();
    let mut renderer = Renderer::with_system_fonts(false);
    let regular = renderer.render(&doc.pages[0], 1., 0).unwrap();
    let rotated = renderer.render(&doc.pages[0], 1., 90).unwrap();
    assert!(pixel(&regular.pixmap, 6., 10.)[0] > 200);
    assert!(pixel(&regular.pixmap, 14., 10.)[2] > 200);
    assert!(pixel(&rotated.pixmap, 20., 6.)[0] > 200);
    assert!(pixel(&rotated.pixmap, 20., 14.)[2] > 200);
}

#[test]
fn missing_segment_positions_are_interpolated_between_explicit_stops() {
    let segments = r#"<Segment><Color Value="255 0 0"/></Segment><Segment><Color Value="0 255 0"/></Segment><Segment Position="0.4"><Color Value="0 0 255"/></Segment><Segment><Color Value="255 255 255"/></Segment>"#;
    let object = gradient_object(
        &axial(r#"StartPoint="0 0" EndPoint="10 0" Extend="3""#, segments),
        "",
    );
    let doc = parser::from_bytes(&fixture(&object, "", "", &[])).unwrap();
    let Some(FillShader::Linear { stops, .. }) = &doc.pages[0].objects[0].style.fill_shader else {
        panic!("missing gradient");
    };
    assert_eq!(
        stops.iter().map(|s| s.position).collect::<Vec<_>>(),
        vec![0., 0.2, 0.4, 1.]
    );
    assert!(pixel(&render_fixture(&object, ""), 7., 10.)[1] > 230);
}

#[test]
fn gradient_extend_limits_the_axis_without_clipping_the_stroke() {
    for extend in 0..=3 {
        let shader = axial(
            &format!("StartPoint=\"3 5\" EndPoint=\"7 5\" Extend=\"{extend}\""),
            red_blue(),
        );
        let object = gradient_object(&shader, "");
        let pixmap = render_fixture(&object, "");
        assert_eq!(
            pixel(&pixmap, 6., 10.) == [255, 255, 255, 255],
            extend & 1 == 0
        );
        assert_eq!(
            pixel(&pixmap, 14., 10.) == [255, 255, 255, 255],
            extend & 2 == 0
        );
        assert!(pixel(&pixmap, 10., 10.)[1] < 10);
    }
}

#[test]
fn repeating_and_reflected_gradients_obey_map_unit() {
    for mode in ["Repeat", "Reflect"] {
        let object = gradient_object(
            &axial(
                &format!(
                    "StartPoint=\"0 0\" EndPoint=\"10 0\" MapType=\"{mode}\" MapUnit=\"2\" Extend=\"3\""
                ),
                red_blue(),
            ),
            "",
        );
        let pixmap = render_fixture(&object, "");
        let first = pixel(&pixmap, 5.5, 10.);
        let second = pixel(&pixmap, 7.5, 10.);
        if mode == "Repeat" {
            assert!(
                (first[0] as i32 - second[0] as i32).abs() < 40,
                "{first:?} {second:?}"
            );
        } else {
            assert!(
                first[0] > first[2] && second[2] > second[0],
                "{first:?} {second:?}"
            );
        }
    }
}

#[test]
fn radial_gradients_render_and_respect_end_circle() {
    for extend in 0..=3 {
        let shader = format!(
            "<RadialShd StartPoint=\"5 5\" EndPoint=\"5 5\" EndRadius=\"3\" Extend=\"{extend}\">{}</RadialShd>",
            red_blue()
        );
        let pixmap = render_fixture(&gradient_object(&shader, ""), "");
        assert!(pixel(&pixmap, 10., 10.)[0] > 225);
        let outside = pixel(&pixmap, 14., 10.);
        assert_eq!(outside == [255, 255, 255, 255], extend & 2 == 0);
        if extend & 2 != 0 {
            assert!(outside[2] > 240 && outside[0] < 10);
        }
    }
}

#[test]
fn gradients_combine_color_and_object_alpha_once() {
    let shader = axial(
        r#"StartPoint="0 0" EndPoint="10 0" Extend="3""#,
        r#"<Segment><Color Value="255 0 0" Alpha="128"/></Segment><Segment><Color Value="255 0 0" Alpha="128"/></Segment>"#,
    );
    // Segment alpha, FillColor alpha and object alpha each apply once.
    for (fill_alpha, object_alpha, expected) in [(255, 255, 127), (128, 255, 191), (128, 128, 223)]
    {
        let object = gradient_object(&shader, &format!("Alpha=\"{fill_alpha}\"")).replace(
            "Fill=\"true\"",
            &format!("Alpha=\"{object_alpha}\" Fill=\"true\""),
        );
        let p = pixel(&render_fixture(&object, ""), 10., 10.);
        assert_eq!(p[0], 255);
        assert!((p[1] as i32 - expected).abs() <= 2, "{p:?}");
    }
    let solid = red_path().replace("Value=\"255 0 0\"", "Value=\"255 0 0\" Alpha=\"128\"");
    assert!((pixel(&render_fixture(&solid, ""), 10., 10.)[1] as i32 - 127).abs() <= 1);
}

#[test]
fn gradients_inherit_color_space_and_drawing_parameters() {
    let resources = r##"<ColorSpaces><ColorSpace ID="7" Type="GRAY"><Palette><CV>#80</CV></Palette></ColorSpace></ColorSpaces><DrawParams><DrawParam ID="8"><FillColor ColorSpace="7"><AxialShd StartPoint="0 0" EndPoint="10 0" Extend="3"><Segment><Color Index="0"/></Segment><Segment><Color Value="#80"/></Segment></AxialShd></FillColor></DrawParam><DrawParam ID="9" Relative="8"/></DrawParams>"##;
    let object = red_path()
        .replace(r#"<FillColor Value="255 0 0"/>"#, "")
        .replace("Fill=\"true\"", "DrawParam=\"9\" Fill=\"true\"");
    assert_eq!(
        pixel(&render_fixture(&object, resources), 10., 10.),
        [128, 128, 128, 255]
    );
    let override_object = object.replace(
        "<AbbreviatedData>",
        "<FillColor Value=\"0 255 0\"/><AbbreviatedData>",
    );
    assert_eq!(
        pixel(&render_fixture(&override_object, resources), 10., 10.),
        [0, 255, 0, 255]
    );
}

#[test]
fn unsupported_radial_parameters_fall_back_without_rejecting_document() {
    for attrs in [
        r#"StartRadius="1""#,
        r#"Eccentricity="0.5""#,
        r#"MapUnit="2""#,
        r#"Angle="45""#,
    ] {
        let shader = format!(
            "<RadialShd StartPoint=\"5 5\" EndPoint=\"5 5\" EndRadius=\"3\" {attrs}>{}</RadialShd>",
            red_blue()
        );
        let object = gradient_object(&shader, "");
        let doc = parser::from_bytes(&fixture(&object, "", "", &[])).unwrap();
        assert!(doc.pages[0].objects[0].style.fill_shader.is_none());
        assert!(
            doc.issues
                .iter()
                .any(|i| i.message.contains("首段颜色回退"))
        );
        assert_eq!(
            pixel(&render_fixture(&object, ""), 10., 10.),
            [255, 0, 0, 255]
        );
    }
}

#[test]
fn gradients_reject_invalid_positions_geometry_and_resource_bombs() {
    for attrs in [
        r#"StartPoint="0 0" EndPoint="0 0""#,
        r#"StartPoint="NaN 0" EndPoint="10 0""#,
        r#"StartPoint="0" EndPoint="10 0""#,
        r#"StartPoint="0 0" EndPoint="10 0" Extend="4""#,
        r#"StartPoint="0 0" EndPoint="10 0" MapUnit="0""#,
    ] {
        let object = gradient_object(&axial(attrs, red_blue()), "");
        assert!(
            parser::from_bytes(&fixture(&object, "", "", &[])).is_err(),
            "{attrs}"
        );
    }
    for segments in [
        r#"<Segment Position="0.9"><Color Value="0 0 0"/></Segment><Segment Position="0.2"><Color Value="255 255 255"/></Segment>"#.to_string(),
        r#"<Segment Position="NaN"><Color Value="0 0 0"/></Segment><Segment><Color Value="255 255 255"/></Segment>"#.to_string(),
        r#"<Segment><Color Value="0 0 0"/></Segment>"#.repeat(65),
    ] {
        let object = gradient_object(&axial(r#"StartPoint="0 0" EndPoint="10 0""#, &segments), "");
        assert!(parser::from_bytes(&fixture(&object, "", "", &[])).is_err());
    }
}

#[test]
fn radial_gradients_reject_nonfinite_and_invalid_parameters() {
    for attrs in [
        r#"StartRadius="NaN""#,
        r#"StartRadius="-1""#,
        r#"Eccentricity="NaN""#,
        r#"Eccentricity="1""#,
        r#"Angle="invalid""#,
    ] {
        let shader = format!(
            "<RadialShd StartPoint=\"5 5\" EndPoint=\"5 5\" EndRadius=\"3\" {attrs}>{}</RadialShd>",
            red_blue()
        );
        let object = gradient_object(&shader, "");
        assert!(
            parser::from_bytes(&fixture(&object, "", "", &[])).is_err(),
            "{attrs}"
        );
    }
}
#[test]
fn gradient_stroke_and_existing_clip_keep_their_boundaries() {
    let shader = axial(r#"StartPoint="3 5" EndPoint="7 5""#, red_blue());
    let object = gradient_object(&shader, "")
        .replace(r#"Stroke="false""#, r#"Stroke="true" LineWidth="1""#)
        .replace(
            "<AbbreviatedData>",
            "<StrokeColor Value=\"0 255 0\"/><AbbreviatedData>",
        );
    let pixmap = render_fixture(&object, "");
    assert!(pixel(&pixmap, 6., 5.2)[1] > 240 && pixel(&pixmap, 6., 5.2)[0] < 10);
    let object = gradient_object(&axial(r#"StartPoint="0 0" EndPoint="10 0" Extend="3""#, red_blue()), "")
        .replace("<AbbreviatedData>", "<Clips><Clip><Area><Path><AbbreviatedData>M 0 0 L 4 0 L 4 10 L 0 10 C</AbbreviatedData></Path></Area></Clip></Clips><AbbreviatedData>");
    let pixmap = render_fixture(&object, "");
    assert!(pixel(&pixmap, 7., 10.)[0] > 150);
    assert_eq!(pixel(&pixmap, 12., 10.), [255, 255, 255, 255]);
    let object = red_path()
        .replace(
            r#"Fill="true" Stroke="false""#,
            r#"Fill="false" Stroke="true" LineWidth="1""#,
        )
        .replace(
            "<AbbreviatedData>",
            &format!(
                "<StrokeColor>{}</StrokeColor><AbbreviatedData>",
                axial(r#"StartPoint="0 0" EndPoint="10 0" Extend="3""#, red_blue())
            ),
        );
    let pixmap = render_fixture(&object, "");
    assert!(pixel(&pixmap, 6., 5.2)[0] > 200);
    assert!(pixel(&pixmap, 14., 5.2)[2] > 200);
}

#[test]
fn hexadecimal_color_channels_and_palette_values_are_checked() {
    let object = red_path().replace("255 0 0", "#FF #00 #00");
    assert_eq!(
        pixel(&render_fixture(&object, ""), 10., 10.),
        [255, 0, 0, 255]
    );
    for value in ["#GG 0 0", "# 0 0", "NaN 0 0"] {
        assert!(
            parser::from_bytes(&fixture(&red_path().replace("255 0 0", value), "", "", &[]))
                .is_err()
        );
    }
    let resources = r##"<ColorSpaces><ColorSpace ID="7" Type="RGB"><Palette><CV>#GG 0 0</CV></Palette></ColorSpace></ColorSpaces>"##;
    assert!(parser::from_bytes(&fixture(red_path(), "", resources, &[])).is_err());
}

#[test]
fn gradient_pixels_survive_pdf_and_ofd_conversion() {
    use ofd_reader::{
        conversion::{self, Format, Options},
        document::Document,
    };
    let object = gradient_object(
        &axial(r#"StartPoint="0 0" EndPoint="10 0" Extend="3""#, red_blue()),
        "",
    );
    let source = Document::from_ofd(parser::from_bytes(&fixture(&object, "", "", &[])).unwrap());
    for format in [Format::Pdf, Format::Ofd] {
        let bytes = conversion::convert(
            &source,
            &Options {
                format,
                pages: vec![0],
                dpi: 96,
            },
        )
        .unwrap();
        let doc = if format == Format::Pdf {
            Document::pdf_from_bytes(bytes).unwrap()
        } else {
            Document::from_ofd(parser::from_bytes(&bytes).unwrap())
        };
        let rendered = doc
            .render(&mut Renderer::with_system_fonts(false), 0, 1., 0)
            .unwrap();
        let left = pixel(&rendered.pixmap, 6., 10.);
        let right = pixel(&rendered.pixmap, 14., 10.);
        assert!(
            left[0] > 200 && right[2] > 200,
            "{format:?} {left:?} {right:?}"
        );
    }
}
#[test]
fn renderer_supports_rotation() {
    let d = parser::from_bytes(&fixture(red_path(), "", "", &[])).unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 90).unwrap();
    assert_eq!(r.pixmap.width(), (30f32 * 96. / 25.4).ceil() as u32);
    assert_eq!(r.pixmap.height(), (40f32 * 96. / 25.4).ceil() as u32);
    assert_eq!(pixel(&r.pixmap, 22., 8.), [255, 0, 0, 255]);
}
#[test]
fn renderer_limits_memory() {
    let d = parser::from_bytes(&fixture(red_path(), "", "", &[])).unwrap();
    let mut p = d.pages[0].clone();
    p.area.w = 20000.;
    p.area.h = 20000.;
    assert!(Renderer::new().render(&p, 1., 0).is_err());
}
#[test]
fn path_supports_curves_and_arcs() {
    for p in [
        "S 0 0 L 10 0 L 10 10 C",
        "M 0 0 Q 5 5 10 0",
        "M 0 0 B 1 2 3 4 5 6",
        "M 0 0 A 5 5 0 0 1 10 0",
    ] {
        assert!(ofd_path(p).unwrap().is_some(), "{p}")
    }
}
#[test]
fn malformed_path_is_reported() {
    assert!(ofd_path("M 0 0 B 1").is_err());
}
#[test]
fn clips_vector_to_path() {
    let o=red_path().replace("<FillColor",r#"<Clips><Clip><Area><Path><AbbreviatedData>M 0 0 L 4 0 L 4 10 L 0 10 C</AbbreviatedData></Path></Area></Clip></Clips><FillColor"#);
    let d = parser::from_bytes(&fixture(&o, "", "", &[])).unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert_eq!(pixel(&r.pixmap, 7., 8.), [255, 0, 0, 255]);
    assert_eq!(pixel(&r.pixmap, 12., 8.), [255, 255, 255, 255]);
}
#[test]
fn missing_images_are_visible_warnings() {
    let o = r#"<ImageObject ID="3" Boundary="0 0 10 10" ResourceID="99"/>"#;
    let d = parser::from_bytes(&fixture(o, "", "", &[])).unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert!(r.warnings.iter().any(|x| x.contains("99")));
}
#[test]
fn draws_image_using_normalized_ctm() {
    let mut im = tiny_skia::Pixmap::new(2, 2).unwrap();
    im.fill(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
    let o = r#"<ImageObject ID="3" Boundary="5 5 10 10" CTM="10 0 0 10 0 0" ResourceID="9"/>"#;
    let res = r#"<MultiMedias><MultiMedia ID="9" Type="Image"><MediaFile>i.png</MediaFile></MultiMedia></MultiMedias>"#;
    let d = parser::from_bytes(&fixture(
        o,
        "",
        res,
        &[("Doc_0/Res/i.png", im.encode_png().unwrap())],
    ))
    .unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert_eq!(pixel(&r.pixmap, 8., 8.), [0, 0, 255, 255]);
    assert!(r.warnings.is_empty());
}
#[test]
fn draws_embedded_glyph_outlines() {
    let pkg = Package::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
    let font = pkg.bytes("Doc_0/Res/Ubuntu-Light.ttf").unwrap();
    let o = r#"<TextObject ID="3" Boundary="0 0 30 20" Font="1" Size="10"><TextCode X="1" Y="12">ABC</TextCode></TextObject>"#;
    let res = r#"<Fonts><Font ID="1" FontName="Ubuntu"><FontFile>f.ttf</FontFile></Font></Fonts>"#;
    let d = parser::from_bytes(&fixture(
        o,
        "",
        res,
        &[("Doc_0/Res/f.ttf", font.as_ref().clone())],
    ))
    .unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert!(r.warnings.is_empty());
    assert!(
        r.pixmap
            .data()
            .chunks_exact(4)
            .filter(|p| p[0] < 80 && p[1] < 80 && p[2] < 80)
            .count()
            > 150
    );
}
#[test]
fn transforms_image_alpha_correctly() {
    let mut image = image::RgbaImage::new(2, 2);
    for p in image.pixels_mut() {
        *p = image::Rgba([255, 0, 0, 128]);
    }
    let mut bytes = Cursor::new(vec![]);
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let o = r#"<ImageObject ID="3" Boundary="0 0 10 10" ResourceID="9"/>"#;
    let res = r#"<MultiMedias><MultiMedia ID="9" Type="Image"><MediaFile>i.png</MediaFile></MultiMedia></MultiMedias>"#;
    let d = parser::from_bytes(&fixture(
        o,
        "",
        res,
        &[("Doc_0/Res/i.png", bytes.into_inner())],
    ))
    .unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    let p = pixel(&r.pixmap, 3., 3.);
    assert_eq!(p[0], 255);
    assert!((p[1] as i32 - 127).abs() <= 1);
}
#[test]
fn report_is_serializable() {
    let d = parser::from_bytes(&fixture("", "", "", &[])).unwrap();
    let s = serde_json::to_string_pretty(&d.report()).unwrap();
    assert!(s.contains("GB/T 42133"));
}

#[test]
fn composite_keeps_inherited_namespace() {
    let a = fixture(
        r#"<CompositeObject ID="3" Boundary="5 5 10 10" ResourceID="9"/>"#,
        "",
        r#"<ofd:CompositeGraphicUnits xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CompositeGraphicUnit ID="9"><Content><PathObject ID="10" Boundary="0 0 10 10" Fill="true" Stroke="false"><FillColor Value="255 0 0"/><AbbreviatedData>M 0 0 L 10 0 L 10 10 L 0 10 C</AbbreviatedData></PathObject></Content></ofd:CompositeGraphicUnit></ofd:CompositeGraphicUnits>"#,
        &[],
    );
    let d = parser::from_bytes(&a).unwrap();
    let r = Renderer::new().render(&d.pages[0], 1., 0).unwrap();
    assert_eq!(pixel(&r.pixmap, 8., 8.), [255, 0, 0, 255]);
}

#[test]
fn prevents_export_over_original() {
    let root = std::env::temp_dir().join(format!("ofd-reader-guard-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("original.ofd");
    std::fs::write(&source, b"original").unwrap();
    assert!(ofd_reader::export::write(&source, &source, b"overwritten").is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"original");
    let target = root.join("output.txt");
    ofd_reader::export::write(&source, &target, b"export").unwrap();
    assert_eq!(std::fs::read(target).unwrap(), b"export");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundled_font_contains_chinese() {
    let f = ttf_parser::Face::parse(ofd_reader::fonts::DEFAULT_CJK, 0).unwrap();
    for ch in "中文阅读打印连续滚动国标".chars() {
        assert!(f.glyph_index(ch).is_some(), "missing {ch}");
    }
}
#[test]
fn ofd_chinese_without_system_fonts() {
    let d = parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap();
    let r = Renderer::with_system_fonts(false)
        .render(&d.pages[0], 0.5, 0)
        .unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(
        r.pixmap
            .data()
            .chunks_exact(4)
            .filter(|p| p[0] < 100 && p[1] < 100)
            .count()
            > 500
    );
}
#[test]
fn pdf_reads_text_outlines_and_dimensions() {
    let d = ofd_reader::document::Document::pdf_from_bytes(
        include_bytes!("../samples/demo.pdf").to_vec(),
    )
    .unwrap();
    assert!(d.is_pdf());
    assert_eq!(d.pages.len(), 3);
    assert_eq!(d.outlines.len(), 3);
    assert_eq!(d.outlines[1].page, Some(1));
    assert!(d.pages[0].text.contains("PDF document reading"));
    assert!((d.pages[0].area.w - 210.).abs() < 0.1);
    assert!(d.pages[1].text.contains("连续"));
    assert_eq!(d.report()["format"], "PDF");
}
#[test]
fn pdf_chinese_fallback_renders_visible_glyphs() {
    let d = ofd_reader::document::Document::pdf_from_bytes(
        include_bytes!("../samples/demo.pdf").to_vec(),
    )
    .unwrap();
    let r = d
        .render(&mut Renderer::with_system_fonts(false), 0, 0.5, 0)
        .unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let count = (120..145)
        .flat_map(|y| (38..340).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let i = (y * r.pixmap.width() as usize + x) * 4;
            r.pixmap.data()[i] < 100
        })
        .count();
    assert!(count > 80, "Chinese region is blank, dark pixels={count}");
    assert!(d.issues.iter().any(|i| i.message.contains("Noto Sans SC")));
}
#[test]
fn rejects_broken_pdf() {
    assert!(ofd_reader::document::Document::pdf_from_bytes(b"%PDF-1.7 broken".to_vec()).is_err());
}
#[test]
fn pdf_rotation_and_pixel_limit() {
    let d = ofd_reader::document::Document::pdf_from_bytes(
        include_bytes!("../samples/demo.pdf").to_vec(),
    )
    .unwrap();
    let mut r = Renderer::with_system_fonts(false);
    let a = d.render(&mut r, 0, 0.25, 0).unwrap();
    let b = d.render(&mut r, 0, 0.25, 90).unwrap();
    assert_eq!(a.pixmap.width(), b.pixmap.height());
    assert_eq!(a.pixmap.height(), b.pixmap.width());
    assert!(d.render(&mut r, 0, 16., 0).is_err());
    assert!(d.render(&mut r, 99, 1., 0).is_err());
}
#[test]
fn print_ranges_are_validated() {
    use ofd_reader::printing::page_range;
    assert_eq!(page_range("1-3,5,2", 5).unwrap(), vec![0, 1, 2, 4]);
    assert_eq!(page_range("", 3).unwrap(), vec![0, 1, 2]);
    for input in ["0", "3-1", "1-9", "-1", "1,,2", "a"] {
        assert!(page_range(input, 5).is_err(), "{input}");
    }
}
#[test]
fn print_permission_is_enforced() {
    let data = fixture(
        red_path(),
        "<Permissions><Print Printable='false'/></Permissions>",
        "",
        &[],
    );
    let d = ofd_reader::document::Document::from_ofd(parser::from_bytes(&data).unwrap());
    assert!(!d.print_allowed);
    assert!(
        ofd_reader::printing::print_pdf(
            &d,
            &ofd_reader::printing::Options {
                pages: vec![0],
                copies: 1,
                dpi: 96,
                printer: None
            }
        )
        .is_err()
    );
}
#[test]
fn print_pdf_preserves_selected_page_sizes() {
    let d = ofd_reader::document::Document::pdf_from_bytes(
        include_bytes!("../samples/demo.pdf").to_vec(),
    )
    .unwrap();
    let options = ofd_reader::printing::Options {
        pages: vec![1, 2],
        copies: 1,
        dpi: 96,
        printer: None,
    };
    let bytes = ofd_reader::printing::print_pdf(&d, &options).unwrap();
    let printed = ofd_reader::document::Document::pdf_from_bytes(bytes).unwrap();
    assert_eq!(printed.pages.len(), 2);
    assert!((printed.pages[0].area.w - d.pages[1].area.w).abs() < 0.1);
    let r = printed
        .render(&mut Renderer::with_system_fonts(false), 0, 0.25, 0)
        .unwrap();
    assert!(r.warnings.is_empty());
}
#[test]
fn print_options_reject_empty_and_invalid() {
    let d = ofd_reader::document::Document::from_ofd(
        parser::from_bytes(&fixture(red_path(), "", "", &[])).unwrap(),
    );
    for (pages, copies, dpi) in [
        (vec![], 1, 150),
        (vec![10], 1, 150),
        (vec![0], 0, 150),
        (vec![0], 1, 600),
    ] {
        assert!(
            ofd_reader::printing::print_pdf(
                &d,
                &ofd_reader::printing::Options {
                    pages,
                    copies,
                    dpi,
                    printer: None
                }
            )
            .is_err()
        );
    }
}
#[test]
fn print_ofd_outputs_real_pdf() {
    let d = ofd_reader::document::Document::from_ofd(
        parser::from_bytes(&fixture(red_path(), "", "", &[])).unwrap(),
    );
    let bytes = ofd_reader::printing::print_pdf(
        &d,
        &ofd_reader::printing::Options {
            pages: vec![0],
            copies: 2,
            dpi: 96,
            printer: None,
        },
    )
    .unwrap();
    let pdf = ofd_reader::document::Document::pdf_from_bytes(bytes).unwrap();
    assert_eq!(pdf.pages.len(), 1);
    let r = pdf
        .render(&mut Renderer::with_system_fonts(false), 0, 1., 0)
        .unwrap();
    let pixel = pixel(&r.pixmap, 8., 8.);
    assert!(pixel[0] > 240 && pixel[1] < 20);
}
