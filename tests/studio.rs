use ofd_reader::{
    conversion::{self, Format, Options},
    document::Document,
    editing::{Editor, Mark, Point, Project},
    model::Rect,
    render::Renderer,
    signing,
};
use std::sync::Arc;
fn pdf() -> Arc<Document> {
    Arc::new(Document::pdf_from_bytes(include_bytes!("../samples/demo.pdf").to_vec()).unwrap())
}
fn identity() -> signing::Identity {
    signing::Identity::from_pkcs12(include_bytes!("fixtures/test-identity.p12"), "fixture-only")
        .unwrap()
}
fn project(doc: &Document) -> Project {
    Project::new(doc, doc.source_hash.clone())
}
#[test]
fn highlight_is_transparent_and_text_survives() {
    let source = pdf();
    let mut work = project(&source);
    work.pages[0].marks.push(Mark::Highlight {
        rect: Rect {
            x: 0.,
            y: 0.,
            w: 30.,
            h: 20.,
        },
        color: [255, 255, 0, 70],
    });
    work.pages[0].marks.push(Mark::Text {
        rect: Rect {
            x: 20.,
            y: 120.,
            w: 80.,
            h: 12.,
        },
        text: "新增中文批注".into(),
        size: 5.,
        color: [200, 20, 20, 255],
    });
    let edited = Document::edited(source, work).unwrap();
    assert!(edited.pages[0].text.contains("新增中文"));
    let image = edited
        .render(&mut Renderer::with_system_fonts(false), 0, 0.5, 0)
        .unwrap();
    assert!(image.warnings.is_empty());
    let b = edited
        .render(&mut Renderer::with_system_fonts(false), 0, 0.5, 90)
        .unwrap();
    assert_eq!(image.pixmap.width(), b.pixmap.height());
}
#[test]
fn annotation_undo_redo_dirty_state() {
    let doc = pdf();
    let original = project(&doc);
    let mut editor = Editor::new(original.clone());
    let mut next = original.clone();
    next.pages[0].marks.push(Mark::Rectangle {
        rect: Rect {
            x: 1.,
            y: 1.,
            w: 20.,
            h: 10.,
        },
        color: [255, 0, 0, 255],
        width: 0.5,
    });
    editor.commit(next.clone(), &doc).unwrap();
    assert!(editor.dirty());
    assert!(editor.undo());
    assert_eq!(editor.project, original);
    assert!(!editor.dirty());
    assert!(editor.redo());
    assert_eq!(editor.project, next);
    editor.saved();
    assert!(!editor.dirty());
}
#[test]
fn source_reference_and_geometry_limits() {
    let doc = pdf();
    let mut work = project(&doc);
    work.pages[0].source = 999;
    assert!(work.validate(&doc).is_err());
    work = project(&doc);
    work.pages[0].marks.push(Mark::Text {
        rect: Rect {
            x: -1.,
            y: 0.,
            w: 10.,
            h: 10.,
        },
        text: "test".into(),
        size: 4.,
        color: [0, 0, 0, 255],
    });
    assert!(work.validate(&doc).is_err());
}
#[test]
fn page_reordering_deletion_and_rotation() {
    let source = pdf();
    let mut work = project(&source);
    work.pages.swap(0, 2);
    work.pages.remove(1);
    work.pages[0].turns = 1;
    let edited = Document::edited(source.clone(), work).unwrap();
    assert_eq!(edited.pages.len(), 2);
    assert!(edited.pages[0].text.contains("Print and export"));
    assert!((edited.pages[0].area.w - source.pages[2].area.h).abs() < 0.1);
    let r = edited
        .render(&mut Renderer::with_system_fonts(false), 0, 0.25, 0)
        .unwrap();
    assert!(r.pixmap.width() > r.pixmap.height());
}
#[test]
fn ofda_refuses_editing_at_core_layer() {
    let source = Arc::new(Document::from_ofd(
        ofd_reader::parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap(),
    ));
    let mut work = project(&source);
    work.pages[0].turns = 1;
    assert!(Document::edited(source, work).is_err());
}
#[test]
fn ink_and_stamp_render() {
    let source = pdf();
    let mut work = project(&source);
    let mut stamp = tiny_skia::Pixmap::new(8, 8).unwrap();
    stamp.fill(tiny_skia::Color::from_rgba8(250, 0, 0, 255));
    work.pages[0].marks = vec![
        Mark::Ink {
            points: vec![Point { x: 10., y: 90. }, Point { x: 40., y: 90. }],
            color: [0, 0, 255, 255],
            width: 2.,
        },
        Mark::Stamp {
            rect: Rect {
                x: 20.,
                y: 160.,
                w: 25.,
                h: 25.,
            },
            png: stamp.encode_png().unwrap(),
            label: "Test only".into(),
        },
    ];
    let edited = Document::edited(source, work).unwrap();
    let r = edited
        .render(&mut Renderer::with_system_fonts(false), 0, 0.25, 0)
        .unwrap();
    assert!(r.warnings.is_empty());
    assert!(
        r.pixmap
            .data()
            .chunks_exact(4)
            .filter(|p| p[0] > 240 && p[1] < 20)
            .count()
            > 200
    );
}
#[test]
fn conversion_pdf_to_ofd_reopens() {
    let source = pdf();
    let bytes = conversion::convert(
        &source,
        &Options {
            format: Format::Ofd,
            pages: vec![1, 2],
            dpi: 96,
        },
    )
    .unwrap();
    let ofd = ofd_reader::parser::from_bytes(&bytes).unwrap();
    assert_eq!(ofd.pages.len(), 2);
    assert_eq!(ofd.metadata.doc_type, "OFD");
    let r = Renderer::with_system_fonts(false)
        .render(&ofd.pages[0], 0.5, 0)
        .unwrap();
    assert!(r.warnings.is_empty());
}
#[test]
fn conversion_ofd_to_pdf_reopens() {
    let source = Document::from_ofd(
        ofd_reader::parser::from_bytes(include_bytes!("../samples/demo.ofd")).unwrap(),
    );
    let bytes = conversion::convert(
        &source,
        &Options {
            format: Format::Pdf,
            pages: vec![2, 0],
            dpi: 96,
        },
    )
    .unwrap();
    let d = Document::pdf_from_bytes(bytes).unwrap();
    assert_eq!(d.pages.len(), 2);
    assert!((d.pages[0].area.h - 297.).abs() < 0.1);
}
#[test]
fn png_zip_preserves_page_count() {
    let bytes = conversion::convert(
        &pdf(),
        &Options {
            format: Format::PngZip,
            pages: vec![0, 2],
            dpi: 96,
        },
    )
    .unwrap();
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(zip.len(), 2);
    assert!(zip.name_for_index(0).unwrap().ends_with(".png"));
}
#[test]
fn conversion_rejects_bad_options() {
    for pages in [vec![], vec![999]] {
        assert!(
            conversion::convert(
                &pdf(),
                &Options {
                    format: Format::Pdf,
                    pages,
                    dpi: 96
                }
            )
            .is_err()
        );
    }
}
#[test]
fn detached_signature_integrity_and_tampering() {
    let id = identity();
    let input = include_bytes!("../samples/demo.ofd");
    let p7s = id.detached(input).unwrap();
    let result = signing::verify_detached(input, &p7s).unwrap();
    assert!(result.integrity_valid);
    assert!(result.certificate_subject.contains("Test Only"));
    assert!(result.trust.contains("未验证"));
    assert!(signing::verify_detached(b"changed", &p7s).is_err());
}
#[test]
fn wrong_certificate_password_is_rejected() {
    assert!(
        signing::Identity::from_pkcs12(include_bytes!("fixtures/test-identity.p12"), "wrong")
            .is_err()
    );
}
#[test]
fn embedded_pdf_signature_verifies_and_rejects_changes() {
    let signed = signing::sign_pdf(
        include_bytes!("../samples/demo.pdf"),
        &identity(),
        "Test only",
    )
    .unwrap();
    let result = signing::verify_pdf(&signed).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].integrity_valid && result[0].file_fully_covered);
    let doc = Document::pdf_from_bytes(signed.clone()).unwrap();
    assert_eq!(doc.pages.len(), 3);
    assert!(signing::sign_pdf(&signed, &identity(), "second").is_err());
    let mut changed = signed;
    changed[8] = b'X';
    assert!(signing::verify_pdf(&changed).is_err());
}
#[test]
fn appended_pdf_data_is_not_reported_fully_covered() {
    let mut signed = signing::sign_pdf(
        include_bytes!("../samples/demo.pdf"),
        &identity(),
        "Test only",
    )
    .unwrap();
    signed.extend_from_slice(b"\n% new data\n");
    let result = signing::verify_pdf(&signed).unwrap();
    assert!(!result[0].file_fully_covered);
}
#[test]
fn default_registration_quotes_paths_and_does_not_touch_userchoice() {
    let values = ofd_reader::associations::windows_plan(std::path::Path::new(
        "C:/Program Files/OFD Reader/reader.exe",
    ))
    .unwrap();
    assert!(
        values
            .iter()
            .all(|v| v.key.starts_with("HKCU") && !v.key.contains("UserChoice"))
    );
    let cmd = values
        .iter()
        .find(|v| v.key.ends_with("open\\command"))
        .unwrap();
    assert_eq!(
        cmd.value,
        "\"C:/Program Files/OFD Reader/reader.exe\" \"%1\""
    );
    let desktop = ofd_reader::associations::desktop_entry(
        std::path::Path::new("/opt/Reader Space/reader"),
        std::path::Path::new("/opt/icon.png"),
    )
    .unwrap();
    assert!(desktop.contains("Exec=\"/opt/Reader Space/reader\" %f"));
    assert!(desktop.contains("application/ofd"));
}

