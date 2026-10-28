"""Private loopback TLS API + synthetic exec credential plugin; no user kubeconfig."""
import argparse,datetime,ipaddress,json,ssl,subprocess,threading
from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
from pathlib import Path
from urllib.parse import urlsplit,parse_qs
from cryptography import x509
from cryptography.hazmat.primitives import hashes,serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

parser=argparse.ArgumentParser();parser.add_argument('--directory',required=True);args=parser.parse_args()
root=Path(args.directory).resolve();root.mkdir(parents=True,exist_ok=True);lock=threading.Lock()
stats=dict(discovery=0,lists=0,forbidden=0,bad_scope=0,bad_auth=0,resources={})
catalog={'pods':('Pod','v1','/api/v1'),'services':('Service','v1','/api/v1'),'deployments':('Deployment','apps/v1','/apis/apps/v1')}
def write(name,data):
 p=root/(name+'.tmp');p.write_text(json.dumps(data),encoding='utf-8');p.replace(root/name)
key=rsa.generate_private_key(public_exponent=65537,key_size=2048)
subject=x509.Name([x509.NameAttribute(NameOID.COMMON_NAME,'TideTerm synthetic API')])
now=datetime.datetime.now(datetime.timezone.utc)
cert=(x509.CertificateBuilder().subject_name(subject).issuer_name(subject).public_key(key.public_key()).serial_number(x509.random_serial_number())
 .not_valid_before(now-datetime.timedelta(days=1)).not_valid_after(now+datetime.timedelta(days=2))
 .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]),False)
 .add_extension(x509.BasicConstraints(ca=True,path_length=None),True).sign(key,hashes.SHA256()))
(root/'ca.crt').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
(root/'server.key').write_bytes(key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()))
plugin=root/'credential.ps1'
plugin.write_text('param([string]$Counter)\n$n=0; if ([IO.File]::Exists($Counter)) { $n=[int][IO.File]::ReadAllText($Counter) }; [IO.File]::WriteAllText($Counter,[string]($n+1)); [Console]::Write(\'{"apiVersion":"client.authentication.k8s.io/v1","kind":"ExecCredential","status":{"token":"fixture-token"}}\')\n',encoding='utf-8')
(root/'plugin-count.txt').write_text('0')

class Handler(BaseHTTPRequestHandler):
 def log_message(self,*_):pass
 def reply(self,code,data):
  encoded=json.dumps(data).encode();self.send_response(code);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(encoded)));self.end_headers();self.wfile.write(encoded)
 def do_GET(self):
  parts=urlsplit(self.path);path=parts.path
  with lock:
   if self.headers.get('Authorization')!='Bearer fixture-token':
    stats['bad_auth']+=1;write('stats.json',stats);self.reply(401,dict(kind='Status',apiVersion='v1',status='Failure',reason='Unauthorized',code=401));return
   if path in ('/api','/apis','/api/v1','/apis/apps','/apis/apps/v1','/version'):
    stats['discovery']+=1;write('stats.json',stats)
    apps=dict(name='apps',versions=[dict(groupVersion='apps/v1',version='v1')],preferredVersion=dict(groupVersion='apps/v1',version='v1'))
    resources=lambda group: [dict(name=name,singularName=kind.lower(),namespaced=True,kind=kind,verbs=['get','list']) for name,(kind,_,base) in catalog.items() if base==group]
    data={'/api':dict(kind='APIVersions',apiVersion='v1',versions=['v1'],serverAddressByClientCIDRs=[]),'/apis':dict(kind='APIGroupList',apiVersion='v1',groups=[apps]),
     '/api/v1':dict(kind='APIResourceList',apiVersion='v1',groupVersion='v1',resources=resources('/api/v1')),
     '/apis/apps':dict(kind='APIGroup',apiVersion='v1',**apps),
     '/apis/apps/v1':dict(kind='APIResourceList',apiVersion='v1',groupVersion='apps/v1',resources=resources('/apis/apps/v1')),
     '/version':dict(major='1',minor='36',gitVersion='v1.36.0')}[path]
    self.reply(200,data);return
   resource=next((name for name,(_,_,base) in catalog.items() if path==base+'/namespaces/fixture/'+name),None)
   if resource:
    kind,version,_=catalog[resource];prefix=kind.lower()
    stats['lists']+=1
    stats['resources'][resource]=stats['resources'].get(resource,0)+1
    query=parse_qs(parts.query)
    if 'continue' in query or query.get('limit') not in (None,['0']) or 'watch' in query:stats['bad_scope']+=1
    write('stats.json',stats);state=json.loads((root/'state.json').read_text(encoding='utf-8'))
    if state.get('fail'):self.reply(403,dict(kind='Status',apiVersion='v1',status='Failure',reason='Forbidden',message='synthetic RBAC denial',code=403));return
    items=[dict(kind=kind,apiVersion=version,metadata=dict(uid=f'fixture-{prefix}-uid-{i:03}',name=('bad\0name' if state.get('nul') and i==0 else f'{prefix}-{i:03}' + ('.' + 'a'*63 + '.' + 'b'*63 + '.' + 'c'*63 if state.get('long_names') else '')),namespace=('other' if state.get('wrong_namespace') else 'fixture'))) for i in range(state.get('count',2))]
    self.reply(200,dict(kind=kind+'List',apiVersion=version,metadata={},items=items));return
   stats['forbidden']+=1;write('stats.json',stats);self.reply(405,dict(kind='Status',apiVersion='v1',status='Failure',reason='MethodNotAllowed',code=405))
 def do_POST(self):
  with lock:stats['forbidden']+=1;write('stats.json',stats)
  self.reply(405,dict(kind='Status',apiVersion='v1',status='Failure',code=405))
 do_DELETE=do_POST
 do_PUT=do_POST

write('state.json',dict(count=2));write('stats.json',stats)
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(root/'ca.crt',root/'server.key');server.socket=tls.wrap_socket(server.socket,server_side=True)
pwsh=subprocess.check_output(['pwsh','-NoProfile','-Command','(Get-Process -Id $PID).Path'],text=True).strip()
config=dict(apiVersion='v1',kind='Config',clusters=[dict(name='fixture',cluster={'server':f'https://127.0.0.1:{server.server_port}','certificate-authority':str(root/'ca.crt')})],contexts=[dict(name='fixture',context=dict(cluster='fixture',user='fixture',namespace='wrong-default'))],users=[dict(name='fixture',user=dict(exec=dict(apiVersion='client.authentication.k8s.io/v1',command=pwsh,args=['-NoProfile','-NonInteractive','-File',str(plugin),str(root/'plugin-count.txt')],interactiveMode='Never')))])
config['contexts'].append(dict(name='/fixture/context',context=dict(cluster='fixture',user='fixture',namespace='wrong-default')))
(root/"scope space's.json").write_text(json.dumps(config),encoding='utf-8')
write('ready.json',dict(port=server.server_port,config=str(root/"scope space's.json")))
server.serve_forever()
