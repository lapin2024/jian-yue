# 内置默认中文字体

默认字体：Noto Sans SC，Regular（wght=400）。授权：SIL Open Font License 1.1，见 OFL.txt。

官方来源：Google Fonts 的 google/fonts 仓库，ofl/notosanssc/NotoSansSC[wght].ttf。

工程内的 NotoSansSC-Regular.ttf 是将官方可变字体固定到 wght=400 后的静态字体，未删除字形。处理工具为 fonttools 4.60.1 的 varLib.instancer。固定字重使界面与 OFD/PDF 替代字体在不同平台上使用相同的常规字形。

字体由 include_bytes! 编译进可执行文件；发布程序运行时不需要读取此目录、安装系统中文字体或联网。优先使用文档自带字体和可匹配的原字体，内置字体作为中文替代。

PDF 字体替代仅针对支持的编码；未知 CMap 不承诺正确显示，并在检查面板提示。字体替代也不保证与未嵌入的原始字体字形完全一致。
