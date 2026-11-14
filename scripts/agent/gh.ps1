$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force

$identity = Get-AgentIdentity
$forward = Get-ForwardedArguments $PSCommandPath $args
$environment = @{
    GH_TOKEN                = (New-AgentToken).token
    GH_REPO                 = "$($identity.Owner)/$($identity.Repo)"
    GH_PROMPT_DISABLED      = '1'
    GITHUB_TOKEN            = $null
    GH_ENTERPRISE_TOKEN     = $null
    GITHUB_ENTERPRISE_TOKEN = $null
}

Invoke-WithEnvironment $environment { & gh @forward }
exit $LASTEXITCODE
