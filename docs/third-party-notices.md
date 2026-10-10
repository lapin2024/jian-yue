# 第三方许可说明

工程自身使用 MIT 许可证；该许可不替代依赖和字体许可证。

- 默认中文字体 Noto Sans SC 来自 Google Fonts 官方 google/fonts 仓库，使用 SIL Open Font License 1.1。静态字重处理和来源记录在 assets/fonts/README.md，许可位于 assets/fonts/OFL.txt；发布包包含 licenses/NotoSansSC-OFL.txt 和对应说明。
- PDF 栅格化使用 Hayro（Apache-2.0）及其组件；PDF 标准字体包含其随库分发的 Foxit 字体，按上游附带许可保留。PDF 结构解析使用 lopdf，打印文件生成使用 pdf-writer。
- eframe/egui 内置字体遵守对应 Ubuntu Font License、SIL Open Font License、Hack 和 Emoji Icon Font 许可证。
- OFD 容器/XML/渲染依赖 zip、roxmltree、tiny-skia、ttf-parser、fontdb、image、svgtypes、serde 等项目。
- samples/demo.ofd 中嵌入 Ubuntu-Light.ttf，附有 samples/licenses/Ubuntu-font-license.txt。
- Windows ARM64 GNU/LLVM 包包含匹配架构的 libunwind.dll 和 licenses/LLVM-LICENSE.txt。
- Rust 标准库为 MIT/Apache-2.0，并附带其第三方许可汇总。

发布包内 dependency-licenses.json 列出解析出的 Cargo 依赖许可；third-party-licenses 目录保留获取到的许可正文与字体授权。构建工具本身不随应用发布。


0.3.0 数字签名额外使用 RustCrypto 的 rsa、cms、der、x509-cert、sha2，以及 p12-keystore、zeroize 等。相应许可保留在依赖清单与第三方许可目录。

程序图标由用户提供，原始 PNG 保留在 assets/icons，ICO 仅封装原图片字节，不推定第三方授权。测试 P12 只存在于源码测试夹具，不作为运行时身份发布。
