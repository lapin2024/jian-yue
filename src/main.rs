use anyhow::{Context, Result, ensure};
use ofd_reader::{document::Document, render::Renderer};
use std::path::PathBuf;
#[cfg(feature = "desktop")]
mod app;
#[cfg(feature = "desktop")]
mod ui;

fn main() {
    if let Err(e) = run() {
        eprintln!("简阅: {e:#}");
        std::process::exit(1)
    }
}
fn option(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|x| x == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!(
            "简阅 {}\n\n桌面: jian-yue [document.ofd|document.pdf]\n检查: jian-yue --inspect document.ofd [--output report.json]\n渲染: jian-yue --render document.ofd --output page.png [--page 1] [--zoom 1.5] [--rotate 90]\n文本: jian-yue --text document.ofd --output text.txt\n附件: jian-yue --extract-attachment document.ofd --attachment 1 --output file.txt\n多文档: 以上命令可以附加 --document 1（序号从 1 开始）\n打印文件: jian-yue --print-pdf document.ofd --pages 1-3 --output print.pdf\n转换: jian-yue --convert document.pdf --output document.ofd [--project document.reader.json]\n签名: jian-yue --sign document.pdf --certificate certificate.p12 --output signed.pdf [--detached]\n验签: jian-yue --verify-signature signed.pdf [--signature document.p7s]\n界面截图测试: jian-yue document.ofd --screenshot screen.png\n",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--version") {
        println!("jian-yue {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mode = [
        "--inspect",
        "--render",
        "--text",
        "--extract-attachment",
        "--print-pdf",
        "--convert",
        "--verify-signature",
        "--sign",
    ]
    .into_iter()
    .find(|m| args.iter().any(|a| a == m));
    if let Some(mode) = mode {
        let path = PathBuf::from(option(&args, mode).context("缺少 OFD 文件路径")?);
        let doc_index: usize = option(&args, "--document").unwrap_or("1".into()).parse()?;
        ensure!(doc_index >= 1, "文档序号从 1 开始");
        let mut doc = Document::open(&path, doc_index - 1)?;
        if let Some(project_path) = option(&args, "--project") {
            let project = ofd_reader::editing::Project::load(
                std::path::Path::new(&project_path),
                &doc,
                &doc.source_hash,
            )?;
            doc = Document::edited(std::sync::Arc::new(doc), project)?;
        }
        let out = option(&args, "--output");
        if let Some(out) = &out {
            ofd_reader::export::ensure_distinct(&path, std::path::Path::new(out))?;
        }
        match mode {
            "--inspect" => {
                let json = serde_json::to_string_pretty(&doc.report())?;
                if let Some(out) = out {
                    std::fs::write(out, json)?
                } else {
                    println!("{json}")
                }
            }
            "--render" => {
                ensure!(doc.export_allowed, "文档权限禁止导出");
                let page: usize = option(&args, "--page").unwrap_or("1".into()).parse()?;
                let _p = doc
                    .pages
                    .get(page.checked_sub(1).context("页码从 1 开始")?)
                    .context("页码超过文档范围")?;
                let zoom: f32 = option(&args, "--zoom").unwrap_or("1".into()).parse()?;
                let rotation: u32 = option(&args, "--rotate").unwrap_or("0".into()).parse()?;
                ensure!(rotation.is_multiple_of(90), "旋转角度必须是 90 的倍数");
                let rendered = doc.render(&mut Renderer::new(), page - 1, zoom, rotation)?;
                rendered
                    .pixmap
                    .save_png(out.context("渲染必须指定 --output page.png")?)?;
                for w in rendered.warnings {
                    eprintln!("渲染提示: {w}");
                }
            }
            "--convert" => {
                let target = out.context("转换必须指定 --output")?;
                let format = match std::path::Path::new(&target)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "pdf" => ofd_reader::conversion::Format::Pdf,
                    "ofd" => ofd_reader::conversion::Format::Ofd,
                    "zip" => ofd_reader::conversion::Format::PngZip,
                    _ => anyhow::bail!("输出应为 .pdf/.ofd/.zip"),
                };
                let pages = ofd_reader::printing::page_range(
                    &option(&args, "--pages").unwrap_or_default(),
                    doc.pages.len(),
                )?;
                let dpi = option(&args, "--dpi").unwrap_or("150".into()).parse()?;
                let bytes = ofd_reader::conversion::convert(
                    &doc,
                    &ofd_reader::conversion::Options { format, pages, dpi },
                )?;
                ofd_reader::export::write(&path, std::path::Path::new(&target), &bytes)?;
            }
            "--verify-signature" => {
                let data = std::fs::read(&path)?;
                let reports = if let Some(sig) = option(&args, "--signature") {
                    let p = std::path::Path::new(&sig);
                    ensure!(std::fs::metadata(p)?.len() <= 1024 * 1024, "签名文件过大");
                    vec![ofd_reader::signing::verify_detached(
                        &data,
                        &std::fs::read(p)?,
                    )?]
                } else {
                    ofd_reader::signing::verify_pdf(&data)?
                };
                println!("{}", serde_json::to_string_pretty(&reports)?);
            }
            "--sign" => {
                ensure!(
                    option(&args, "--project").is_none(),
                    "请先保存并打开编辑副本，再签名"
                );
                let certificate = PathBuf::from(
                    option(&args, "--certificate").context("缺少 --certificate P12/PFX 路径")?,
                );
                ensure!(
                    std::fs::metadata(&certificate)?.len() <= 8 * 1024 * 1024,
                    "证书文件过大"
                );
                let target = PathBuf::from(out.context("签名必须指定 --output")?);
                ofd_reader::export::ensure_distinct(&certificate, &target)?;
                let password = zeroize::Zeroizing::new(
                    std::env::var("OFD_READER_CERT_PASSWORD")
                        .context("请通过 OFD_READER_CERT_PASSWORD 环境变量提供证书包密码")?,
                );
                let identity = ofd_reader::signing::Identity::from_pkcs12(
                    &std::fs::read(certificate)?,
                    &password,
                )?;
                let bytes = std::fs::read(&path)?;
                let signed = if args.iter().any(|s| s == "--detached") {
                    identity.detached(&bytes)?
                } else {
                    ensure!(doc.is_pdf(), "OFD 签名需要 --detached，生成分离式 .p7s");
                    ofd_reader::signing::sign_pdf(
                        &bytes,
                        &identity,
                        &option(&args, "--reason").unwrap_or("Document approval".into()),
                    )?
                };
                ofd_reader::export::write(&path, &target, &signed)?;
            }
            "--print-pdf" => {
                let pages = ofd_reader::printing::page_range(
                    &option(&args, "--pages").unwrap_or_default(),
                    doc.pages.len(),
                )?;
                let dpi = option(&args, "--dpi").unwrap_or("150".into()).parse()?;
                let bytes = ofd_reader::printing::print_pdf(
                    &doc,
                    &ofd_reader::printing::Options {
                        pages,
                        copies: 1,
                        dpi,
                        printer: None,
                    },
                )?;
                ofd_reader::export::write(
                    &path,
                    std::path::Path::new(&out.context("必须指定 --output print.pdf")?),
                    &bytes,
                )?;
            }
            "--text" => {
                ensure!(doc.copy_allowed, "文档权限禁止复制文字");
                let text = doc
                    .pages
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("--- 第 {} 页 ---\n{}\n", i + 1, p.text))
                    .collect::<String>();
                if let Some(out) = out {
                    std::fs::write(out, text)?
                } else {
                    print!("{text}")
                }
            }
            "--extract-attachment" => {
                ensure!(doc.export_allowed, "文档权限禁止导出");
                let index: usize = option(&args, "--attachment")
                    .context("缺少 --attachment 序号")?
                    .parse()?;
                let a = doc
                    .attachments
                    .get(index.checked_sub(1).context("附件序号从 1 开始")?)
                    .context("附件序号越界")?;
                std::fs::write(out.context("必须指定 --output 文件路径")?, a.bytes.as_ref())?;
            }
            _ => unreachable!(),
        }
        return Ok(());
    }
    #[cfg(feature = "desktop")]
    {
        let path = args
            .first()
            .filter(|s| !s.starts_with('-'))
            .map(PathBuf::from);
        let screenshot = option(&args, "--screenshot").map(PathBuf::from);
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn FreeConsole() -> i32;
            }
            unsafe {
                FreeConsole();
            }
        }
        if let (Some(path), Some(shot)) = (&path, &screenshot) {
            ofd_reader::export::ensure_distinct(path, shot)?;
        }
        app::launch(
            path,
            screenshot,
            args.iter().any(|a| a == "--single"),
            option(&args, "--start-page")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            option(&args, "--project").map(PathBuf::from),
            option(&args, "--tab").unwrap_or("read".into()),
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(feature = "desktop"))]
    {
        anyhow::bail!("此构建没有桌面界面，请使用 --inspect/--render/--text");
    }
}
