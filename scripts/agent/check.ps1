$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AgentApp.psm1') -Force

$identity = Get-AgentIdentity
$failures = [Collections.Generic.List[string]]::new()

function Format-Permissions($permissions) {
    if ($null -eq $permissions) { return '(none)' }
    $pairs = foreach ($p in $permissions.PSObject.Properties) { "$($p.Name)=$($p.Value)" }
    (@($pairs) | Sort-Object) -join ' '
}

function Assert-Equal([string]$What, $Actual, $Expected) {
    $mark = if ("$Actual" -ceq "$Expected") { 'ok  ' } else { $failures.Add("${What}: expected '$Expected', got '$Actual'"); 'FAIL' }
    '{0} {1,-32} {2}' -f $mark, $What, $Actual
}

$expectedPermissions = 'contents=write metadata=read pull_requests=write'
$fullName = "$($identity.Owner)/$($identity.Repo)"

$probe = 'ONE_LAUNCH_AGENT_CHECK_PROBE'
function Get-Probe {
    if (Test-Path "env:$probe") { "set to '$((Get-Item "env:$probe").Value)'" } else { 'unset' }
}
function Set-Probe($Value) {
    if ($null -eq $Value) { Remove-Item "env:$probe" -ErrorAction Ignore } else { Set-Item "env:$probe" $Value }
}
foreach ($start in $null, 'outer') {
    foreach ($during in $null, 'inner') {
        $case = 'env {0}, call {1}' -f $(if ($null -eq $start) { 'unset' } else { 'set' }), $(if ($null -eq $during) { 'clears' } else { 'sets' })
        Set-Probe $start
        $outside = Get-Probe
        $inside = Invoke-WithEnvironment @{ $probe = $during } { Get-Probe }
        Assert-Equal "${case}: inside" $inside $(if ($null -eq $during) { 'unset' } else { "set to '$during'" })
        Assert-Equal "${case}: after" (Get-Probe) $outside
    }
}
Set-Probe 'outer'
try { Invoke-WithEnvironment @{ $probe = 'inner' } { throw 'stop' } } catch { }
Assert-Equal 'env set, call throws: after' (Get-Probe) "set to 'outer'"
Set-Probe $null

$jwt = New-AgentJwt
$app =Invoke-AgentApi -Path '/app' -Bearer $jwt
Assert-Equal 'app id' $app.id $identity.AppId
Assert-Equal 'app slug' $app.slug $identity.AppSlug
Assert-Equal 'app owner' $app.owner.login $identity.Owner
Assert-Equal 'app permissions' (Format-Permissions $app.permissions) $expectedPermissions
Assert-Equal 'app webhook events' (@($app.events) -join ' ') ''

$installations = @(Invoke-AgentApi -Path '/app/installations?per_page=100' -Bearer $jwt | ForEach-Object { $_ })
Assert-Equal 'installations' $installations.Count 1
foreach ($installation in $installations) {
    Assert-Equal 'installed on account' $installation.account.login $identity.Owner
    Assert-Equal 'repository selection' $installation.repository_selection 'selected'
    Assert-Equal 'installation permissions' (Format-Permissions $installation.permissions) $expectedPermissions
    Assert-Equal 'installation suspended' ([bool]$installation.suspended_at) $false
}

$clock = [Diagnostics.Stopwatch]::StartNew()
$grant = New-AgentToken -Jwt $jwt
$clock.Stop()
$minutes = [int][Math]::Round(([DateTimeOffset]$grant.expires_at - [DateTimeOffset]::UtcNow).TotalMinutes)
Assert-Equal 'token permissions' (Format-Permissions $grant.permissions) $expectedPermissions
Assert-Equal 'token lifetime (minutes)' $minutes 60
'     {0,-32} {1} ms' -f 'token minted in', $clock.ElapsedMilliseconds

$reach = Invoke-AgentApi -Path '/installation/repositories?per_page=100' -Bearer $grant.token
Assert-Equal 'repositories the token reaches' $reach.total_count 1
Assert-Equal 'repository the token reaches' (@($reach.repositories.full_name) -join ' ') $fullName

$listing = New-AgentToken -Jwt $jwt -ListingOnly
Assert-Equal 'listing token permissions' (Format-Permissions $listing.permissions) 'metadata=read'
$installed = Invoke-AgentApi -Path '/installation/repositories?per_page=100' -Bearer $listing.token
Assert-Equal 'repositories installed on' $installed.total_count 1
Assert-Equal 'repository installed on' (@($installed.repositories.full_name) -join ' ') $fullName

$bot = Invoke-AgentApi -Path "/users/$([Uri]::EscapeDataString($identity.Name))" -Bearer $grant.token
Assert-Equal 'bot user id' $bot.id $identity.BotUserId
'     {0,-32} {1}' -f 'commit address', $identity.Email

if ($failures.Count -gt 0) {
    ''
    $failures | ForEach-Object { "FAIL $_" }
    exit 1
}
