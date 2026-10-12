<#
 Serves every model folder under -ModelsRoot (each with model.gguf + mmproj.gguf) from ONE llama-server
 in router mode on -Port. Only -MaxLoaded models sit in memory at once; a request for another model
 swaps it in. In viSHion: provider "Local server", address http://localhost:8188, then Find models.
   powershell -File tools\serve-models.ps1
#>
param(
  [string]$ModelsRoot = 'C:\Users\shuff\Developer\ocr-bakeoff\models',
  [string]$LlamaServer = 'C:\Users\shuff\Developer\ocr-bakeoff\bin\llama\llama-server.exe',
  [int]$Port = 8188,
  [int]$MaxLoaded = 1,
  [int]$Ctx = 8192
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $LlamaServer)) { throw "llama-server not found: $LlamaServer" }
$ini = New-Object System.Collections.Generic.List[string]
$ini.Add('version = 1'); $ini.Add('')
foreach ($d in Get-ChildItem $ModelsRoot -Directory | Sort-Object Name) {
  $m = Join-Path $d.FullName 'model.gguf'; $p = Join-Path $d.FullName 'mmproj.gguf'
  if (-not ((Test-Path $m) -and (Test-Path $p))) { Write-Warning "skipping $($d.Name): needs model.gguf and mmproj.gguf"; continue }
  $ini.Add("[$($d.Name)]")
  $ini.Add('model = ' + $m.Replace('\', '/'))
  $ini.Add('mmproj = ' + $p.Replace('\', '/'))
  $ini.Add("ctx-size = $Ctx"); $ini.Add('n-gpu-layers = 99'); $ini.Add('temp = 0'); $ini.Add('parallel = 1'); $ini.Add('')
}
$file = Join-Path $env:TEMP 'viSHion-models.ini'
[IO.File]::WriteAllLines($file, $ini)
Write-Host "Serving $((($ini | Where-Object { $_ -match '^\[' }).Count)) models on http://localhost:$Port (max $MaxLoaded loaded)"
& $LlamaServer --models-preset $file --models-max $MaxLoaded --host 127.0.0.1 --port $Port --no-webui
