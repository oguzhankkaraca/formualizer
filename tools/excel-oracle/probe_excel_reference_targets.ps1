param(
    [string]$WorkbookPath = "C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
    [string]$OutputPath = "C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\excel-reference-targets.json"
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
    $workbook.Worksheets.Item("Inputs").Range("F7").Value2 = 300
    $excel.CalculateFullRebuild()
    Wait-Calculation $excel
    $targets = @(
        [ordered]@{ sheet = "CashFlow Engine"; address = "J8"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C8,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J9"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C9,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J11"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C11,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J13"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C13,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J14"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C14,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J15"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C15,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J16"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C16,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' },
        [ordered]@{ sheet = "CashFlow Engine"; address = "J21"; reference_formula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C21,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))' }
    )
    $results = @()
    foreach ($target in $targets) {
        $worksheet = $workbook.Worksheets.Item($target.sheet)
        $cell = $worksheet.Range($target.address)
        try {
            $selected = $null
            $errorText = $null
            try { $selected = $worksheet.Evaluate($target.reference_formula) } catch { $errorText = $_.Exception.Message }
            $results += [ordered]@{
                sheet = $target.sheet
                address = $target.address
                formula = [string]$cell.Formula2
                excel_value2 = $cell.Value2
                excel_text = [string]$cell.Text
                reference_formula = $target.reference_formula
                selected_reference = if ($null -eq $selected) { $null } else { [string]$selected }
                error = $errorText
            }
        }
        finally { Release-ComObject $cell; Release-ComObject $worksheet }
    }
    $output = [ordered]@{
        schema = "formualizer.excel-reference-targets/v1"
        workbook = (Resolve-Path $WorkbookPath).Path
        input = [ordered]@{ sheet = "Inputs"; address = "F7"; value = 300 }
        iteration = [bool]$excel.Iteration
        results = $results
        note = "CELL(address,INDEX(...)) is Excel-observed selected-reference evidence; it does not enumerate Excel's full dependency graph."
    }
    [System.IO.File]::WriteAllText($OutputPath, ($output | ConvertTo-Json -Depth 20) + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Generated $OutputPath"
}
finally {
    if ($null -ne $workbook) { try { $workbook.Close($false) } catch {}; Release-ComObject $workbook }
    try { $excel.Quit() } catch {}
    Release-ComObject $excel
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
