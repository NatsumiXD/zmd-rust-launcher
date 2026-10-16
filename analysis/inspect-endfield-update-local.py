import pathlib,hashlib,json,configparser,re
r=pathlib.Path(r'D:\yj\Hypergryph Launcher\games\Arknights Endfield');out={'kind':'read-only-update-file-structure','files':[]}
for name in ['config.ini','game_files','verify_files','verify_files.json']:
 p=r/name;b=p.read_bytes();row={'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'md5':hashlib.md5(b).hexdigest(),'head_hex':b[:8].hex()}
 if name=='config.ini':
  t=b.decode('utf-8-sig',errors='replace');row['whitelisted_values']={m[1]:m[2].strip() for m in re.finditer(r'(?im)^\s*(game_version|version|appcode|channel|sub_channel)\s*=([^\r\n]*)',t)}
 if name.endswith('.json'):
  try:
   v=json.loads(b);row['root_type']=type(v).__name__;row['root_keys']=list(v) if isinstance(v,dict) else None;row['entry_count']=len(v)
   if isinstance(v,dict):row['value_types']={k:type(x).__name__ for k,x in v.items()};row['list_lengths']={k:len(x) for k,x in v.items() if isinstance(x,list)};row['list_item_keys']={k:list(x[0]) if isinstance(x[0],dict) else type(x[0]).__name__ for k,x in v.items() if isinstance(x,list) and x}
   elif isinstance(v,list) and v:row['first_entry_keys']=list(v[0]) if isinstance(v[0],dict) else type(v[0]).__name__
  except Exception as e:row['parse_error']=type(e).__name__
 out['files'].append(row)
p=pathlib.Path('analysis/game-update-local-structure.json');p.write_text(json.dumps(out,ensure_ascii=False,indent=2),encoding='utf-8');print(p.read_text(encoding='utf-8'))
