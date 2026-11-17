from pathlib import Path
import json, os, subprocess as sp
source=os.environ['SOURCE'];assert sp.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
out=Path('tmp/foreground-setup-evidence');out.mkdir(parents=True,exist_ok=True)
path=Path('pebrel_app/src/ai_hook/native_events/subagent_tests.rs');orig=path.read_text(encoding='utf8')
name='antigravity_confirmed_foreground_finishes_after_brain_exceeds_scan_limit';pre,target=orig.split('fn '+name+'()',1);assert 'confirmed-foreground-0001' in target
cmd=['cargo','nextest','run','--locked','--config','.github/ci-profile.toml','--cargo-profile','ci','-p','pebrel','--bin','pebrel','--features','gpui-test-support','--test-threads','1','--retries','0','-E','test('+name+')']
report={'source':source,'original':[]}
def run(tag,positive):
 p=out/(tag+'.log');
 with p.open('w',encoding='utf8') as log:rc=sp.run(cmd,stdout=log,stderr=sp.STDOUT).returncode
 t=p.read_text(errors='replace');summary=[l.strip() for l in t.splitlines() if 'Summary [' in l];assert summary and (rc==0 if positive else rc!=0), (tag,rc,t[-900:]);assert any(('7 passed' if positive else '1 failed') in x for x in summary);return summary
cmd[-1]='test(ai_hook::native_events::subagent_tests::)'
for n in range(3):report['original'].append(run('all-subagent-'+str(n+1),True))
cmd[-1]='test('+name+')'
production=Path('pebrel_app/src/ai_hook/native_events.rs');base=production.read_text(encoding='utf8')
try:
    needle='let confirmed_foreground = is_confirmed_antigravity_foreground(&id);'; timeout='else if is_confirmed_antigravity_foreground(&id) {';assert base.count(needle)==base.count(timeout)==1
    production.write_text(base.replace(needle,'let confirmed_foreground = false;').replace(timeout,'else if false {'),encoding='utf8')
    report['unconfirmed_regression']=run('remove-confirmed-foreground-treatment',False)
    text=(out/'remove-confirmed-foreground-treatment.log').read_text(errors='replace');assert 'confirmed foreground session must not drop its own completion at scan limit' in text
    report['expected_failure']='removing confirmed foreground treatment fails the final completion, after setup';(out/'result.json').write_text(json.dumps(report,indent=2))
finally:production.write_text(base,encoding='utf8')
