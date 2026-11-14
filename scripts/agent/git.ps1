$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force

$identity = Get-AgentIdentity
$helper = '!pwsh -NoProfile -File "{0}"' -f ((Join-Path $PSScriptRoot 'git-credential.ps1') -replace '\\', '/')
$env:GIT_TERMINAL_PROMPT = '0'
$env:GCM_INTERACTIVE = 'never'
Remove-Item Env:GIT_SSH, Env:GIT_SSH_COMMAND, Env:GIT_ASKPASS, Env:SSH_ASKPASS -ErrorAction SilentlyContinue

$config = @(
    '-c', "user.name=$($identity.Name)"
    '-c', "user.email=$($identity.Email)"
    '-c', 'credential.helper='
    '-c', "credential.helper=$helper"
    '-c', 'url.https://github.com/.insteadOf=git@github.com:'
    '-c', 'url.https://github.com/.insteadOf=ssh://git@github.com/'
    '-c', 'protocol.ssh.allow=never'
)

& git @config @args
exit $LASTEXITCODE
