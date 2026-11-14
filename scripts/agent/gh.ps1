$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force

$identity = Get-AgentIdentity
Remove-Item Env:GITHUB_TOKEN, Env:GH_ENTERPRISE_TOKEN, Env:GITHUB_ENTERPRISE_TOKEN -ErrorAction SilentlyContinue
$env:GH_TOKEN = (New-AgentToken).token
$env:GH_REPO = "$($identity.Owner)/$($identity.Repo)"
$env:GH_PROMPT_DISABLED = '1'

& gh @args
exit $LASTEXITCODE