#[test]
fn windows_registration_converts_verbatim_drive_and_unc_paths() {
    for (input, expected) in [
        (r"\\?\E:\阅读 工具\简阅.exe", r"E:\阅读 工具\简阅.exe"),
        (r"\\?\UNC\server\share\简阅.exe", r"\\server\share\简阅.exe"),
        (r"\\server\share\简阅.exe", r"\\server\share\简阅.exe"),
    ] {
        let values = ofd_reader::associations::windows_plan(std::path::Path::new(input)).unwrap();
        let commands: Vec<_> = values
            .iter()
            .filter(|v| v.key.ends_with(r"\shell\open\command"))
            .collect();
        assert_eq!(commands.len(), 3);
        for command in commands {
            assert_eq!(command.value, format!("\"{expected}\" \"%1\""));
        }
        assert!(
            values
                .iter()
                .filter(|v| v.key.ends_with(r"\DefaultIcon"))
                .all(|v| v.value == format!("\"{expected}\",0"))
        );
        assert!(values.iter().any(
            |v| v.key.ends_with(r"\Applications\简阅.exe\SupportedTypes") && v.name == ".ofd"
        ));
        assert!(values.iter().any(
            |v| v.key.ends_with(r"\Applications\简阅.exe\SupportedTypes") && v.name == ".pdf"
        ));
    }
}
#[test]
fn windows_registration_rejects_relative_and_device_paths() {
    for path in [
        r"reader.exe",
        r"C:reader.exe",
        r"\\.\pipe\reader.exe",
        r"\\?\Volume{example}\reader.exe",
        "C:\\bad\nname.exe",
        "C:\\bad\"name.exe",
    ] {
        assert!(
            ofd_reader::associations::windows_plan(std::path::Path::new(path)).is_err(),
            "{path}"
        );
    }
}

#[test]
fn source_hash_guards_project_reopening() {
    let source = pdf();
    let work = project(&source);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("project.json");
    std::fs::write(&path, serde_json::to_vec(&work).unwrap()).unwrap();
    assert!(Project::load(&path, &source, &source.source_hash).is_ok());
    assert!(Project::load(&path, &source, "other").is_err());
}
#[test]
fn icon_ico_contains_all_supplied_frames() {
    let ico = include_bytes!("../assets/icons/app.ico");
    assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 6);
    let mut sizes = vec![];
    for i in 0..6 {
        let n = ico[6 + i * 16] as usize;
        sizes.push(if n == 0 { 256 } else { n });
    }
    assert_eq!(sizes, vec![32, 48, 64, 96, 128, 256]);
}
