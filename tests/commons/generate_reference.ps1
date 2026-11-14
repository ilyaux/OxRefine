param(
    [string]$OpenRefineDir = (Join-Path $PSScriptRoot '..\..\..\OpenRefine\openrefine-3.10.1'),
    [string]$JavaBin = ''
)
$ErrorActionPreference = 'Stop'

# This is a separate CommonsExtension mapping reference, not a bundled importer.
$commonsCommit = '4a2c838d54e462dc51d60d21efbac66b0a0fe07a'
$commonsExpectedMainHash = 'B1BEE728E85B4AA44967427707ECDB501A6B4AA3D87608BA7B29115C858F0957'
$commonsSources = [ordered]@{
    'FileRecord.java' = 'B67C211B65454F7889F897E4D02D36A15D212BA5B8680AB9B34934741A8265E9'
    'FileRecordToRows.java' = '69646BBC85B5EF2B7A42A6F1D90295CB8AFBA4E41C8F61F508A65C75573BB8E5'
}
$commonsRepo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$commonsTarget = Join-Path $commonsRepo 'target\commons-reference-355'
$commonsClasses = Join-Path $commonsTarget 'classes'
$commonsOutput = Join-Path $commonsTarget 'reference.json'
$commonsReference = (Resolve-Path -LiteralPath $OpenRefineDir).Path
$commonsMainJar = Join-Path $commonsReference 'webapp\WEB-INF\lib\openrefine-main.jar'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $commonsMainJar).Hash -ne $commonsExpectedMainHash) {
    throw 'OpenRefine main jar differs from the verified official 3.10.1 reference.'
}
New-Item -ItemType Directory -Force -Path $commonsClasses | Out-Null
foreach ($commonsSource in $commonsSources.Keys) {
    $commonsSourcePath = Join-Path $commonsTarget $commonsSource
    if (!(Test-Path -LiteralPath $commonsSourcePath)) {
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/OpenRefine/CommonsExtension/$commonsCommit/src/main/java/org/openrefine/extensions/commons/importer/$commonsSource" -OutFile $commonsSourcePath
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $commonsSourcePath).Hash -ne $commonsSources[$commonsSource]) {
        throw "Pinned source checksum differs: $commonsSource"
    }
}
$commonsClasspath = "$commonsReference\webapp\WEB-INF\lib\*;$commonsReference\server\target\lib\*"
$commonsJavac = if ($JavaBin) { Join-Path $JavaBin 'javac.exe' } else { (Get-Command javac -ErrorAction Stop).Source }
$commonsJava = if ($JavaBin) { Join-Path $JavaBin 'java.exe' } else { (Get-Command java -ErrorAction Stop).Source }
$commonsCompileInputs = @($commonsSources.Keys | ForEach-Object { Join-Path $commonsTarget $_ })
$commonsCompileInputs += Join-Path $PSScriptRoot 'CommonsMappingReference.java'
& $commonsJavac -encoding UTF-8 -classpath $commonsClasspath -d $commonsClasses @commonsCompileInputs
if ($LASTEXITCODE -ne 0) { throw 'Compiling the standalone reference harness failed.' }
& $commonsJava -classpath "$commonsClasses;$commonsClasspath" org.openrefine.extensions.commons.importer.CommonsMappingReference $commonsOutput
if ($LASTEXITCODE -ne 0) { throw 'Running the standalone reference harness failed.' }
$commonsResult = Get-Content -Raw -Encoding UTF8 -LiteralPath $commonsOutput | ConvertFrom-Json
$commonsProvenance = [ordered]@{
    reference = 'CommonsExtension v0.1.5 mapping executed against OpenRefine 3.10.1 libraries'
    extensionCommit = $commonsCommit
    extensionSourceBase = "https://github.com/OpenRefine/CommonsExtension/blob/$commonsCommit/src/main/java/org/openrefine/extensions/commons/importer/"
    extensionSourceSha256 = $commonsSources
    openRefineMainJarSha256 = $commonsExpectedMainHash
    generator = 'tests/commons/generate_reference.ps1'
    scope = 'FileRecordToRows only; synthetic records; no HTTP requests; excludes importer column setup, fetching and Ox wikitext addition'
    normalization = 'Removed generated recon.id only'
}
[ordered]@{ provenance = $commonsProvenance; cases = $commonsResult.cases } |
    ConvertTo-Json -Depth 100 | Set-Content -Encoding UTF8 -LiteralPath $commonsOutput
Write-Output "Generated $($commonsResult.cases.Count) reference cases at $commonsOutput"
