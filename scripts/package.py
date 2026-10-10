"""Package verified binaries, source, samples and third-party license texts."""
from pathlib import Path
import json, shutil, hashlib, tarfile, zipfile, sys, re
ROOT=Path(__file__).resolve().parents[1]
DIST=ROOT/'dist';DIST.mkdir(exist_ok=True)
meta=json.loads((ROOT/'tmp/cargo-metadata.json').read_text('utf-8-sig'))
license_root=DIST/'third-party-licenses';license_root.mkdir(exist_ok=True)
licenses=[]
for p in meta['packages']:
    if p['name']=='ofd-reader':continue
    base=Path(p['manifest_path']).parent
    texts=[]
    for f in base.rglob('*'):
        if not f.is_file():continue
        rel=f.relative_to(base)
        if len(rel.parts)>3:continue
        name=f.name.lower()
        if name.startswith(('license','copying','copyright')) or name in ['ofl.txt','ufl.txt','hack-regular.txt','emoji-icon-font-mit-license.txt']:
            target=license_root/(p['name']+'-'+p['version'])/rel
            target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(f,target);texts.append(str(target.relative_to(DIST)).replace('\\','/'))
    licenses.append({'name':p['name'],'version':p['version'],'license':p['license'],'repository':p['repository'],'license_texts':texts})
(DIST/'dependency-licenses.json').write_text(json.dumps(licenses,ensure_ascii=False,indent=2),'utf-8')
rustdocs=ROOT/'tools/rustup/toolchains/1.90.0-x86_64-pc-windows-gnu/share/doc/rust'
for name in ['COPYRIGHT.html','COPYRIGHT-library.html']:
    if (rustdocs/name).exists():
        folder=license_root/'rust-standard-library';folder.mkdir(exist_ok=True);shutil.copy2(rustdocs/name,folder/name)
# Documentation reflects the finished builds before this script is run.
common=['README.md','LICENSE','docs','samples']
binaries=[
 ('windows-x64','target/release/jian-yue.exe'),
 ('windows-x86','target/i686-pc-windows-gnu/release/jian-yue.exe'),
 ('windows-arm64','target/aarch64-pc-windows-gnullvm/release/jian-yue.exe'),
 ('linux-x64','target/x86_64-unknown-linux-gnu/release/jian-yue'),
 ('linux-x86','target/i686-unknown-linux-gnu/release/jian-yue'),
 ('linux-arm64','target/aarch64-unknown-linux-gnu/release/jian-yue'),
]
checksums=[]
def add_common(bundle):
    for entry in common:
        src=ROOT/entry;target=bundle/entry
        if src.is_dir():shutil.copytree(src,target,dirs_exist_ok=True)
        else:shutil.copy2(src,target)
    shutil.copytree(license_root,bundle/'third-party-licenses',dirs_exist_ok=True)
    fontlicenses=bundle/'licenses';fontlicenses.mkdir(exist_ok=True)
    shutil.copy2(ROOT/'assets/fonts/OFL.txt',fontlicenses/'NotoSansSC-OFL.txt')
    shutil.copy2(ROOT/'assets/fonts/README.md',fontlicenses/'NotoSansSC-README.md')
    shutil.copytree(ROOT/'assets/icons',bundle/'icons',dirs_exist_ok=True)
    shutil.copy2(DIST/'dependency-licenses.json',bundle/'dependency-licenses.json')
for name,path in binaries:
    source=ROOT/path
    if not source.is_file():raise RuntimeError(f'Missing binary: {source}')
    bundle=DIST/f'jian-yue-0.3.0-{name}';bundle.mkdir(exist_ok=True)
    shutil.copy2(source,bundle/('简阅.exe' if name.startswith('windows') else 'jian-yue'));add_common(bundle)
    if name=='windows-arm64':
        tool=ROOT/'tools/llvm-mingw-20261006-ucrt-x86_64'
        shutil.copy2(tool/'aarch64-w64-mingw32/bin/libunwind.dll',bundle/'libunwind.dll')
        (bundle/'licenses').mkdir(exist_ok=True)
        shutil.copy2(tool/'LICENSE.TXT',bundle/'licenses/LLVM-LICENSE.txt')
    if name.startswith('windows'):
        archive=DIST/(bundle.name+'.zip')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6,strict_timestamps=False) as z:
            for f in sorted(bundle.rglob('*')):
                if f.is_file():z.write(f,f.relative_to(DIST))
    else:
        archive=DIST/(bundle.name+'.tar.gz')
        with tarfile.open(archive,'w:gz') as tar:
            for f in sorted(bundle.rglob('*')):
                if f.is_file():
                    info=tar.gettarinfo(str(f),arcname=str(f.relative_to(DIST)))
                    info.mode=0o755 if f.name=='jian-yue' else 0o644
                    with f.open('rb') as stream:tar.addfile(info,stream)
    checksums.append((hashlib.sha256(archive.read_bytes()).hexdigest(),archive.name))
    print(archive.name,archive.stat().st_size)
sourcezip=DIST/'jian-yue-0.3.0-source.zip'
with zipfile.ZipFile(sourcezip,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for entry in ['Cargo.toml','Cargo.lock','README.md','LICENSE','build.rs','.gitignore','src','tests','scripts','docs','samples','assets','.github']:
        p=ROOT/entry
        if p.is_file():z.write(p,'jian-yue/'+entry)
        else:
            for f in sorted(p.rglob('*')):
                if f.is_file() and '__pycache__' not in f.parts:z.write(f,'jian-yue/'+str(f.relative_to(ROOT)).replace('\\','/'))
checksums.append((hashlib.sha256(sourcezip.read_bytes()).hexdigest(),sourcezip.name))
(DIST/'SHA256SUMS.txt').write_text(''.join(f'{sha}  {name}\n' for sha,name in checksums),'utf-8')
