"""Create a reproducible OFD demonstration with only original artwork and XML.
Optional Ubuntu-Light.ttf is used under its bundled UFL license.
"""
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
from xml.sax.saxutils import escape
from PIL import Image, ImageDraw
import io, sys
ROOT = Path(__file__).resolve().parents[1]
NS = 'http://www.ofdspec.org/2016'
def xml(body): return ('<?xml version="1.0" encoding="UTF-8"?>'+body).encode('utf-8')
def wrap(name,body,attrs=''): return f'<ofd:{name} xmlns:ofd="{NS}" {attrs}>{body}</ofd:{name}>'
seq = 100

def text(x,y,w,h,t,size=4,font=1,color='42 51 68'):
    global seq;seq+=1
    return f'<ofd:TextObject ID="{seq}" Boundary="{x} {y} {w} {h}" Font="{font}" Size="{size}"><ofd:FillColor Value="{color}"/><ofd:TextCode X="0" Y="{size}">{escape(t)}</ofd:TextCode></ofd:TextObject>'
def box(x,y,w,h,color,stroke='false'):
    global seq;seq+=1
    return f'<ofd:PathObject ID="{seq}" Boundary="{x} {y} {w} {h}" Fill="true" Stroke="{stroke}"><ofd:FillColor Value="{color}"/><ofd:AbbreviatedData>M 0 0 L {w} 0 L {w} {h} L 0 {h} C</ofd:AbbreviatedData></ofd:PathObject>'
def page(objects): return xml(wrap('Page','<ofd:Content><ofd:Layer ID="90" Type="Body">'+objects+'</ofd:Layer></ofd:Content>'))
entries={}
entries['OFD.xml']=xml(wrap('OFD','<ofd:DocBody><ofd:DocInfo><ofd:DocID>JIANYUE-DEMO-20261010</ofd:DocID><ofd:Title>简阅 示例档案</ofd:Title><ofd:Author>简阅</ofd:Author><ofd:Creator>Original demo generator</ofd:Creator><ofd:CreationDate>2026-10-10</ofd:CreationDate></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody>','Version="1.0" DocType="OFD-A"'))
outlines=''.join(f'<ofd:OutlineElem Title="{title}"><ofd:Actions><ofd:Action Event="CLICK"><ofd:Goto><ofd:Dest Type="XYZ" PageID="{i}" Left="0" Top="0"/></ofd:Goto></ofd:Action></ofd:Actions></ofd:OutlineElem>' for i,title in [(1,'欢迎与阅读功能'),(2,'矢量图形与图像'),(3,'档案检查与附件')])
entries['Doc_0/Document.xml']=xml(wrap('Document','<ofd:CommonData><ofd:MaxUnitID>9999</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:PublicRes>PublicRes.xml</ofd:PublicRes><ofd:DocumentRes>DocumentRes.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages>'+''.join(f'<ofd:Page ID="{i}" BaseLoc="Pages/Page_{i-1}/Content.xml"/>' for i in range(1,4))+'</ofd:Pages><ofd:Outlines>'+outlines+'</ofd:Outlines><ofd:Attachments>Attachments.xml</ofd:Attachments>'))
font_paths=list((ROOT/'tools/cargo/registry/src').glob('*/epaint_default_fonts-*/fonts/Ubuntu-Light.ttf'))
font_bytes=font_paths[0].read_bytes() if font_paths else None
if font_bytes is None and (ROOT/'samples/demo.ofd').exists():
    with ZipFile(ROOT/'samples/demo.ofd') as old:
        if 'Doc_0/Res/Ubuntu-Light.ttf' in old.namelist():font_bytes=old.read('Doc_0/Res/Ubuntu-Light.ttf')
