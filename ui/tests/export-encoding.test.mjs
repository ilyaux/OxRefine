import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function dialog(format = "csv", initialPreferences = {}, columns = ["name", "city"]) {
  const hooks = [], requests = [], saved = [], exports = {}, preferenceWrites = []
  const preferences = plain(initialPreferences)
  let cursor = 0, dirty = true, effects = [], view, debounced, requested
  let engine = { mode: "row-based", facets: [] }
  const jsx = (type, props, key) => ({ type, props, key })
  const react = {
    useState(initial) {
      const i = cursor++
      hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }
      return [hooks[i].value, (next) => { hooks[i].value = typeof next === "function" ? next(hooks[i].value) : next; dirty = true }]
    },
    useEffect(effect, deps) {
      const i = cursor++, old = hooks[i]
      if (!old || deps.some((value, index) => !Object.is(value, old.deps[index]))) {
        hooks[i] = { deps, cleanup: old?.cleanup }
        effects.push(() => { hooks[i].cleanup?.(); hooks[i].cleanup = effect() })
      }
    },
  }
  const code = ts.transpileModule(readFileSync(new URL("../src/ExportDialog.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "lucide-react") return { ChevronUpIcon:"chevron-up",ChevronDownIcon:"chevron-down" }
      if (name === "@/Dialogs") return { useDebounced(value) { requested = value; return debounced ??= value } }
      if (name === "@/lib/utils") return { cn: (...parts) => parts.join(" ") }
      if (name === "@/components/ui/input") return { Input: "input" }
      if (name === "@/components/ui/textarea") return { Textarea: "textarea" }
      if (name === "@/components/instrument") return { FullView: "full-view", Metric: "metric", Spacer: "spacer", StripAction: "strip-action", SummaryStrip: "summary-strip" }
      if (name === "@/api") return {
        call(command, args, options) {
          if(command==="preferences") { assert.equal(options.quiet,true);return Promise.resolve({data:preferences}) }
          if(command==="set_preference") { assert.equal(options.quiet,true);assert.equal(args.name,"oxrefine.export.presets");preferences[args.name]=plain(args.value);preferenceWrites.push(plain(args));return Promise.resolve({data:preferences}) }
          assert.equal(command, "export")
          return new Promise((resolve, reject) => requests.push({ args, reply: (data) => resolve({ data }), fail: (message) => reject(new Error(message)) }))
        },
      }
      throw new Error(`Unexpected ExportDialog dependency ${name}`)
    },
  })
  function render() {
    let turns = 0
    while (dirty) {
      assert.ok(++turns < 20); dirty = false; cursor = 0
      view = exports.ExportDialog({ format, name: "people.csv", columns, engine, rows: 2, allRows: 8,
        onExport: (format, ext, params) => saved.push(JSON.parse(JSON.stringify({ format, ext, params }))), onClose() {} })
      const queued = effects; effects = []; queued.forEach((effect) => effect())
    }
  }
  render()
  return {
    requests, saved, preferences, preferenceWrites,
    get view() { return view },
    get save() { return nodes(view).find((node) => node.type === "strip-action") },
    change(node, value) { node.props.onChange({ target: { value, checked: value } }); render() },
    click(node) { node.props.onClick(); render() },
    chooseEncoding(value) { this.change(nodes(view).find((node) => node.props?.["aria-label"] === "Character encoding"), value) },
    check(label, value) { const parent = nodes(view).find((node) => node.type === "label" && text(node).trim() === label); assert.ok(parent,label); this.change(nodes(parent).find((node) => node.type === "input"), value) },
    segment(label, value) { const control = nodes(view).find((node) => typeof node.type === "function" && node.props?.label === label); assert.ok(control,label); control.props.onChange(value); render() },
    advanceDebounce() { debounced = requested; dirty = true; render() },
    setEngine(value) { engine = value; dirty = true; render() },
    async flush() { for (let i=0;i<4;i++) await Promise.resolve(); render() },
    unmount() { hooks.forEach((hook) => hook.cleanup?.()) },
  }
}
function nodes(node) {
  if (Array.isArray(node)) return node.flatMap(nodes)
  if (node && typeof node === "object") return [node,...nodes(node.props?.children)]
  return []
}
function text(node) {
  if (Array.isArray(node)) return node.map(text).join("")
  if (node && typeof node === "object") return text(node.props?.children)
  return node == null || typeof node === "boolean" ? "" : String(node)
}
const plain = (value) => JSON.parse(JSON.stringify(value))
const column = (name) => ({name,dateSettings:{format:"iso-8601",custom:"yyyy-MM-dd",omitTime:false,useLocalTimeZone:false},reconSettings:{output:"entity-name",blankUnmatchedCells:false,linkToEntityPages:true}})
const options = (names=["name","city"],blank=false) => ({outputColumnHeaders:true,outputBlankRows:blank,columns:names.map(column)})

