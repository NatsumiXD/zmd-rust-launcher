const fs = require('fs');
const crypto = require('crypto');
const path = require('path');
const root = 'D:\\yj\\Hypergryph Launcher';
const logs = 'C:\\Users\\miang\\AppData\\LocalLow\\Hypergryph\\33a0a6296a20400d503c59ac0fd6341e\\logs';
const sha256 = b => crypto.createHash('sha256').update(b).digest('hex');
const urlWithoutQuery = s => String(s).split('?')[0];
const result = {kind:'read-only-whitelisted-local-observation', sources:[], historicalLauncherMetadata:[], historicalQueryArguments:[], updaterEvents:[], nativeStrings:[], webStrings:[]};
for (const name of ['games.log','updater.log']) {
  const file = path.join(logs,name), bytes = fs.readFileSync(file);
  result.sources.push({file,size:bytes.length,sha256:sha256(bytes)});
  const lines = bytes.toString('utf8').split(/\r?\n/);
  if (name==='games.log') {
    for (let i=0;i<lines.length;i++) {
      if ([178,29199,51628].includes(i) && /clhc enter Post_ProtocolVer_LauncherVer_GameVer_Req /.test(lines[i])) {
        const q={line:i+1};
        for(const key of ['strProtocolVersion','strLauncherVersion','strLanguage','strAppCode','strChannel','strGameVersion','strGameAppCode','strGameChannel','strGameSubChannel','bContainProtocolVersion','bContainGameVersion','bContainLauncherVersion','bGameInstallPath_SSD','strTargetApp']) {
          const m=lines[i].match(new RegExp('\\b'+key+':([^,\\s]*)'));
          if(m)q[key]=m[1];
        }
        result.historicalQueryArguments.push(q);
      }
      const match=lines[i].match(/m_lastestLauncherInfo:(.*)$/);
      if (!match) continue;
      try {
        const m=JSON.parse(match[1]);
        if (!m.zip_package_url && i!==51663) continue;
        result.historicalLauncherMetadata.push({line:i+1,action:m.action,request_version:m.request_version,version:m.version,md5:m.md5,package_size:m.package_size,total_size:m.total_size,zip_package_url_without_query:urlWithoutQuery(m.zip_package_url||''),queryKeys:m.zip_package_url?[...new URL(m.zip_package_url).searchParams.keys()]:[]});
      } catch {}
    }
  } else {
    for (let i=0;i<lines.length;i++) {
      const s=lines[i];
      if (!/Download strPackageUrl|OnDownloadFinished|OnDecompress|OnCheckFilesCompleteFinished|OnCheckFilesFinished|OnCopyFinish/.test(s)) continue;
      const event={line:i+1};
      const url=s.match(/https?:\/\/[^\s,"<>]+/);
      if (url) event.url_without_query=urlWithoutQuery(url[0]);
      const method=s.match(/\b(?:On[A-Za-z]+|Download)\b/);
      if (method) event.method=method[0];
      for (const key of ['bRet','nCode']) { const m=s.match(new RegExp('\\b'+key+':(true|false|[0-9]+)')); if(m)event[key]=m[1]; }
      result.updaterEvents.push(event);
    }
  }
}
for (const name of ['Games.exe','Updater.exe','Launcher.exe']) {
  const file=path.join(root,'1.6.0',name), bytes=fs.readFileSync(file);
  result.sources.push({file,size:bytes.length,sha256:sha256(bytes)});
  const text=bytes.toString('latin1');
  for (const m of text.matchAll(/[\x20-\x7e]{6,}/g)) {
    if (!/^(?:https?:\/\/|\/api\/)/.test(m[0])) continue;
    if (!/launcher|update|version|batch|download/i.test(m[0])) continue;
    if (m[0].length>1000) continue;
    result.nativeStrings.push({file:name,offset:m.index,value:urlWithoutQuery(m[0])});
  }
}
const file=path.join(root,'1.6.0','res','web','index.ff0a17.js'), bytes=fs.readFileSync(file), text=bytes.toString('utf8');
result.sources.push({file,size:bytes.length,sha256:sha256(bytes)});
for (const term of ['https://launcher.hypergryph.com','/api/proxy/web/batch_proxy','version_url','get_single_ent']) {
  let from=0; while(true){const i=text.indexOf(term,from);if(i<0)break;result.webStrings.push({term,charOffset:i});from=i+term.length;}
}
fs.writeFileSync(path.join(__dirname,'games-acquisition-resources-observation.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
