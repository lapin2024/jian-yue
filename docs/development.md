# 开发和构建

## 工程结构

- src/archive.rs：受限 ZIP 读取和包内路径解析。
- src/parser.rs：XML、资源、页面、模板、复合对象、注释、附件和档案静态检查。
- src/model.rs：独立于 GUI 的文档模型。
- src/render.rs：字体轮廓、矢量和图像软件栅格化；页面原点/旋转/缩放。
- src/export.rs：导出路径检查，防止通常的同路径/符号链接覆盖原件。
- src/app.rs：eframe/egui 桌面界面、连续阅读视口、64 MiB 页面缓存与后台任务。
- src/ui.rs：参考图风格的矢量图标和工具栏。
- src/document.rs：OFD/PDF 统一模型、PDF 文字/大纲和渲染。
- src/pdf_fonts.rs：未嵌入中文字体的阅读副本修复和兼容性提示。
- src/fonts.rs：编译进程序的默认中文字体。
- src/printing.rs：打印范围、打印 PDF、Windows GDI 和 Linux CUPS。
- src/main.rs：命令行入口和桌面启动。
- tests/core.rs：安全、解析和像素回归测试。

## 平台目标

| 平台 | 推荐原生构建目标 | 当前 Windows 主机交叉构建目标 |
|---|---|---|
| Windows x86-64 | x86_64-pc-windows-msvc | x86_64-pc-windows-gnu |
| Windows x86 32 位 | i686-pc-windows-msvc | i686-pc-windows-gnu |
| Windows ARM64 | aarch64-pc-windows-msvc | aarch64-pc-windows-gnullvm |
| Linux x86-64 | x86_64-unknown-linux-gnu | 同目标，Zig 链接 |
| Linux x86 32 位 | i686-unknown-linux-gnu | 同目标，Zig 链接 |
| Linux ARM64 | aarch64-unknown-linux-gnu | 同目标，Zig 链接 |

应用没有 OFD SDK、Java、WebView、浏览器内核或外部 OFD 转换器依赖。GUI 需要操作系统窗口系统和可用 OpenGL 驱动；Linux 运行时建议具备 X11/Wayland、XKB、EGL/GL 及中文字体。无图形服务器时可使用命令行功能。

## 原生构建

Windows 安装 rustup 和 Visual Studio C++ Build Tools，选择 Desktop development with C++。ARM64 原生构建使用 ARM64 Windows 主机；32 位交叉构建需要相应 MSVC 库。

Linux 使用发行版构建工具、pkg-config 和图形开发库。x86 32 位在 x86-64 主机上构建时，添加 i386 开发库和 gcc-multilib。推荐在 ARM64 Linux 上原生构建 ARM64 发布版。

```sh
rustup toolchain install 1.90.0 --profile minimal
cargo +1.90.0 test --locked
cargo +1.90.0 build --release --locked
```

## 本次独立工具链

本地工具链安装在工程 tools 内，没有改写系统 PATH：Rust 1.90.0 GNU 主机工具链、MinGW GCC（64/32 位）、LLVM-MinGW（ARM64）、Zig 0.15.2、cargo-zigbuild。工具缓存不应提交到源码库。

本次交叉构建脚本只用于已准备好这些工具的当前工程环境：

```powershell
.\scripts\cross-build.ps1 windows-x86
.\scripts\cross-build.ps1 windows-arm64
.\scripts\cross-build.ps1 linux-x64
.\scripts\cross-build.ps1 linux-x86
.\scripts\cross-build.ps1 linux-arm64
```

Linux 链接目标指定 glibc 2.28 下限，最终依赖版本需以验证记录的 ELF 检查为准；不同发行版的窗口和驱动兼容性仍需真机验证。

GitHub Actions 配置了六个原生/相应交叉目标。该文件仅提供 CI 配置，本地创建它并不等于已在 GitHub 上执行；ARM64 托管运行器能否使用也取决于仓库和账户条件。

## 维护检查

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

依赖升级应先检查 Rust 最低版本、目标架构、字体和图片解析库的行为差异，并重跑像素测试。示例包 samples/demo.ofd 与 samples/demo.pdf 已随仓库提交。程序本身无 Python 依赖。