test("CSV charset and BOM share preview/save parameters; pending and stale previews cannot approve Save", async () => {
  const d = dialog(); assert.equal(d.save.props.disabled,true)
  assert.deepEqual(plain(d.requests[0].args.params),{ encoding:"UTF-8",bom:false,encodingErrors:"error",options:options() })
  d.requests[0].reply("name,city\r\nAlice,London\r\n"); await d.flush(); assert.equal(d.save.props.disabled,false)
  d.chooseEncoding("UTF-16LE"); assert.equal(d.save.props.disabled,true,"disable during debounce too")
  d.click(d.save); assert.equal(d.saved.length,0)
  d.advanceDebounce(); assert.equal(d.requests[1].args.params.encoding,"UTF-16LE")
  d.check("Include byte-order mark (BOM)",true); d.advanceDebounce()
  d.requests[1].reply("old preview"); await d.flush(); assert.equal(d.save.props.disabled,true)
  d.requests[2].reply("name,city\r\nИлья,Москва\r\n"); await d.flush(); assert.equal(d.save.props.disabled,false)
  assert.match(text(d.view),/Илья,Москва/); assert.match(text(d.view),/decoded text, BOM hidden/)
  d.click(d.save); assert.deepEqual(d.saved[0],{format:"csv",ext:"csv",params:{encoding:"UTF-16LE",bom:true,encodingErrors:"error",options:options()}})
  d.setEngine({mode:"record-based",facets:[]}); assert.equal(d.save.props.disabled,true)
  d.requests.at(-1).reply("new engine preview"); await d.flush(); assert.equal(d.save.props.disabled,false)
  d.unmount()
})

test("legacy charset failures stay explicit and replacement is opt-in", async () => {
  const d=dialog("tsv"); d.chooseEncoding("US-ASCII"); d.advanceDebounce()
  assert.equal(d.requests.at(-1).args.params.encodingErrors,"error")
  d.requests.at(-1).fail("Character U+0418 cannot be encoded as US-ASCII. Choose UTF-8."); await d.flush()
  assert.equal(d.save.props.disabled,true); assert.match(text(d.view),/Character U\+0418/)
  d.click(d.save); assert.equal(d.saved.length,0)
  d.check("Replace unsupported characters with ?",true); assert.equal(d.save.props.disabled,true); d.advanceDebounce()
  assert.equal(d.requests.at(-1).args.params.encodingErrors,"replace")
  assert.equal(d.requests.at(-1).args.params.bom,false)
  d.requests.at(-1).reply("name\tcity\r\n????\t??????\r\n"); await d.flush(); assert.equal(d.save.props.disabled,false)
  assert.match(text(d.view),/\?\?\?\?/); d.click(d.save); assert.equal(d.saved[0].params.encodingErrors,"replace")
  d.click(nodes(d.view).find((node) => node.type === "button" && text(node)==="Excel.xlsx"))
  assert.equal(nodes(d.view).find((node) => node.props?.["aria-label"]==="Character encoding"),undefined)
  d.click(d.save); assert.deepEqual(d.saved[1],{format:"xlsx",ext:"xlsx",params:{options:options(["name","city"],true)}})
  d.unmount()
})

