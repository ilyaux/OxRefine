# Wall time + peak RAM of a headless recipe run. Usage: .\bench.ps1 [data\people.csv]
param([string]$Csv = "data\people.csv")
$ox = Join-Path $PSScriptRoot "..\target\release\ox.exe"
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process $ox -ArgumentList "apply", "recipe.json", $Csv, "-o", "data\out.csv" -NoNewWindow -PassThru -WorkingDirectory $PSScriptRoot
$peak = 0
while (-not $p.HasExited) { $p.Refresh(); $peak = [Math]::Max($peak, $p.PeakWorkingSet64); Start-Sleep -Milliseconds 20 }
"{0} -> {1:N0} ms wall, {2:N0} MB peak RAM (file {3:N0} MB)" -f $Csv, $sw.ElapsedMilliseconds, ($peak / 1MB), ((Get-Item (Join-Path $PSScriptRoot $Csv)).Length / 1MB)
