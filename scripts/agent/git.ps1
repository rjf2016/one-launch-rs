$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force

$identity = Get-AgentIdentity
$forward = $args
$helper = '!pwsh -NoProfile -File "{0}"' -f ((Join-Path $PSScriptRoot 'git-credential.ps1') -replace '\\', '/')

$config = @(
    '-c', "user.name=$($identity.Name)"
    '-c', "user.email=$($identity.Email)"
    '-c', 'credential.helper='
    '-c', "credential.helper=$helper"
    '-c', 'url.https://github.com/.insteadOf=git@github.com:'
    '-c', 'url.https://github.com/.insteadOf=ssh://git@github.com/'
    '-c', 'protocol.ssh.allow=never'
)

# git ignores an empty reset whose key matches the URL less closely than the entry it should clear, so each key in force is reset by name.
$repoUrl = "https://github.com/$($identity.Owner)/$($identity.Repo)"
$headerKeys = @(
    'http.extraHeader'
    'http.https://github.com/.extraHeader'
    "http.$repoUrl.extraHeader"
    "http.$repoUrl.git.extraHeader"
)
$inFolder = if ($forward.Count -ge 2 -and $forward[0] -ceq '-C') { $forward[0..1] } else { @() }
$headerKeys += @(& git @inFolder config --name-only --get-regexp '^http(\..*)?\.extraheader$')
foreach ($key in $headerKeys | Select-Object -Unique) {
    $config += '-c', "$key="
}

$environment = @{
    GIT_TERMINAL_PROMPT = '0'
    GCM_INTERACTIVE     = 'never'
    GIT_SSH             = $null
    GIT_SSH_COMMAND     = $null
    GIT_ASKPASS         = $null
    SSH_ASKPASS         = $null
}

Invoke-WithEnvironment $environment { & git @config @forward }
exit $LASTEXITCODE
