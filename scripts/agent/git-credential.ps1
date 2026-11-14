$ErrorActionPreference = 'Stop'
if ($args.Count -lt 1 -or $args[0] -ne 'get') { exit 0 }

$ask = @{}
foreach ($line in ([Console]::In.ReadToEnd() -split "`r?`n")) {
    $at = $line.IndexOf('=')
    if ($at -gt 0) { $ask[$line.Substring(0, $at)] = $line.Substring($at + 1) }
}
if ($ask['protocol'] -ne 'https' -or $ask['host'] -ne 'github.com') { exit 0 }

Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force
$token = (New-AgentToken).token
# git reads a trailing CR as part of the value, so no Write-Output here
[Console]::Out.Write("username=x-access-token`npassword=$token`n")
