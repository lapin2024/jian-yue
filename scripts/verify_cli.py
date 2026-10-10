"""Exercise a built Windows binary through its public command line."""
import hashlib, json, subprocess, sys, os
from pathlib import Path
from zipfile import ZipFile
ROOT=Path(__file__).resolve().parents[1]
exe=Path(sys.argv[1]).resolve()
name=sys.argv[2] if len(sys.argv)>2 else 'windows-x64'
out=ROOT/'tmp/verification'/name
out.mkdir(parents=True,exist_ok=True)
source=ROOT/'samples/demo.ofd'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
before=sha(source)
checks=[]
childenv=os.environ.copy();childenv["OFD_READER_NO_SYSTEM_FONTS"]="1"
def run(*args,success=True):
    r=subprocess.run([str(exe),*map(str,args)],capture_output=True,text=True,encoding='utf-8',errors='replace',env=childenv)
    assert (r.returncode==0)==success, f'command failed: {args}: {r.stderr}'
    checks.append({'args':[str(a) for a in args],'exit_code':r.returncode,'stderr':r.stderr})
    return r
run('--inspect',source,'--output',out/'report.json')
report=json.loads((out/'report.json').read_text('utf-8'))
assert report['pages']==3 and report['metadata']['doc_type']=='OFD-A'
run('--render',source,'--page',1,'--zoom',1,'--output',out/'page1.png')
run('--render',source,'--page',2,'--zoom',1,'--rotate',90,'--output',out/'page2-rotated.png')
run('--text',source,'--output',out/'text.txt')
assert '归档文档阅读器' in (out/'text.txt').read_text('utf-8')
run('--extract-attachment',source,'--attachment',1,'--output',out/'attachment.txt')
with ZipFile(source) as z: assert (out/'attachment.txt').read_bytes()==z.read('Doc_0/Attachments/readme.txt')
run('--render',source,'--page',0,'--output',out/'invalid.png',success=False)
run('--render',source,'--page',99,'--output',out/'invalid.png',success=False)
run('--inspect',source,'--output',source,success=False)
run('--render',source,'--rotate',12,'--output',out/'invalid.png',success=False)
pdf_source=ROOT/'samples/demo.pdf'
pdf_before=sha(pdf_source)
run('--inspect',pdf_source,'--output',out/'pdf-report.json')
assert json.loads((out/'pdf-report.json').read_text('utf-8'))['pages']==3
run('--render',pdf_source,'--output',out/'pdf-page1.png')
run('--text',pdf_source,'--output',out/'pdf-text.txt')
assert 'PDF document reading' in (out/'pdf-text.txt').read_text('utf-8')
assert '内置中文字体' in (out/'pdf-text.txt').read_text('utf-8')
run('--render',pdf_source,'--page',2,'--rotate',90,'--output',out/'pdf-rotated.png')
run('--print-pdf',source,'--pages','1,3','--dpi',96,'--output',out/'ofd-print.pdf')
run('--print-pdf',pdf_source,'--pages','2-3','--dpi',96,'--output',out/'pdf-print.pdf')
r=run('--inspect',out/'pdf-print.pdf')
assert json.loads(r.stdout)['pages']==2
run('--print-pdf',pdf_source,'--pages','0','--output',out/'bad-print.pdf',success=False)
run('--convert',pdf_source,'--pages','1-2','--dpi',96,'--output',out/'converted.ofd')
r=run('--inspect',out/'converted.ofd');assert json.loads(r.stdout)['pages']==2
run('--convert',source,'--dpi',96,'--output',out/'converted.pdf')
run('--convert',pdf_source,'--pages','1,3','--dpi',96,'--output',out/'pages.zip')
with ZipFile(out/'pages.zip') as z:assert len(z.namelist())==2
run('--render',pdf_source,'--project',ROOT/'samples/demo.reader.json','--output',out/'edited.png')
run('--text',pdf_source,'--project',ROOT/'samples/demo.reader.json','--output',out/'edited.txt')
assert '新增文字批注' in (out/'edited.txt').read_text('utf-8')
run('--convert',pdf_source,'--project',ROOT/'samples/demo.reader.json','--dpi',96,'--output',out/'edited.ofd')
run('--convert',source,'--output',out/'invalid.docx',success=False)
childenv['OFD_READER_CERT_PASSWORD']='fixture-only'
certificate=ROOT/'tests/fixtures/test-identity.p12'
run('--sign',pdf_source,'--certificate',certificate,'--output',out/'signed.pdf')
r=run('--verify-signature',out/'signed.pdf');assert json.loads(r.stdout)[0]['file_fully_covered']
run('--sign',source,'--certificate',certificate,'--detached','--output',out/'document.p7s')
r=run('--verify-signature',source,'--signature',out/'document.p7s');assert json.loads(r.stdout)[0]['integrity_valid']
run('--verify-signature',pdf_source,'--signature',out/'document.p7s',success=False)
run('--sign',out/'signed.pdf','--certificate',certificate,'--output',out/'signed-again.pdf',success=False)
run('--convert',pdf_source,'--output',pdf_source,success=False)
assert sha(source)==before,'source file changed'
assert sha(pdf_source)==pdf_before,'PDF source changed'
(out/'cli-checks.json').write_text(json.dumps({'binary':str(exe),'binary_sha256':sha(exe),'source_sha256':before,'pdf_source_sha256':pdf_before,'system_fonts_disabled':True,'checks':checks,'all_passed':True},ensure_ascii=False,indent=2),'utf-8')
print(f'{name}: {len(checks)} CLI checks passed; source hash unchanged')
