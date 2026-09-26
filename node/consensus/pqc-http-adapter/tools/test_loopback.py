#!/usr/bin/env python3
"""Bounded compatibility checks against the actual adapter and a private IPC fixture.
The fixture does not execute consensus or establish production qualification.
"""
import argparse,base64,hashlib,http.client,json,os,pathlib,queue,socket,struct,subprocess,tempfile,threading,time
PROFILE='dytallix-pqc-http-local-v1'
ORIGIN='http://127.0.0.1:4173'
class EngineFixture:
 def __init__(self,path):
  self.path=path;self.listener=socket.socket(socket.AF_UNIX);self.listener.bind(str(path));os.chmod(path,0o600);self.listener.listen(4);self.listener.settimeout(.2)
  self.requests=[];self.failure=None;self.stop=threading.Event();self.reply_override=None
  self.thread=threading.Thread(target=self.run,daemon=True);self.thread.start()
 def run(self):
  while not self.stop.is_set():
   try:c,_=self.listener.accept()
   except socket.timeout:continue
   except OSError:return
   try:
    with c:
     c.settimeout(3)
     def read(n):
      result=b''
      while len(result)<n:
       chunk=c.recv(n-len(result))
       if not chunk:raise ValueError('IPC write side closed before response')
       result+=chunk
      return result
     n=struct.unpack('>I',read(4))[0];assert 0<n<=2097152
     request=json.loads(read(n));assert set(request)=={'version','method','path','query','body_base64','remote_addr'}
     assert request['version']==1;body=base64.b64decode(request['body_base64'],validate=True);assert base64.b64encode(body).decode()==request['body_base64']
     self.requests.append((request,body))
     if self.reply_override is not None:
      reply=self.reply_override;self.reply_override=None
     else:
      rpc=json.loads(body) if body else {'id':'get-fixture'}
      result=json.dumps({'jsonrpc':'2.0','id':rpc['id'],'result':{'fixture':True}},separators=(',',':')).encode()
      reply={'version':1,'status':200,'headers':{'Content-Type':'application/json','Cache-Control':'no-store'},'body_base64':base64.b64encode(result).decode()}
     raw=json.dumps(reply,separators=(',',':')).encode();c.sendall(struct.pack('>I',len(raw))+raw)
   except Exception as e:self.failure=repr(e)
 def close(self):self.stop.set();self.listener.close();self.thread.join(timeout=2)

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--binary',type=pathlib.Path,required=True);ap.add_argument('--output',type=pathlib.Path,required=True);a=ap.parse_args()
 if a.output.exists():raise SystemExit('Refuse to overwrite evidence')
 binary=a.binary.resolve();binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest();checks=[]
 def check(name,condition):
  if not condition:raise AssertionError(name)
  checks.append(name)
 with tempfile.TemporaryDirectory(prefix='dyt-pqc-http-fixture-') as td:
  home=pathlib.Path(td).resolve();(home/'data').mkdir(mode=0o700);engine=EngineFixture(home/'data/rpc.sock');process=None
  args=[str(binary),'--profile',PROFILE,'--home',str(home),'--listen','127.0.0.1:0']
  try:
   for name,extra in [('production',['--production']),('unknown-profile',['--profile','wrong']),('public-bind',['--profile',PROFILE,'--home',str(home),'--listen','0.0.0.0:0'])]:
    out=subprocess.run([str(binary),*extra],capture_output=True,timeout=3);check(name+'-refused',out.returncode!=0 and not out.stdout)
   def start():
    p=subprocess.Popen(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    line=queue.Queue()
    threading.Thread(target=lambda:line.put(p.stdout.readline()),daemon=True).start()
    try:ready=json.loads(line.get(timeout=5))
    except Exception:
     p.kill();p.wait();raise
    check('ready-remains-no-go',ready['launch_status']=='NO_GO' and ready['production_authorized'] is False)
    host,port=ready['listen'].rsplit(':',1);return p,host,int(port)
   process,host,port=start()
   def request(method,path='/',body=None,headers=None,chunked=False):
    connection=http.client.HTTPConnection(host,port,timeout=4)
    try:
     connection.request(method,path,body=body,headers=headers or {},encode_chunked=chunked)
     response=connection.getresponse();raw=response.read();return response.status,dict((k.lower(),v)for k,v in response.getheaders()),raw
    finally:connection.close()
   body=b'{"jsonrpc":"2.0", "id":"native-1","method":"abci_query","params":{"path":"/ordinary/profile"}}'
   status,headers,raw=request('POST',body=body,headers={'Content-Type':'application/json','X-Forwarded-For':'203.0.113.1'})
   check('native-post-success',status==200 and json.loads(raw)['id']=='native-1')
   check('raw-json-bytes-preserved',engine.requests[-1][1]==body)
   check('remote-address-derived-from-peer',engine.requests[-1][0]['remote_addr'].startswith('127.0.0.1:'))
   check('response-header-preserved',headers.get('cache-control')=='no-store' and headers.get('content-type')=='application/json')
   check('no-implicit-cors-without-origin','access-control-allow-origin'not in headers)
   chunks=[body[:21],body[21:]]
   status,headers,raw=request('POST',body=iter(chunks),headers={'Content-Type':'application/json','Origin':ORIGIN},chunked=True)
   check('chunked-body-compatible',status==200 and engine.requests[-1][1]==body)
   check('approved-origin-returned',headers.get('access-control-allow-origin')==ORIGIN and headers.get('vary')=='Origin')
   status,headers,raw=request('GET','/abci_query?path=%22%2Fordinary%2Fprofile%22&height=0')
   check('uri-get-compatible',status==200 and engine.requests[-1][0]['method']=='GET')
   check('raw-query-preserved',engine.requests[-1][0]['query']=='path=%22%2Fordinary%2Fprofile%22&height=0' and engine.requests[-1][1]==b'')
   before=len(engine.requests)
   status,headers,raw=request('OPTIONS',headers={'Origin':ORIGIN,'Access-Control-Request-Method':'POST','Access-Control-Request-Headers':'content-type'})
   check('browser-preflight',status==204 and headers.get('access-control-allow-origin')==ORIGIN and raw==b'')
   cases=[('unapproved-origin','POST','/',body,{'Content-Type':'application/json','Origin':'https://example.invalid'},403),('websocket-explicitly-unsupported','GET','/websocket',None,{},501),('upgrade-explicitly-unsupported','GET','/status',None,{'Upgrade':'websocket'},501),('root-browsing-explicitly-unsupported','GET','/',None,{},501),('form-post-explicitly-unsupported','POST','/abci_query',b'path=x',{'Content-Type':'application/x-www-form-urlencoded'},501),('method-explicitly-unsupported','PUT','/',None,{},405),('preflight-extra-header-rejected','OPTIONS','/',None,{'Origin':ORIGIN,'Access-Control-Request-Method':'POST','Access-Control-Request-Headers':'authorization'},403)]
   for name,method,path,payload,h,want in cases:
    status,_,_=request(method,path,payload,h);check(name,status==want)
   check('rejected-and-preflight-requests-not-forwarded',len(engine.requests)==before)
   engine.reply_override={'version':1,'status':200,'headers':{'Access-Control-Allow-Origin':'*'},'body_base64':'e30='}
   status,headers,_=request('POST',body=body,headers={'Content-Type':'application/json','Origin':ORIGIN})
   check('engine-cannot-override-origin-policy',status==502 and headers.get('access-control-allow-origin')==ORIGIN)
   # Restart the stateless adapter against the same socket; no chain state claim.
   process.terminate();process.wait(timeout=3);process=None
   process,host,port=start()
   status,_,raw=request('POST',body=body,headers={'Content-Type':'application/json'})
   check('adapter-restart-reconnects',status==200 and json.loads(raw)['id']=='native-1')
   os.chmod(home/'data',0o755)
   status,_,_=request('POST',body=body,headers={'Content-Type':'application/json'})
   check('private-path-rechecked-per-request',status==502)
   os.chmod(home/'data',0o700)
   check('ipc-fixture-no-errors',engine.failure is None)
  finally:
   if process is not None:process.terminate();process.wait(timeout=3)
   engine.close()
 check('temporary-fixture-removed',not home.exists())
 check('binary-unchanged',hashlib.sha256(binary.read_bytes()).hexdigest()==binary_hash)
 a.output.write_text(json.dumps({'status':'PASS','scope':'actual-adapter-with-synthetic-IPC-engine','check_count':len(checks),'checks':checks,'binary_sha256':binary_hash,'launch_status':'NO_GO','production_qualified':False,'actual_consensus_tested':False,'private_material_used':False,'cleanup_complete':True},indent=2)+'\n')
 print(f'{len(checks)} loopback compatibility checks passed')
if __name__=='__main__':main()
