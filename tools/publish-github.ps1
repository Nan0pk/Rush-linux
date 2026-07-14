[CmdletBinding()]
param(
    [string]$Owner = 'Nan0pk',
    [string]$Repo = 'Rush-linux',
    [switch]$Private
)

$ErrorActionPreference = 'Stop'

$token = if ($env:GH_TOKEN) { $env:GH_TOKEN } elseif ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { $null }
if (-not $token) {
    throw 'Set GH_TOKEN or GITHUB_TOKEN to a GitHub token with repository creation permission.'
}

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$repoUrl = "https://github.com/$Owner/$Repo"
$apiBody = @{
    name = $Repo
    private = [bool]$Private
    description = 'Future-aligned adaptive Linux distribution scaffold with optid runtime optimizer.'
    has_issues = $true
    has_projects = $true
    has_wiki = $false
    auto_init = $false
} | ConvertTo-Json

try {
    Invoke-RestMethod `
        -Method Post `
        -Uri 'https://api.github.com/user/repos' `
        -Headers @{
            Authorization = "Bearer $token"
            Accept = 'application/vnd.github+json'
            'X-GitHub-Api-Version' = '2022-11-28'
        } `
        -Body $apiBody `
        -ContentType 'application/json' | Out-Null
    Write-Host "Created $repoUrl"
} catch {
    $message = $_.Exception.Message
    if ($message -notmatch '422') {
        throw
    }
    Write-Host "Repository may already exist; continuing with push."
}

git remote remove origin 2>$null
git remote add origin "$repoUrl.git"
git push -u origin main
