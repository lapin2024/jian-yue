"""Original multi-page PDF fixture: text, CJK fallback, vector graphics and outline."""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import A4
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont
ROOT=Path(__file__).resolve().parents[1]
pdfmetrics.registerFont(UnicodeCIDFont('STSong-Light'))
p=ROOT/'samples/demo.pdf'
c=canvas.Canvas(str(p),pagesize=A4,pageCompression=1)
c.setTitle('简阅 - PDF 示例');c.setAuthor('简阅')
w,h=A4
for i,(title,subtitle) in enumerate([('PDF document reading','Open, search and print PDFs locally.'),('Continuous reading','Scroll down through pages without switching views.'),('Print and export','Select page ranges and copies, or save a print PDF.')],1):
    c.bookmarkPage(f'page{i}');c.addOutlineEntry(title,f'page{i}',level=0)
    c.setFillColorRGB(.1,.56,.85);c.rect(0,h-12,w,12,fill=1,stroke=0)
    c.setFont('Helvetica',30);c.drawString(58,h-105,title)
    c.setFont('Helvetica',14);c.setFillColorRGB(.2,.25,.32);c.drawString(58,h-145,subtitle)
    c.setFont('STSong-Light',22);c.drawString(58,h-205,['内置中文字体与 PDF 阅读','向下滚动，连续浏览每一页','打印、页码范围与文件保护'][i-1])
    c.setFillColorRGB(.93,.96,.99);c.roundRect(58,h-360,w-116,100,8,fill=1,stroke=0)
    c.setFillColorRGB(.2,.3,.43);c.setFont('Helvetica',16);c.drawString(77,h-300,'OFD  /  PDF  /  Windows  /  Linux')
    c.setFont('STSong-Light',13);c.drawString(77,h-332,'中文显示无需额外安装字体。')
    c.setStrokeColorRGB(.1,.56,.85);c.setLineWidth(2);path=c.beginPath();path.moveTo(58,h-440);path.curveTo(170,h-380,330,h-530,w-58,h-425);c.drawPath(path)
    c.setFont('Helvetica',11);c.setFillColorRGB(.5,.55,.62);c.drawString(58,38,f'JianYue 0.3.0  |  Original PDF sample  |  {i} / 3')
    c.showPage()
c.save();print(p)
