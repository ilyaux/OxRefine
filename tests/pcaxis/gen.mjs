// Execute the bundled PC-Axis importer from the hash-pinned official 3.10.1 release.
import { createHash } from "node:crypto"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES } from "./cases.mjs"

const artifacts = {
  "webapp/WEB-INF/lib/openrefine-main.jar":"b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
  "webapp/extensions/pc-axis/module/MOD-INF/lib/openrefine-pcaxis.jar":"e899c40b601df090484636301e4ef7e63215e168b6323a1f02b888c2d2236682",
}
for (const [path,expected] of Object.entries(artifacts)) {
  const actual=createHash("sha256").update(readFileSync(join(OR_HOME,path))).digest("hex")
  if(actual!==expected) throw new Error(`Reference artifact mismatch: ${path}`)
}
const workspace=join(import.meta.dirname,"../../target/pcaxis-reference-346/workspace")
mkdirSync(workspace,{recursive:true})
const or=await startOpenRefine({port:3338,workspace,jvm:["-Duser.language=en","-Duser.country=US","-Duser.timezone=UTC","-Dfile.encoding=UTF-8"]})
const inputs=new URL("./inputs/",import.meta.url);mkdirSync(inputs,{recursive:true})
const cases=[]
try {
  const reportedVersion=await or.get("/command/core/get-version")
  for(const c of CASES) {
    const options={encoding:"UTF-8",skipDataLines:0,limit:-1,...c.options}
    const file=`${c.name}.px`
    const bytes=c.encoding==="windows-1252"?Buffer.from([...c.text].map(ch=>{
      if(ch==="€")return 0x80
      const code=ch.charCodeAt(0);if(code>255||code>=0x80&&code<0xa0)throw new Error(`Unsupported Windows-1252 fixture character ${ch}`)
      return code
    })):Buffer.from(c.text)
    writeFileSync(new URL(file,inputs),bytes)
    let response
    try{response=await or.command("create-project-from-upload",{"project-file":new File([bytes],file),"project-name":"pcaxis-reference",format:"text/line-based/pc-axis",options},{json:false})}
    catch(error){
      if(!c.malformed)throw error
      const status=Number(error.message.match(/HTTP (\d+)/)?.[1]);if(!status)throw error
      cases.push({name:c.name,file:`inputs/${file}`,options,malformed:true,error:{status}})
      console.log(c.name,`HTTP ${status}`);continue
    }
    const project=new URL(response.headers.get("location")??"",or.url).searchParams.get("project")
    if(!project) {
      cases.push({name:c.name,file:`inputs/${file}`,options,malformed:!!c.malformed,error:{status:response.status,body:(await response.text()).slice(0,500)}})
    }else{
      const models=await or.get(`/command/core/get-models?project=${project}`)
      if(!models.columnModel){
        if(!c.malformed)throw new Error(`${c.name}: ${JSON.stringify(models)}`)
        if(typeof models.message==="string")models.message=models.message.replaceAll(project,"<project>")
        cases.push({name:c.name,file:`inputs/${file}`,options,malformed:true,error:{models}})
      }else{
        const table=await or.table(project)
        cases.push({name:c.name,file:`inputs/${file}`,options,malformed:!!c.malformed,columns:table.columns,rows:table.rows.map(r=>r.cells)})
        await or.command("delete-project",{project})
      }
    }
    console.log(c.name,cases.at(-1).rows?`${cases.at(-1).rows.length} rows`:"error")
  }
  const wizard=await or.wizard({upload:new File([Buffer.from(CASES[0].text)],"table.px")})
  writeFileSync(new URL("fixtures.json",import.meta.url),JSON.stringify({provenance:{release:"3.10.1",releaseUrl:"https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip",releaseSha256:"6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8",artifacts,reportedVersion,generator:"tests/pcaxis/gen.mjs",normalization:"Generated failed-project ids replaced by <project> in error messages"},wizard,cases},null,2)+"\n")
}finally{or.stop()}