if font_bytes is None:raise RuntimeError('Ubuntu-Light.ttf is required; preserve the distributed sample or prepare the font and its UFL license.')
fontfile='<ofd:FontFile>Ubuntu-Light.ttf</ofd:FontFile>'
entries['Doc_0/PublicRes.xml']=xml(wrap('Res','<ofd:Fonts><ofd:Font ID="1" FontName="Microsoft YaHei" FamilyName="Noto Sans CJK SC"/><ofd:Font ID="2" FontName="Ubuntu Light" FamilyName="Ubuntu">'+fontfile+'</ofd:Font></ofd:Fonts>','BaseLoc="Res"'))
entries['Doc_0/Res/Ubuntu-Light.ttf']=font_bytes
entries['Doc_0/DocumentRes.xml']=xml(wrap('Res','<ofd:MultiMedias><ofd:MultiMedia ID="10" Type="Image" Format="PNG"><ofd:MediaFile>landscape.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias>','BaseLoc="Res"'))
image=Image.new('RGB',(720,320),(225,237,248));draw=ImageDraw.Draw(image)
draw.ellipse((520,35,610,125),fill=(250,198,87));draw.polygon([(0,320),(170,100),(370,320)],fill=(108,143,181));draw.polygon([(190,320),(440,70),(720,320)],fill=(52,91,136));draw.rectangle((0,280,720,320),fill=(31,68,110));b=io.BytesIO();image.save(b,format='PNG');entries['Doc_0/Res/landscape.png']=b.getvalue()
p1=box(0,0,210,297,'255 255 255')+box(0,0,210,8,'44 98 187')+text(22,28,166,16,'简阅',12,1,'44 98 187')+text(22,50,166,13,'归档文档阅读器',7)+text(22,75,166,10,'一份文档，跨越不同平台与架构。',4.8)
p1+=box(22,98,166,46,'240 245 253')+text(29,108,152,10,'Windows  /  Linux  /  x86  /  ARM64',4.7,2)+text(29,123,152,10,'Rust 原生桌面 · 本地解析 · 只读阅读',4.2)
for y,title,detail in [(165,'清晰阅读','使用顶部工具栏翻页、缩放、适合宽度和旋转。'),(197,'快速查找','按 Ctrl + F 搜索文字，点击结果跳转到页面。'),(229,'档案检查','查看标准条款提示，明确字体、签名和附件情况。')]:
    p1+=text(22,y,166,10,title,5)+text(22,y+13,166,10,detail,3.9)
p1+=text(22,278,166,8,'OFD-A · 演示档案 · 原文件保持只读',3.2)
entries['Doc_0/Pages/Page_0/Content.xml']=page(p1)
p2=text(22,25,166,14,'矢量图形与图像',7)+text(22,46,166,10,'以下内容由 OFD 图元直接绘制。',4)
p2+=box(22,68,48,24,'44 98 187')+box(80,68,48,24,'81 149 121')+box(138,68,48,24,'233 173 76')
p2+='<ofd:PathObject ID="500" Boundary="22 108 166 50" Stroke="true" Fill="false" LineWidth="1.2"><ofd:StrokeColor Value="44 98 187"/><ofd:AbbreviatedData>M 0 40 B 35 0 65 50 95 18 Q 125 0 166 30</ofd:AbbreviatedData></ofd:PathObject>'
p2+='<ofd:ImageObject ID="501" Boundary="22 175 166 73.8" ResourceID="10" CTM="166 0 0 73.8 0 0" Interpolate="true"/>'
p2+=text(22,264,166,10,'支持直线、曲线、颜色、图像和嵌入字体外轮廓。',3.8)
entries['Doc_0/Pages/Page_1/Content.xml']=page(p2)
p3=text(22,25,166,14,'面向长期保存的阅读',7)
for y,title,detail in [(66,'标准依据','参考 GB/T 42133—2022 档案应用指南。'),(109,'只读保护','OFD-A 显示只读提示，不改写源文件。'),(152,'透明提示','未嵌入字体和未支持特性会出现在检查面板。'),(195,'附件保留','切换到附件面板，可另存示例说明文本。')]:
    p3+=box(22,y-4,3,25,'44 98 187')+text(32,y,154,10,title,5)+text(32,y+14,154,10,detail,3.6)
p3+=text(22,259,166,17,'此示例特意保留未嵌入的中文字体，供演示档案检查提示。',3.5)
entries['Doc_0/Pages/Page_2/Content.xml']=page(p3)
entries['Doc_0/Attachments.xml']=xml(wrap('Attachments','<ofd:Attachment ID="900" Name="示例说明.txt" Format="TXT" Size="1"><ofd:FileLoc>Attachments/readme.txt</ofd:FileLoc></ofd:Attachment>'))
entries['Doc_0/Attachments/readme.txt']='简阅 示例附件。\n此文件用于验证附件列表与原始字节提取。\n'.encode('utf-8')
out=ROOT/'samples/demo.ofd'
with ZipFile(out,'w',ZIP_DEFLATED) as z:
    for path,data in entries.items(): z.writestr(path,data)
if font_paths:
    license_dir=ROOT/'samples/licenses';license_dir.mkdir(exist_ok=True)
    (license_dir/'Ubuntu-font-license.txt').write_bytes((font_paths[0].parent/'UFL.txt').read_bytes())
print(out)
