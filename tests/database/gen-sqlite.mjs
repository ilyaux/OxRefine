// Reproduce SQLite mapping through the actual bundled database extension, not hand-written cells.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { resolve, join } from 'node:path'
import { execFileSync } from 'node:child_process'
import { OR_HOME, startOpenRefine } from '../../tools/openrefine.mjs'
const artifacts={
 'webapp/WEB-INF/lib/openrefine-main.jar':'b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957',
 'webapp/extensions/database/module/MOD-INF/lib/openrefine-database.jar':'ff9b36b43e14d5fb842003866d042286298b621a9fc7a01444e1ae5997a80c67',
 'webapp/extensions/database/module/MOD-INF/lib/sqlite-jdbc-3.51.1.0.jar':'29eda0d1e5ab235b0f1baad05c531106c59c4a72c92a51a4b9c1a4e4c84abffe',
}
for(const[path,sha]of Object.entries(artifacts))if(createHash('sha256').update(readFileSync(join(OR_HOME,path))).digest('hex')!==sha)throw new Error('Reference main artifact mismatch')
const workspace=resolve('target/database-reference-344/sqlite-workspace');mkdirSync(workspace,{recursive:true})
const db=resolve('target/database-reference-344/fixture.sqlite');
execFileSync('python',['-c',"import sqlite3,sys,pathlib; p=pathlib.Path(sys.argv[1]); p.unlink(missing_ok=True); c=sqlite3.connect(p); c.executescript(pathlib.Path(sys.argv[2]).read_text(encoding='utf-8')); c.close()",db,resolve('tests/database/sqlite-fixture.sql')],{stdio:'pipe'})
// DriverManager discovers JDBC drivers on the application classpath. The stock dynamic module
// does not register SQLite's driver in this distribution; expose its unchanged, bundled JAR.
const jdbcClasspath=[join(OR_HOME,'server/classes'),join(OR_HOME,'server/target/lib/*'),join(OR_HOME,'webapp/extensions/database/module/MOD-INF/lib/sqlite-jdbc-3.51.1.0.jar')].join(';')
const or=await startOpenRefine({port:3338,mem:'512M',workspace,jvm:['-cp',jdbcClasspath,'-Duser.timezone=UTC']})
try{
 const csrf=(await or.get('/command/core/get-csrf-token')).token
 async function post(command,fields){const body=new URLSearchParams();for(const[k,v]of Object.entries(fields))body.set(k,typeof v==='string'?v:JSON.stringify(v));const response=await fetch(`${or.url}/command/${command}${command.includes('?')?'&':'?'}csrf_token=${csrf}`,{method:'POST',body});if(!response.ok)throw new Error(`Reference database command HTTP ${response.status}: ${(await response.text()).slice(0,1000)}`);return response.json()}
 const {jobID}=await post('core/create-importing-job',{})
 const ctl=sub=>`core/importing-controller?controller=database%2Fdatabase-import-controller&jobID=${jobID}&subCommand=${sub}`
 const info={connectionName:'synthetic SQLite',databaseType:'sqlite',databaseServer:'localhost',databasePort:'0',databaseUser:'na',databasePassword:'',initialDatabase:db.replaceAll('\\','/'),initialSchema:'',query:'SELECT * FROM typed_fixture ORDER BY id'}
 const tested=await post('database/test-query',info);if(tested.code!=='ok')throw new Error('Reference SQLite test query failed')
 const init=await post(ctl('initialize-parser-ui'),info)
 await post(ctl('create-project'),{...info,options:JSON.stringify({...init.options,projectName:'database SQLite reference'})})
 let config
 for(let i=0;i<300;i++){config=(await post(`core/get-importing-job-status?jobID=${jobID}`,{})).job?.config??{};if(config.projectID)break;if(config.state==='error')throw new Error('Reference SQLite import failed');await new Promise(r=>setTimeout(r,100))}
 if(!config.projectID)throw new Error('Reference SQLite import timed out')
 const columns=(await or.get(`/command/core/get-models?project=${config.projectID}`)).columnModel.columns
 if(columns.length!==13)throw new Error(`Reference SQLite import returned ${columns.length} columns instead of 13`)
 const raw=await(await fetch(`${or.url}/command/core/get-rows?project=${config.projectID}&start=0&limit=100`)).text()
 const decoded=JSON.parse(raw,(_key,value,context)=>typeof value==='number'&&Number.isInteger(value)&&!Number.isSafeInteger(value)&&/^-?\d+$/.test(context?.source??'')?JSON.rawJSON(context.source):value)
 const rows=decoded.rows.map(r=>columns.map(c=>r.cells[c.cellIndex]??null))
 writeFileSync(new URL('reference-sqlite.json',import.meta.url),JSON.stringify({provenance:{release:'3.10.1',reportedVersion:await or.get('/command/core/get-version'),artifacts,generator:'tests/database/gen-sqlite.mjs',scope:'Bundled database import controller, synthetic SQLite fixture, default import options',bootstrap:'Unchanged bundled sqlite-jdbc JAR also on the application classpath for DriverManager discovery; stock dynamic-module setup returned HTTP400 and an empty project'},options:init.options,query:info.query,columns:columns.map(c=>c.name),rows},null,2)+'\n')
 await or.command('delete-project',{project:config.projectID})
 console.log('Actual OpenRefine SQLite reference:',columns.length,'columns,',rows.length,'rows')
}finally{or.stop()}
