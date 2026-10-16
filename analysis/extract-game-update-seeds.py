import json,re,struct,pathlib,hashlib
p=pathlib.Path(r'D:\yj\Hypergryph Launcher\1.6.0\Games.exe'); b=p.read_bytes(); pe=struct.unpack_from('<I',b,60)[0]
n=struct.unpack_from('<H',b,pe+6)[0]; z=struct.unpack_from('<H',b,pe+20)[0]; opt=pe+24; base=struct.unpack_from('<Q',b,opt+24)[0]; sections=[]
for i in range(n):
 o=opt+z+i*40; vs,va,rs,rp=struct.unpack_from('<IIII',b,o+8); sections.append((rp,rs,va))
def addr(o):
 for rp,rs,va in sections:
  if rp<=o<rp+rs:return hex(base+va+o-rp)
 return None
pat=re.compile(r'(get_latest|/api/|GameAssist|GameVer|GameInfo|DownloadGame|patch|manifest|filelist|file_list|version.json|md5|unzip|decompress|\.ini|\.json|downloadPath|local_ver|local_game|SetLocal|GetLocal|WriteLocal|SaveLocal|LoadLocal|GameConfig)',re.I)
rows=[]
for m in re.finditer(rb'[\x20-\x7e]{6,}',b):
 s=m[0].decode('ascii')
 if pat.search(s) and len(s)<1500: rows.append({'address':addr(m.start()),'offset':m.start(),'text':s})
out={'path':str(p),'sha256':hashlib.sha256(b).hexdigest(),'scope':'ASCII printable strings only; seeds are not behavior evidence','rows':rows}
pathlib.Path('analysis/game-update-seeds.json').write_text(json.dumps(out,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'sha256':out['sha256'],'count':len(rows),'rows':[r for r in rows if 16400000<r['offset']<16600000 and re.search(r'local_ver|local_game|SetLocal|GetLocal|WriteLocal|SaveLocal|LoadLocal|GameConfig',r['text'],re.I)]},ensure_ascii=False,indent=2))
