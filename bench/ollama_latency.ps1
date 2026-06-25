# Zero-build Ollama latency probe — hits the same /api/chat the app uses and reports
# TTFT, generation tok/s, prompt-eval tok/s for a few representative prompts. Use this
# for a quick latency read without compiling; use `xconsole-bench` for the full agent eval.
#
#   pwsh bench/ollama_latency.ps1 -Model qwen3.5:9b
#   pwsh bench/ollama_latency.ps1 -Model qwen3.5:9b -NumCtx 8192 -Out bench/results/llm.json
param(
  [string]$Model = "qwen3.5:9b",
  [string]$Base  = "http://localhost:11434",
  [int]$NumCtx   = 65536,
  [string]$Out   = ""
)
$ErrorActionPreference = "Stop"

function Invoke-Turn($system, $user, $think, $np = 96) {
  $msgs = @()
  if ($system) { $msgs += @{ role = "system"; content = $system } }
  $msgs += @{ role = "user"; content = $user }
  $body = @{
    model = $Model; messages = $msgs; stream = $false; think = $think
    keep_alive = "30m"; options = @{ num_ctx = $NumCtx; num_predict = $np }
  } | ConvertTo-Json -Depth 8
  $r = Invoke-RestMethod -Uri "$Base/api/chat" -Method Post -Body $body -ContentType "application/json" -TimeoutSec 300
  $ns = 1e9
  [pscustomobject]@{
    ttft_s = [math]::Round(($r.load_duration + $r.prompt_eval_duration) / $ns, 2)
    gen_s  = [math]::Round($r.eval_duration / $ns, 2)
    genTPS = [math]::Round($(if ($r.eval_duration) { $r.eval_count / ($r.eval_duration / $ns) } else { 0 }), 1)
    peTPS  = [math]::Round($(if ($r.prompt_eval_duration) { $r.prompt_eval_count / ($r.prompt_eval_duration / $ns) } else { 0 }), 0)
    ptok   = $r.prompt_eval_count
    gtok   = $r.eval_count
    thinkC = $(if ($r.message.thinking) { $r.message.thinking.Length } else { 0 })
  }
}

$short = "You are a concise voice assistant. Answer in one or two short sentences."
$big   = ("# Tool: ssh_exec`nRun a command on a VPS.`n# Skill: deploy`nSteps.`n" * 60) + "`n# Persistent memory`n" + ("- The user prefers concise answers.`n" * 40)
$q     = "In one short sentence, what is a reverse proxy?"

Write-Output "Warming up $Model ..."
Invoke-Turn $short "hi" $false 8 | Out-Null

$cases = @(
  @{ name = "short  think=OFF"; sys = $short; think = $false },
  @{ name = "short  think=ON "; sys = $short; think = $true  },
  @{ name = "big-sys think=OFF"; sys = $big;  think = $false }
)
"{0,-18} {1,7} {2,7} {3,7} {4,7} {5,6} {6,5} {7,7}" -f "case", "TTFT", "gen_s", "genTPS", "peTPS", "ptok", "gtok", "thinkC"
$rows = @()
foreach ($c in $cases) {
  $r = Invoke-Turn $c.sys $q $c.think
  "{0,-18} {1,6}s {2,6}s {3,7} {4,7} {5,6} {6,5} {7,7}" -f $c.name, $r.ttft_s, $r.gen_s, $r.genTPS, $r.peTPS, $r.ptok, $r.gtok, $r.thinkC
  $rows += @{ case = $c.name; ttft_s = $r.ttft_s; gen_s = $r.gen_s; genTPS = $r.genTPS; peTPS = $r.peTPS; ptok = $r.ptok; gtok = $r.gtok; thinkC = $r.thinkC }
}

if ($Out) {
  $dir = Split-Path -Parent $Out
  if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
  @{ model = $Model; num_ctx = $NumCtx; cases = $rows } | ConvertTo-Json -Depth 6 | Out-File -Encoding utf8 $Out
  Write-Output "Wrote $Out"
}
