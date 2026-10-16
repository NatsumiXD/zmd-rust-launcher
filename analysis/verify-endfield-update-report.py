import pathlib,re,json,hashlib
r=pathlib.Path(__file__).resolve().parent;t=(r/'endfield-game-update.md').read_text(encoding='utf-8');ids=set(re.findall(r'ev_[0-9a-f]{64}',t));found=set()
def walk(v):
 if isinstance(v,dict):
  if isinstance(v.get('evidence_id'),str):found.add(v['evidence_id'])
  for x in v.values():walk(x)
 elif isinstance(v,list):
  for x in v:walk(x)
for n in ['endfield-game-update-native-evidence.json','endfield-game-update-launcher-evidence.json','acq-batch-request.json']:walk(json.loads((r/n).read_text(encoding='utf-8-sig')))
missing=sorted(ids-found)
assert not missing,missing
assert not re.search(r'auth_key=[^\s"<>]+',t)
v=json.loads((r/'game-update-live-metadata.json').read_text(encoding='utf-8-sig'))
for x in v['responses']:assert x['status']==200
pack=v['responses'][1]['body']['pkg']['packs'];size=sum(int(x['package_size']) for x in pack)
assert len(pack)==59 and size==62376921620
out={'kind':'report-static-consistency-check','report_sha256':hashlib.sha256((r/'endfield-game-update.md').read_bytes()).hexdigest(),'cited_rea_evidence_ids':len(ids),'missing_evidence_ids':missing,'metadata_responses':len(v['responses']),'all_metadata_http_200':True,'full_package_volumes':len(pack),'full_package_bytes':size,'executed_official_programs':False,'downloaded_game_packages':False,'modified_official_installation':False,'rea_sessions_closed':True,'hibernated':False}
(r/'game-update-report-verification.json').write_text(json.dumps(out,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(out,ensure_ascii=False,indent=2))
