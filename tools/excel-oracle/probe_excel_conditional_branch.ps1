param(
    [string]$WorkbookPath = "C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
    [string]$OutputPath = "C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\excel-conditional-branch.json"
)
$ErrorActionPreference = "Stop"
function Release-ComObject { param([object]$Object); if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object) } }
function Wait-Calculation { param([object]$Excel); while ([int]$Excel.CalculationState -eq 1) { Start-Sleep -Milliseconds 10 } }
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
try { $excel.AutomationSecurity = 3 } catch {}
$workbook = $null
try {
    $workbook = $excel.Workbooks.Open((Resolve-Path $WorkbookPath).Path, 0, $false)
    $excel.Calculation = -4135
    $excel.Iteration = $true
    $excel.MaxIterations = 100
    $excel.MaxChange = 0.001
    try { $excel.MultiThreadedCalculation.Enabled = $true } catch {}
    $workbook.Worksheets.Item("Inputs").Range("F7").Value2 = 300
    $excel.CalculateFullRebuild()
    Wait-Calculation $excel
    $sheet = $workbook.Worksheets.Item("CashFlow Engine")
    $i65 = $sheet.Range("I65")
    $k65 = $sheet.Range("K65")
    try {
        $andValue = $sheet.Evaluate('=AND($J$11="CC",$J$14>4)')
        $iCondition = $sheet.Evaluate('=I65="Yes"')
        $iValue = $sheet.Evaluate('=IF(AND($J$11="CC",$J$14>4),"Yes","No")')
        $kValue = $sheet.Evaluate('=IF(I65="Yes",K64-1,"")')
        $direct = $null
        $directError = $null
        try { $direct = $k65.DirectPrecedents } catch { $directError = $_.Exception.Message }
        $directAddress = $null
        if ($null -ne $direct) { $directAddress = $direct.Address($false, $false, 1, $true) }
        $result = [ordered]@{
            sheet                     = "CashFlow Engine"
            cells                     = @(
                [ordered]@{ address = "I65"; formula = [string]$i65.Formula2; value2 = $i65.Value2; text = [string]$i65.Text }
                [ordered]@{ address = "K65"; formula = [string]$k65.Formula2; value2 = $k65.Value2; text = [string]$k65.Text }
            )
            excel_evaluated_condition = [ordered]@{ and_condition = $andValue; i65_equals_yes = $iCondition; i65_selected_value = $iValue }
            excel_evaluated_k65       = $kValue
            direct_precedents         = $directAddress
            direct_precedents_error   = $directError
        }
        [System.IO.File]::WriteAllText($OutputPath, ($result | ConvertTo-Json -Depth 20) + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
        Write-Output "Generated $OutputPath"
    }
    finally {
        Release-ComObject $direct
        Release-ComObject $k65
        Release-ComObject $i65
        Release-ComObject $sheet
    }
}
finally {
    if ($null -ne $workbook) { try { $workbook.Close($false) } catch {}; Release-ComObject $workbook }
    try { $excel.Quit() } catch {}
    Release-ComObject $excel
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
