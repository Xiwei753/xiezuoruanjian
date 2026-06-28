# push.ps1 — 提交所有改动并推送到 GitHub
#
# 用法:
#   .\push.ps1                    自动提交并推送
#   .\push.ps1 -Message "xxx"     指定提交信息

param(
    [string]$Message = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

# 检查是否在 git 仓库
if (-not (Test-Path (Join-Path $scriptDir ".git"))) {
    Write-Host "Error: Not a git repository" -ForegroundColor Red
    exit 1
}

# 读取 GitHub token
$credentialsFile = Join-Path $env:USERPROFILE ".git-credentials"
if (-not (Test-Path $credentialsFile)) {
    Write-Host "Error: ~/.git-credentials not found" -ForegroundColor Red
    exit 1
}

$credLine = Get-Content $credentialsFile -Encoding utf8 | Where-Object { $_ -match "github\.com" } | Select-Object -First 1
if ($credLine -match "https://([^:]+):([^@]+)@github\.com") {
    $user = $Matches[1]
    $token = $Matches[2]
} else {
    Write-Host "Error: Cannot parse GitHub token from credentials file" -ForegroundColor Red
    exit 1
}

$remote = "https://${user}:${token}@github.com/Xiwei753/xiezuoruanjian.git"

# 获取当前分支
$branch = & git -C $scriptDir rev-parse --abbrev-ref HEAD
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: Failed to get current branch" -ForegroundColor Red
    exit 1
}

# 检查是否有改动
$status = & git -C $scriptDir status --porcelain
if (-not $status) {
    Write-Host "No changes to commit."
    # 检查是否有未推送的提交
    $behind = & git -C $scriptDir rev-list --count "HEAD..origin/$branch" 2>$null
    if ($behind -eq "" -or $behind -eq "0") {
        Write-Host "Already up to date."
        exit 0
    }
} else {
    # 自动提交
    & git -C $scriptDir add -A
    if ($Message -ne "") {
        & git -C $scriptDir commit -m $Message
    } else {
        # 自动生成提交信息
        $files = ($status | ForEach-Object { $_.Trim().Substring(2) }) -join ", "
        $autoMsg = "chore: update $files"
        & git -C $scriptDir commit -m $autoMsg
    }
    Write-Host "Committed."
}

# 拉取远端更新（rebase）
Write-Host "Pulling from remote..."
& git -C $scriptDir pull $remote $branch --rebase
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: Pull failed, resolve conflicts manually" -ForegroundColor Red
    exit 1
}

# 推送
Write-Host "Pushing to $branch..."
& git -C $scriptDir push $remote "${branch}:main"
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: Push failed" -ForegroundColor Red
    exit 1
}

Write-Host "Done! Pushed to main." -ForegroundColor Green
