# 진단용: 빌드한 앱을 WebDriver 없이 직접 띄워 본다.
#  1) 환경 변수 TONGHAP_DATA_DIR만 주고 15초 뒤에도 살아 있는지, stdout/stderr 출력
#  2) tauri-driver·msedgedriver가 하는 것처럼 TAURI_WEBVIEW_AUTOMATION과 원격 디버그 포트를 주고,
#     WebView2가 포트를 여는지(/json/version) 확인
param([Parameter(Mandatory = $true)][string]$App)
$ErrorActionPreference = 'Continue'

function Show-Log($label, $file) {
    Write-Host "----- $label -----"
    if (Test-Path $file) { Get-Content $file -Raw | Write-Host } else { Write-Host '(없음)' }
}

function Run-Case($name, $vars, $probePort) {
    $tmp = Join-Path $env:RUNNER_TEMP "smoke-$name"
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    $out = Join-Path $tmp 'out.txt'; $err = Join-Path $tmp 'err.txt'
    foreach ($k in $vars.Keys) { Set-Item -Path "Env:$k" -Value $vars[$k] }
    $env:TONGHAP_DATA_DIR = Join-Path $tmp 'data-root'
    Write-Host "=== 사례 $name 시작 ==="
    $p = Start-Process -FilePath $App -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    Start-Sleep -Seconds 15
    $alive = -not $p.HasExited
    Write-Host "15초 뒤 프로세스 상태: $(if ($alive) { '살아 있음' } else { "종료됨(종료 코드 $($p.ExitCode))" })"
    Write-Host '--- msedgewebview2.exe 프로세스(명령줄) ---'
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Select-Object ProcessId, ParentProcessId, CommandLine | Format-List | Out-String -Width 4000 | Write-Host
    if ($probePort) {
        try {
            $r = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$probePort/json/version" -TimeoutSec 5
            Write-Host "디버그 포트 응답: $($r.Content)"
        } catch { Write-Host "디버그 포트($probePort) 응답 없음: $($_.Exception.Message)" }
    }
    Get-ChildItem -Recurse $env:TONGHAP_DATA_DIR -ErrorAction SilentlyContinue | Select-Object -First 20 FullName | Format-Table -HideTableHeaders | Out-String | Write-Host
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    # 남은 브라우저 프로세스가 다음 사례에 합류하지 않도록 모두 끝날 때까지 기다린다.
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
    Show-Log 'stdout' $out
    Show-Log 'stderr' $err
    foreach ($k in $vars.Keys) { Remove-Item -Path "Env:$k" -ErrorAction SilentlyContinue }
    Remove-Item Env:TONGHAP_DATA_DIR -ErrorAction SilentlyContinue
    return $alive
}

Write-Host '=== 실행 환경 ==='
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
Write-Host "사용자: $($id.Name), 관리자 권한 역할: $((New-Object Security.Principal.WindowsPrincipal $id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
whoami /groups | Select-String 'Mandatory Label' | Write-Host
Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$a = Run-Case 'plain' @{} $null
$b = Run-Case 'automation' @{ TAURI_WEBVIEW_AUTOMATION = 'true'; WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222' } 9222
if (-not $a) { throw '앱이 단독 실행에서 15초 안에 종료되었습니다(시작 실패).' }
if (-not $b) { throw '자동화 설정에서 앱이 종료되었습니다.' }