## 外部样本

本次额外测试样本取自 OFDRW 官方仓库的测试资源目录，存入 tmp，不随原创示例包再分发：helloworld.ofd、testPathClip.ofd、test-ofda.ofd。对应结果见验证记录。仓库样本自身可能含字体替代、编码损失或未支持图元，成功打开不能作为所有内容正确呈现的证明。

## 0.2.0 新增依赖与资源

assets/fonts/NotoSansSC-Regular.ttf 必须随源代码提供，include_bytes! 在编译时嵌入。发布可执行文件运行时不读取外部字体文件。默认字体来自 Google Fonts，已固定为 wght=400；字体许可证与生成来源也在该目录。

Hayro 0.4.0 与 lopdf 0.38 固定在 Cargo.lock 中；后续升级需重跑 PDF 内容、中文字体、旋转、打印和所有架构构建。纯 CLI 构建也包含 PDF 和打印文件生成功能。

Windows 打印通过 windows-sys 调用 PrintDlgW、StartDocW、StretchDIBits、EndDoc；x86 PRINTDLGW 使用上游正确的 packed ABI。Linux 通过 Command 的结构化参数调用 lpstat/lp，不构造 shell 命令，也不执行 PDF 中的动作。

连续阅读使用真实 egui ScrollArea 的鼠标滚轮事件测试，覆盖跨页、页码跟随、连续/单页定位和缓存限制。静态截图仅验证显示；实体打印机任务需要设备测试。

## 0.3.0 工作副本、签名和关联

- src/editing.rs：毫米坐标的可序列化标注、页面计划和受限撤销历史。项目绑定原件 SHA-256。
- src/conversion.rs：PDF/OFD/PNG 的外观重建；输出包含工作副本内容。
- src/signing.rs：PKCS#12 RSA 身份、CMS、PDF 签名占位区/ByteRange 和验签。
- src/studio.rs：顶部工具页、画布鼠标操作、项目/转换/签名窗口。
- src/associations.rs：显式点击后执行的默认应用注册；纯计划函数用于测试，测试不写真实注册表或默认设置。
- build.rs、assets/icons/app.rc：Windows 多尺寸图标资源。Linux/window/header 使用用户提供的 PNG。

编辑源页面保持不可变，渲染器先绘制源页及源坐标标注，再应用永久页面旋转和阅读视图旋转。打印和转换均接收同一工作副本文档。

连续阅读将累计页位置二分定位到视口，只遍历附近页面；滚动停稳前只请求缩略预览，160 ms 后请求高清。工作线程检查文档代次和当前页，跳过离屏过时任务；每帧最多上传一个回复中的纹理。该实现不承诺所有大文件/驱动达到固定 FPS。

P12 密码用短期 Zeroizing 字符串传给签名任务，不写入项目。私钥只从用户选择的证书包读取；程序不嵌入测试身份。验签不能推导 CA 信任。已有签名 PDF 不进行重写签名。

Windows 图标资源编译针对 i686/ARM64 设置 RC 目标，避免链接错误架构的资源对象。默认关联在 HKCU 注册，不修改 UserChoice。Linux 使用结构化 Command 调用 xdg-mime，并写当前用户的 desktop/MIME/icon 条目。


简阅在源代码中保留 ofd-reader / ofd_reader 作为 Rust 包和库名称，以保持模块路径；可执行目标为 jian-yue，Windows 发布时命名为 简阅.exe。Linux 的 app_id、desktop 文件和关联标识统一使用 jian-yue。

拖放显式启用 ViewportBuilder::with_drag_and_drop(true)，在处理时 take 掉输入队列，验证文件和大小写无关扩展名，避免重复打开或错误选择批次中的目录。混合批次只处理有效本地文档；多文档弹出选择框。测试使用真正的 egui DroppedFile 输入，验证中文空格路径、重复消费、混合目录、非法文件和多文件。

ARM64 GNU/LLVM 的图标资源使用 llvm-rc 生成 .res、llvm-cvtres /machine:ARM64 转换为 COFF，再直接链接对象，避免 LLVM windres 的 BFD 架构别名兼容问题。
