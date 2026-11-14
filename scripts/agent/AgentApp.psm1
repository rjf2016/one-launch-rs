Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:AppId = 5752844
$script:AppSlug = 'one-launch-agents'
$script:BotUserId = 356454087
$script:Owner = 'rjf2016'
$script:Repo = 'one-launch-rs'
$script:Api = 'https://api.github.com'
$script:DefaultKeyDir = 'D:\Coding\slop\one-launch-rs-bot'
$script:Permissions = [ordered]@{ contents = 'write'; pull_requests = 'write' }

function Get-AgentIdentity {
    [pscustomobject]@{
        AppId       = $script:AppId
        AppSlug     = $script:AppSlug
        BotUserId   = $script:BotUserId
        Owner       = $script:Owner
        Repo        = $script:Repo
        Name        = "$script:AppSlug[bot]"
        Email       = "$script:BotUserId+$script:AppSlug[bot]@users.noreply.github.com"
        Permissions = $script:Permissions
    }
}

function Get-AgentKeyPath {
    $dir = if ($env:ONE_LAUNCH_AGENT_KEY_DIR) { $env:ONE_LAUNCH_AGENT_KEY_DIR } else { $script:DefaultKeyDir }
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        throw "agent key folder not found: $dir (set ONE_LAUNCH_AGENT_KEY_DIR)"
    }
    $keys = @(Get-ChildItem -LiteralPath $dir -Filter '*.private-key.pem' -File)
    if ($keys.Count -ne 1) {
        throw "expected exactly one *.private-key.pem in $dir, found $($keys.Count)"
    }
    $keys[0].FullName
}

function ConvertTo-Base64Url {
    param([Parameter(Mandatory)][byte[]]$Bytes)
    [Convert]::ToBase64String($Bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

function New-AgentJwt {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $header = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes('{"alg":"RS256","typ":"JWT"}'))
    $claims = '{{"iat":{0},"exp":{1},"iss":"{2}"}}' -f ($now - 60), ($now + 300), $script:AppId
    $claims = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes($claims))
    $rsa = [Security.Cryptography.RSA]::Create()
    try {
        $rsa.ImportFromPem([IO.File]::ReadAllText((Get-AgentKeyPath)))
        $signature = $rsa.SignData(
            [Text.Encoding]::ASCII.GetBytes("$header.$claims"),
            [Security.Cryptography.HashAlgorithmName]::SHA256,
            [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    }
    finally {
        $rsa.Dispose()
    }
    "$header.$claims.$(ConvertTo-Base64Url $signature)"
}

function Invoke-AgentApi {
    param(
        [string]$Method = 'GET',
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Bearer,
        $Body
    )
    $request = @{
        Method    = $Method
        Uri       = "$script:Api$Path"
        UserAgent = $script:AppSlug
        Headers   = @{
            Authorization          = "Bearer $Bearer"
            Accept                 = 'application/vnd.github+json'
            'X-GitHub-Api-Version' = '2022-11-28'
        }
    }
    if ($null -ne $Body) {
        $request.Body = $Body | ConvertTo-Json -Depth 5 -Compress
        $request.ContentType = 'application/json'
    }
    try {
        Invoke-RestMethod @request
    }
    catch {
        $status = if ($_.Exception.PSObject.Properties['StatusCode']) { [int]$_.Exception.StatusCode } else { 0 }
        throw "GitHub $Method $Path failed (HTTP $status)"
    }
}

function New-AgentToken {
    param(
        [string]$Jwt = (New-AgentJwt),
        [switch]$ListingOnly
    )
    $installation = Invoke-AgentApi -Path "/repos/$script:Owner/$script:Repo/installation" -Bearer $Jwt
    $scope = if ($ListingOnly) {
        @{ permissions = @{ metadata = 'read' } }
    }
    else {
        @{ repositories = @($script:Repo); permissions = $script:Permissions }
    }
    Invoke-AgentApi -Method POST -Path "/app/installations/$($installation.id)/access_tokens" -Bearer $Jwt -Body $scope
}

function Invoke-WithEnvironment {
    param(
        [Parameter(Mandatory)][hashtable]$Environment,
        [Parameter(Mandatory)][scriptblock]$Body
    )
    $before = @{}
    foreach ($name in $Environment.Keys) {
        $before[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        foreach ($name in $Environment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $Environment[$name])
        }
        & $Body
    }
    finally {
        foreach ($name in $before.Keys) {
            [Environment]::SetEnvironmentVariable($name, $before[$name])
        }
    }
}

Export-ModuleMember -Function Get-AgentIdentity, New-AgentJwt, Invoke-AgentApi, New-AgentToken, Invoke-WithEnvironment