test("custom separator options, chosen columns and encoded preview survive charset changes", async () => {
  const d=dialog("*sv")
  d.chooseEncoding("UTF-8"); d.check("Include byte-order mark (BOM)",true)
  d.segment("Separator",";"); d.segment("Line endings","\r\n")
  d.check("Column headers",false); d.check("Quote every cell",true); d.check("Keep blank rows",false); d.check("city",false)
  d.chooseEncoding("windows-1251"); d.advanceDebounce()
  const params=plain(d.requests.at(-1).args.params)
  assert.deepEqual(params,{encoding:"windows-1251",bom:false,encodingErrors:"error",options:{separator:";",lineSeparator:"\r\n",outputColumnHeaders:false,quoteAll:true,outputBlankRows:false,columns:[column("name")]}})
  d.requests.at(-1).reply('"Илья"\r\n'); await d.flush(); d.click(d.save)
  assert.deepEqual(d.saved[0].params,params)
  d.unmount()
})

test("ordered per-column settings persist without data and apply safely to a different project",async()=>{
  const d=dialog();await d.flush()
  d.click(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Move city up"))
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Date format"),"custom")
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Custom date pattern"),"yyyy/MM/dd")
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Column options"),"city")
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Reconciled cell output"),"entity-id")
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Export preset name"),"Reusable")
  d.click(nodes(d.view).find((n)=>n.type==="button"&&text(n)==="Save preset"));await d.flush()
  assert.equal(d.preferenceWrites.length,1)
  const saved=d.preferenceWrites[0].value[0]
  assert.deepEqual(saved.columns.map((c)=>c.name),["city","name"])
  assert.equal(saved.columns[0].reconSettings.output,"entity-id");assert.equal(saved.columns[1].dateSettings.custom,"yyyy/MM/dd")
  assert.deepEqual(Object.keys(saved).sort(),["bom","columns","encoding","format","name","replace","sv"].sort(),"presets exclude rows, engines, file paths and service data")
  d.unmount()
  const next=dialog("csv",d.preferences,["name"]);await next.flush()
  next.change(nodes(next.view).find((n)=>n.props?.["aria-label"]==="Load export preset"),"Reusable")
  assert.match(text(next.view),/1 preset columns are missing/)
  next.advanceDebounce()
  assert.deepEqual(plain(next.requests.at(-1).args.params.options.columns),[saved.columns[1]])
  next.requests.at(-1).reply("formatted sample");await next.flush();next.click(next.save)
  assert.equal(next.saved.length,1);next.unmount()
})

test("invalid stored presets are not silently replaced; workbook controls carry custom settings",async()=>{
  const d=dialog("xlsx",{"oxrefine.export.presets":[{name:"Broken"}]});await d.flush()
  assert.match(text(d.view),/Saved export presets are invalid/)
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Export preset name"),"New")
  const save=nodes(d.view).find((n)=>n.type==="button"&&text(n)==="Save preset")
  assert.equal(save.props.disabled,true);d.click(save);assert.equal(d.preferenceWrites.length,0)
  d.change(nodes(d.view).find((n)=>n.props?.["aria-label"]==="Date format"),"custom")
  assert.match(text(d.view),/export dates as text/)
  assert.doesNotMatch(text(d.view),/Link matched entities in HTML/)
  d.click(d.save);assert.equal(d.saved[0].params.options.columns[0].dateSettings.format,"custom");d.unmount()
})

test("format-specific blank-row defaults and explicit choices survive format switches",()=>{
  const d=dialog("csv")
  assert.equal(d.requests[0].args.params.options.outputBlankRows,false)
  const choose=(label)=>d.click(nodes(d.view).find((n)=>n.type==="button"&&text(n)===label))
  choose("Custom tabular.csv");d.advanceDebounce();assert.equal(d.requests.at(-1).args.params.options.outputBlankRows,true)
  d.check("Keep blank rows",false);d.advanceDebounce()
  choose("TSV.tsv");d.advanceDebounce();assert.equal(d.requests.at(-1).args.params.options.outputBlankRows,false)
  d.check("Keep blank rows",true);d.advanceDebounce()
  choose("Custom tabular.csv");d.advanceDebounce();assert.equal(d.requests.at(-1).args.params.options.outputBlankRows,false)
  choose("TSV.tsv");d.advanceDebounce();assert.equal(d.requests.at(-1).args.params.options.outputBlankRows,true)
  d.unmount()
})
