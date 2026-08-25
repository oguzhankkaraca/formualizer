param(
    [string]$WorkbookPath = "C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
function Release-ComObject { param([object]$Object); if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object) } }
function Wait-Calculation { param([object]$Excel); while ([int]$Excel.CalculationState -eq 1) { Start-Sleep -Milliseconds 10 } }
$source = (Resolve-Path $WorkbookPath).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) { $OutputPath = Join-Path $env:TEMP ("excel-heavy-f7-" + $PID + ".xlsx") }
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
try { $excel.AutomationSecurity = 3 } catch {}
try { $excel.AskToUpdateLinks = $false } catch {}
$workbook = $null
try {
    $workbook = $excel.Workbooks.Open($source, 0, $false)
    $excel.Calculation = -4135
    $excel.Iteration = $true
    $excel.MaxIterations = 100
    $excel.MaxChange = 0.001
    try { $excel.MultiThreadedCalculation.Enabled = $true } catch {}
    $worksheet = $workbook.Worksheets.Item("Inputs")
    try { $worksheet.Range("F7").Value2 = 300 } finally { Release-ComObject $worksheet }
    $excel.CalculateFullRebuild()
    Wait-Calculation $excel
    $workbook.SaveCopyAs($OutputPath)
    Write-Output $OutputPath
}
finally {
    if ($null -ne $workbook) { try { $workbook.Close($false) } catch {}; Release-ComObject $workbook }
    try { $excel.Quit() } catch {}
    Release-ComObject $excel
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
