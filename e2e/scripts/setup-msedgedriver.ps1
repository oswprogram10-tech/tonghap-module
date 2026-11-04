# WebView2 런타임과 같은 버전의 msedgedriver를 준비하고, 경로를 MSEDGEDRIVER_PATH로 내보낸다.
# msedgedriver는 WebView2(=Edge) 버전의 앞 세 자리까지 맞아야 한다는 것이 Tauri 문서의 안내다.
$ErrorActionPreference = 'Stop'

$outDir = Join-Path $env:RUNNER_TEMP 'msedgedriver'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# 1) 설치된 WebView2 런타임 버전 (없으면 Edge 버전)
$webview2Guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$version = $null
foreach ($key in @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$webview2Guid",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$webview2Guid",
    "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$webview2Guid"
)) {
    if (Test-Path $key) { $version = (Get-ItemProperty $key).pv; if ($version) { break } }
}
if (-not $version) {
    $version = (Get-ItemProperty 'HKCU:\Software\Microsoft\Edge\BLBeacon' -ErrorAction SilentlyContinue).version
}
if (-not $version) { throw 'WebView2/Edge 버전을 찾을 수 없습니다.' }
$major = $version.Split('.')[0]
Write-Host "WebView2 버전: $version (주 버전 $major)"

function Get-DriverVersion($exe) {
    if (-not (Test-Path $exe)) { return $null }
    $text = & $exe --version
    if ($text -match '(\d+\.\d+\.\d+\.\d+)') { return $Matches[1] }
    return $null
}

# 2) 러너에 이미 있는 드라이버(EDGEWEBDRIVER)가 주 버전까지 같으면 그대로 쓴다.
$driverExe = $null
if ($env:EDGEWEBDRIVER) {
    $existing = Join-Path $env:EDGEWEBDRIVER 'msedgedriver.exe'
    $existingVersion = Get-DriverVersion $existing
    Write-Host "러너의 msedgedriver: $existingVersion"
    if ($existingVersion -and $existingVersion.Split('.')[0] -eq $major) { $driverExe = $existing }
}

# 3) 아니면 같은 버전을 내려받는다. 그 정확한 빌드가 없으면 같은 주 버전의 최신 빌드를 쓴다.
if (-not $driverExe) {
    $base = 'https://msedgedriver.microsoft.com'
    $zip = Join-Path $outDir 'edgedriver_win64.zip'
    $candidates = @($version)
    try {
        # LATEST_RELEASE_<주버전>_WINDOWS 는 UTF-16 텍스트다.
        $bytes = (Invoke-WebRequest -UseBasicParsing "$base/LATEST_RELEASE_${major}_WINDOWS").Content
        $latest = if ($bytes -is [byte[]]) { [Text.Encoding]::Unicode.GetString($bytes) } else { [string]$bytes }
        $latest = ($latest -replace '[^\d\.]', '').Trim()
        if ($latest -and $latest -ne $version) { $candidates += $latest }
    } catch { Write-Host "최신 빌드 조회 실패: $($_.Exception.Message)" }

    $downloaded = $false
    foreach ($v in $candidates) {
        try {
            Write-Host "msedgedriver $v 내려받는 중"
            Invoke-WebRequest -UseBasicParsing "$base/$v/edgedriver_win64.zip" -OutFile $zip
            $downloaded = $true
            break
        } catch { Write-Host "실패($v): $($_.Exception.Message)" }
    }
    if (-not $downloaded) { throw "WebView2 $version 에 맞는 msedgedriver를 내려받지 못했습니다." }
    Expand-Archive -Path $zip -DestinationPath $outDir -Force
    $driverExe = Join-Path $outDir 'msedgedriver.exe'
}

$got = Get-DriverVersion $driverExe
Write-Host "사용할 msedgedriver: $driverExe ($got)"
if (-not $got -or $got.Split('.')[0] -ne $major) { throw "msedgedriver($got)와 WebView2($version)의 주 버전이 다릅니다." }

"MSEDGEDRIVER_PATH=$driverExe" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
