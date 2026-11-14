// Official OpenRefine byte/cell references, including its ignored dateSettings controls.
import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { serve, MANIFEST } from "../recon/service.mjs"
const { release } = JSON.parse(readFileSync(new URL("encoding-fixtures.json",import.meta.url)))
for (const [path,hash] of Object.entries(release.artifacts)) if(createHash("sha256").update(readFileSync(join(OR_HOME,path))).digest("hex")!==hash) throw new Error(`Wrong reference artifact ${path}`)
const corpus='name,when,n,alive,url\nDouglas Adams,2024-01-02T03:04:05.123Z,42,true,https://example.org/a?x=1&y=2\nDouglas Adams,2023-12-31T23:59:00Z,7,false,plain\nNobody Known,,0,true,"<tag> & quote"\n'
const all={mode:"row-based",facets:[]}
const typed=[...["when","n","alive"].map((columnName)=>({op:"core/text-transform",columnName,engineConfig:all,expression:"grel:"+({when:"value.toDate()",n:"value.toNumber()",alive:"value == 'true'"}[columnName]),onError:"keep-original",repeat:false,repeatCount:10}))]
const service=await serve()
const or=await startOpenRefine({port:3340,workspace:fileURLToPath(new URL("../../target/export-custom-reference/",import.meta.url)),jvm:["-Duser.language=en","-Duser.country=US","-Duser.timezone=UTC","-Dfile.encoding=UTF-8"]})
try {
  const project=await or.createProject(corpus,{name:"custom-export"})
  let result=await or.runOps(project,typed); if(result.code!=="ok") throw new Error(JSON.stringify(result))
  await or.command("set-preference",{name:"reconciliation.standardServices",value:JSON.stringify([{url:service.base+"/svc",...MANIFEST(service.base)}])})
  result=await or.runOps(project,[{op:"core/recon",columnName:"name",engineConfig:all,config:{mode:"standard-service",service:service.base+"/svc",identifierSpace:"http://ox.test/entity/",schemaSpace:"http://ox.test/prop/",type:null,autoMatch:true,columnDetails:[],limit:0}},
    {op:"ox/recon-judge-one-cell",columnName:"name",row:1,judgment:"matched",match:{id:"Q3",name:"Ada Lovelace",types:["Q5"],score:100}}])
  if(result.code!=="ok") throw new Error(JSON.stringify(result))
  const table=await or.table(project); const raw=await or.get(`/command/core/get-rows?project=${project}&start=0&limit=100`)
  for(const row of table.rows) for(const cell of row.cells) if(cell?.r) cell.r=raw.pool.recons[cell.r]
  const services=[{url:service.base+"/svc",...MANIFEST(service.base)}]
  const columns=[{name:"url"},{name:"name",reconSettings:{output:"entity-id",blankUnmatchedCells:false,linkToEntityPages:true}},{name:"when"},{name:"n"},{name:"alive"}]
  const cases=[]
  for(const format of ["csv","tsv","*sv","html","xlsx","xls","ods"]) for(const headers of [true,false]) cases.push({name:`ordered ${format} headers ${headers}`,format,...(format==="ods"?{differenceKind:"ods-time",knownDifference:"OpenRefine 3.10.1 drops the time from ODS date cells; OxRefine preserves their timestamps."}:{}),params:{options:{columns,outputColumnHeaders:headers,outputBlankRows:true}}})
  for(const output of ["entity-name","entity-id","cell-content"]) cases.push({name:`recon ${output}`,format:"*sv",params:{options:{columns:[{name:"name",reconSettings:{output,blankUnmatchedCells:true,linkToEntityPages:true}}],separator:";",lineSeparator:"\n",outputBlankRows:true}}})
  cases.push({name:"one empty column quote-all",format:"*sv",params:{options:{columns:[{name:"when"}],quoteAll:true,lineSeparator:"\n",outputBlankRows:true}}})
  for(const format of ["*sv","html","xlsx"]) cases.push({name:`date controls ignored by OR ${format}`,format,differenceKind:"date-controls",knownDifference:"OpenRefine 3.10.1 builds dateFormatter but never applies it; OxRefine applies requested formatting (as text in workbooks).",params:{options:{columns:[{name:"when",dateSettings:{format:"custom",custom:"yyyy/MM/dd HH:mm:ss",omitTime:false,useLocalTimeZone:false}}],separator:";",lineSeparator:"\n",outputBlankRows:true}}})
  const out=[]
  for(const c of cases) {
    const response=await or.command("export-rows",{project,format:c.format,engine:all,options:JSON.stringify(c.params.options)},{json:false})
    const bytes=Buffer.from(await response.arrayBuffer())
    out.push({...c,params:{...c.params,reconciliationServices:services},...( ["xlsx","xls","ods"].includes(c.format) ? {outputBase64:bytes.toString("base64")} : {outputText:bytes.toString("utf8")} )})
    console.log(`${c.name}: ${bytes.length} bytes`)
  }
  writeFileSync(new URL("custom-fixtures.json",import.meta.url),JSON.stringify({openrefine:"3.10.1",reportedVersion:await or.get("/command/core/get-version"),release,corpus,typed,table,cases:out},null,1).replaceAll(service.base,"https://recon.example.test")+"\n")
} finally {or.stop();service.close()}
