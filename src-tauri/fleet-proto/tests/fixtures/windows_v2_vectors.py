# SPDX-License-Identifier: AGPL-3.0-or-later
# Offline golden generator; test seeds only. Requires Python cryptography.
import json,hashlib,base64
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding,PublicFormat
c=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
b64=lambda x:base64.b64encode(x).decode()
key=lambda n:Ed25519PrivateKey.from_private_bytes(bytes([n])*32)
pub=lambda n:b64(key(n).public_key().public_bytes(Encoding.Raw,PublicFormat.Raw))
binding={'org_id':'org-alpha','device_id':'Desk/Łaptop?1','enrollment_generation':'11111111-1111-4111-8111-111111111111','authority_key_b64':pub(11)}
command={'version':2,'binding':binding,'command_id':'delivery-1','idempotency_key':'request-1','catalog_id':'fixture.inspect','action_class':'safe','payload':'{"z":false,"a":["é",null,7],"big":12345678901234567890123456789012345678901234567890,"exponent":1e400}','epoch_version':'4','signature':'','signer_key':pub(13)}
command['payload']=command['payload'][:-1]+',"escape":'+c('line\nslash\\🔒').decode()+'}'
cp=c(['fleet.windows.command.v2',binding,command['idempotency_key'],command['catalog_id'],command['action_class'],command['payload'],'4']);command['signature']=b64(key(13).sign(cp))
outcome={'version':2,'binding':binding,'receipt_id':'receipt-1','sequence':'1','previous_hash':None,'command_id':'delivery-1','catalog_id':'fixture.inspect','action_class':'safe','outcome':'unknown','failure_stage':'report','observed_at':'2026-01-01T00:00:00.123456789Z','summary':{'kind':'command','result_sha256':hashlib.sha256(c(['fleet.windows.result.v2',c({'error':'fixture'}).decode()])).hexdigest()},'record_hash':'','signature':''}
op=c(['fleet.windows.action-outcome.v2',2,binding,'receipt-1','1',None,'delivery-1','fixture.inspect','safe','unknown','report',outcome['observed_at'],outcome['summary']]);oh=hashlib.sha256(op).digest();outcome['record_hash']=oh.hex();outcome['signature']=b64(key(7).sign(oh))
ack={'binding':binding,'command_id':'delivery-1','report_id':'report-1','success':False,'detail':'fixture','result':c({'error':'fixture'}).decode(),'phase':None,'terminal_reason':None}
request=c({'nonce':'nonce-1','windows_v2':{'version':2,'binding':binding,'acks':[ack],'outcomes':[outcome]}})
response=c({'windows_v2':{'version':2,'binding':binding,'commands':[command],'ack_receipts':[{'command_id':'delivery-1','report_id':'report-1','report_sha256':hashlib.sha256(c(['fleet.windows.ack.v2',ack])).hexdigest(),'disposition':'accepted','resulting_status':'failed'}],'outcome_receipts':[{'receipt_id':'receipt-1','sequence':'1','record_hash':oh.hex(),'disposition':'accepted'}],'command_epoch_floor':'4','head':{'last_sequence':'1','previous_hash':oh.hex()}}})
proof={'version':2,'binding':binding,'nonce':'nonce-1','request_sha256':b64(hashlib.sha256(request).digest()),'http_status':200,'response_sha256':b64(hashlib.sha256(response).digest()),'signature':''}
pp=c(['fleet.windows.outcome-receipt.v2',binding,'nonce-1',proof['request_sha256'],200,proof['response_sha256']]);proof['signature']=b64(key(11).sign(pp))
vector={'description':'Public test seeds: device 7, authority 11, operator 13, each repeated 32 bytes. Independently generated with Python cryptography Ed25519 and hashlib SHA256.','device_key':pub(7),'command':command,'command_preimage':cp.decode(),'outcome':outcome,'outcome_preimage':op.decode(),'ack':ack,'result_hash':hashlib.sha256(c(['fleet.windows.result.v2',ack['result']])).hexdigest(),'ack_hash':hashlib.sha256(c(['fleet.windows.ack.v2',ack])).hexdigest(),'request_bytes':request.decode(),'response_bytes':response.decode(),'proof':proof,'proof_preimage':pp.decode()}
readiness=dict(outcome,receipt_id='readiness-1',command_id=None,catalog_id='readiness.self_test',summary={'kind':'readiness','result':{'arm_predicate':'unknown','signed_dispatch':'pass','simulate_branch':'pass','recovery_path':'fail','observed_at':'2025-12-31T23:00:00-01:00'}})
rp=c(['fleet.windows.action-outcome.v2',2,binding,readiness['receipt_id'],'1',None,None,readiness['catalog_id'],'safe','unknown','report',readiness['observed_at'],readiness['summary']]);rh=hashlib.sha256(rp).digest();readiness['record_hash']=rh.hex();readiness['signature']=b64(key(7).sign(rh))
vector['readiness']=readiness;vector['readiness_preimage']=rp.decode()
epoch_vectors=[]
for value in [-9223372036854775808, 9223372036854775807]:
    cmd=dict(command,epoch_version=str(value))
    command_preimage=c(['fleet.windows.command.v2',binding,cmd['idempotency_key'],cmd['catalog_id'],cmd['action_class'],cmd['payload'],cmd['epoch_version']])
    cmd['signature']=b64(key(13).sign(command_preimage))
    reply=json.loads(response);reply['windows_v2']['command_epoch_floor']=str(value);reply['windows_v2']['commands']=[cmd]
    reply_bytes=c(reply)
    receipt=dict(proof,response_sha256=b64(hashlib.sha256(reply_bytes).digest()))
    receipt_preimage=c(['fleet.windows.outcome-receipt.v2',binding,'nonce-1',receipt['request_sha256'],200,receipt['response_sha256']])
    receipt['signature']=b64(key(11).sign(receipt_preimage))
    epoch_vectors.append({'command':cmd,'command_preimage':command_preimage.decode(),'response_bytes':reply_bytes.decode(),'proof':receipt,'proof_preimage':receipt_preimage.decode()})
vector['epoch_boundaries']=epoch_vectors
from pathlib import Path
Path(__file__).with_name('windows_v2.json').write_text(json.dumps(vector,indent=2,ensure_ascii=False)+'\n')
